//! Stage 8 on real processes: four validators (`orvexa-v2-node run
//! --validator-key`) decide blocks by BFT consensus over HTTP, and a follower
//! checks every commit certificate. Faults: one validator killed (3 of 4
//! keep deciding), a second killed (no quorum: nothing is decided, nothing
//! forks), both back (work resumes and the lagging ones catch up), and all
//! four killed at once and restarted (each resumes from its WAL).
//!
//! After every phase all live nodes must agree on height and tip, and every
//! block must carry precommits of at least 3 of the 4 validators.

use orvexa_shielded::{
    client::NodeClient,
    envelope::Envelope,
    ledger::{block_id, verify_commit},
    profile::NetworkProfile,
    seed_wallet::SeedWallet,
};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NODE: &str = env!("CARGO_BIN_EXE_orvexa-v2-node");
const N: usize = 4;

struct Proc {
    child: Child,
    url: String,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start(dir: &Path, name: &str, listen: &str, args: &[&str]) -> Proc {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{name}.log")))
        .unwrap();
    let mut child = Command::new(NODE)
        .args(["run", "--data-dir", dir.join(name).to_str().unwrap()])
        .args(["--network", dir.join("network.json").to_str().unwrap()])
        .args(["--listen", listen])
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log))
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line
        .trim()
        .strip_prefix("ORVEXA_V2_LISTEN=")
        .expect("node prints its address")
        .to_owned();
    Proc { child, url }
}

struct Net {
    dir: PathBuf,
    ports: Vec<u16>,
    validators: Vec<Option<Proc>>,
    follower: Option<Proc>,
    profile: NetworkProfile,
}

impl Net {
    fn start_validator(&mut self, i: usize) {
        let key = self.dir.join(format!("v{i}.key"));
        let p = start(
            &self.dir,
            &format!("v{i}"),
            &format!("127.0.0.1:{}", self.ports[i]),
            &[
                "--validator-key",
                key.to_str().unwrap(),
                "--block-every-ms",
                "150",
                "--timeouts-ms",
                "800",
            ],
        );
        self.validators[i] = Some(p);
    }

    fn kill(&mut self, i: usize) {
        // SIGKILL: no shutdown, consensus state on disk must be enough.
        if let Some(mut p) = self.validators[i].take() {
            p.child.kill().unwrap();
            p.child.wait().unwrap();
        }
    }

    fn live(&self) -> Vec<NodeClient> {
        self.validators
            .iter()
            .flatten()
            .chain(self.follower.iter())
            .map(|p| NodeClient::new(&p.url))
            .collect()
    }

    fn any(&self) -> NodeClient {
        NodeClient::new(&self.validators.iter().flatten().next().unwrap().url)
    }

    /// Every live node at the same height and tip, at least `min`.
    async fn agree(&self, min: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut tips = Vec::new();
            for c in self.live() {
                if let Ok(s) = c.status().await {
                    tips.push((s.height, s.tip));
                }
            }
            if tips.len() == self.live().len()
                && tips[0].0 >= min
                && tips.iter().all(|t| *t == tips[0])
            {
                return tips[0].0;
            }
            assert!(Instant::now() < deadline, "nodes do not agree: {tips:?}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Every block up to `height` carries a valid certificate of ≥ 3 of 4.
    async fn check_certificates(&self, height: u64) {
        let params = self.profile.params().unwrap();
        let chain: [u8; 32] = hex::decode(&self.profile.chain_id)
            .unwrap()
            .try_into()
            .unwrap();
        let client = NodeClient::new(&self.follower.as_ref().unwrap().url);
        for h in 1..=height {
            let block = client.block(h).await.unwrap();
            assert!(
                block.commit.signatures.len() >= 3,
                "block {h} has {} precommits",
                block.commit.signatures.len()
            );
            verify_commit(&params, &chain, h, &block_id(&chain, &block), &block.commit).unwrap();
        }
    }
}

async fn included(client: &NodeClient, txid: &[u8; 32], within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if client.tx(txid).await.is_ok_and(|t| t.height.is_some()) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

struct Payer {
    wallet: SeedWallet,
    to: orchard::Address,
    chain: [u8; 32],
}

impl Payer {
    async fn pay(&mut self, via: &NodeClient, amount: u64) -> [u8; 32] {
        self.pay_env(via, amount).await.0
    }

    async fn pay_env(&mut self, via: &NodeClient, amount: u64) -> ([u8; 32], Envelope) {
        via.sync(&mut self.wallet).await.unwrap();
        let env: Envelope = self
            .wallet
            .transfer(self.chain, self.to, amount, 10, 200)
            .unwrap();
        (via.submit(&env).await.unwrap(), env)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_validators_survive_crashes() {
    let dir = std::env::temp_dir().join(format!("orvexa-v2-bft-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Each validator makes its own key and hands out the public key only.
    let ports: Vec<u16> = (0..N).map(|_| free_port()).collect();
    let mut validators = Vec::new();
    for (i, port) in ports.iter().enumerate() {
        let out = Command::new(NODE)
            .args([
                "keygen",
                "--out",
                dir.join(format!("v{i}.key")).to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        let public = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        validators.push(format!("{public}@http://127.0.0.1:{port}"));
    }
    let a = SeedWallet::generate();
    let b = SeedWallet::generate();
    let made = Command::new(NODE)
        .args([
            "genesis",
            "--to",
            &orvexa_shielded::address::encode(&a.address()),
        ])
        .args(["--validators", &validators.join(",")])
        .args(["--out", dir.join("network.json").to_str().unwrap()])
        .status()
        .unwrap();
    assert!(made.success());
    let profile =
        NetworkProfile::parse(&std::fs::read_to_string(dir.join("network.json")).unwrap()).unwrap();
    assert_eq!(profile.validators.len(), N);
    let chain: [u8; 32] = hex::decode(&profile.chain_id).unwrap().try_into().unwrap();

    let mut net = Net {
        dir: dir.clone(),
        ports,
        validators: (0..N).map(|_| None).collect(),
        follower: None,
        profile,
    };
    for i in 0..N {
        net.start_validator(i);
    }
    let follow_url = net.validators[0].as_ref().unwrap().url.clone();
    net.follower = Some(start(
        &dir,
        "follower",
        "127.0.0.1:0",
        &["--follow", &follow_url, "--poll-ms", "150"],
    ));
    let mut payer = Payer {
        wallet: a,
        to: b.address(),
        chain,
    };

    // All four: a payment submitted to one validator is decided by all.
    let tx = payer.pay(&net.any(), 1_000).await;
    assert!(
        included(&net.any(), &tx, Duration::from_secs(30)).await,
        "decided with 4 of 4"
    );
    let h = net.agree(1).await;

    // One validator down: 3 of 4 is still a quorum.
    net.kill(3);
    let tx = payer.pay(&net.any(), 1_000).await;
    assert!(
        included(&net.any(), &tx, Duration::from_secs(30)).await,
        "decided with 3 of 4"
    );
    let h = net.agree(h + 1).await;

    // Two down: no quorum. Nothing is decided and nobody forks.
    net.kill(2);
    let stalled = payer.pay(&net.any(), 1_000).await;
    assert!(
        !included(&net.any(), &stalled, Duration::from_secs(8)).await,
        "2 of 4 must not decide"
    );
    assert_eq!(net.agree(h).await, h);

    // Both back: the pending payment is decided, the lagging validators catch up.
    net.start_validator(2);
    net.start_validator(3);
    assert!(
        included(&net.any(), &stalled, Duration::from_secs(60)).await,
        "decided after recovery"
    );
    let h = net.agree(h + 1).await;

    // All four killed in the middle of a height, then restarted. Two are
    // stopped first so the other two are surely voting (and logging) on a
    // height that cannot finish yet.
    net.kill(2);
    net.kill(3);
    let (tx, env) = payer.pay_env(&net.any(), 1_000).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    for i in 0..N {
        net.kill(i);
    }
    for i in 0..N {
        net.start_validator(i);
    }
    // The height that was in progress is finished from the WALs.
    let h = net.agree(h + 1).await;
    // Mempools live in memory: with every validator gone the payment may be
    // lost, and the wallet sends it again (the same transaction, same txid).
    if !included(&net.any(), &tx, Duration::from_secs(3)).await {
        assert_eq!(net.any().submit(&env).await.unwrap(), tx);
    }
    assert!(
        included(&net.any(), &tx, Duration::from_secs(60)).await,
        "decided after a full restart"
    );
    let tx = payer.pay(&net.any(), 1_000).await;
    assert!(included(&net.any(), &tx, Duration::from_secs(30)).await);
    let h = net.agree(h + 1).await;

    net.check_certificates(h).await;
    let mut logs = String::new();
    for i in 0..N {
        logs.push_str(&std::fs::read_to_string(dir.join(format!("v{i}.log"))).unwrap());
    }
    assert!(logs.contains("committed block"));
    assert!(
        logs.contains("resuming height"),
        "a restarted validator resumed from its WAL"
    );
    assert!(
        logs.contains("synced block"),
        "a lagging validator caught up from its peers"
    );
    assert!(
        !logs.contains("consensus error"),
        "consensus reported an error"
    );
    assert!(!logs.contains("refused by the ledger"));
    if std::env::var_os("ORVEXA_KEEP_TEST_LOGS").is_none() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}
