//! Stage 7 acceptance on real processes: producer + follower nodes, wallets
//! over the HTTP API only, A → B → C and back, lookup by hash, double spend
//! through the API, SIGKILL restart of both nodes, restore on another
//! "device", and a scan of every API response and log for protected data.

use orvexa_shielded::{
    client::{ClientError, NodeClient},
    envelope::Envelope,
    seed_wallet::SeedWallet,
};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const NODE: &str = env!("CARGO_BIN_EXE_orvexa-v2-node");
const SUPPLY: u64 = 1_000_000;

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

fn start(dir: &Path, name: &str, genesis: &Path, role: &[&str]) -> Proc {
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{name}.log")))
        .unwrap();
    let mut child = Command::new(NODE)
        .args([
            "run",
            "--data-dir",
            dir.join(name).to_str().unwrap(),
            "--genesis",
            genesis.to_str().unwrap(),
        ])
        .args(["--listen", "127.0.0.1:0"])
        .args(role)
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

fn kill(mut p: Proc) {
    // SIGKILL: no graceful shutdown, the store must survive as is.
    p.child.kill().unwrap();
    p.child.wait().unwrap();
}

async fn until<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(60);
    while !check().await {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct World {
    dir: PathBuf,
    genesis: PathBuf,
    producer: Option<Proc>,
    follower: Option<Proc>,
    a: SeedWallet,
    b: SeedWallet,
    c: SeedWallet,
    /// Every JSON view and page fetched, for the privacy check.
    seen: Vec<String>,
}

impl World {
    fn p(&self) -> NodeClient {
        NodeClient::new(&self.producer.as_ref().unwrap().url)
    }
    fn f(&self) -> NodeClient {
        NodeClient::new(&self.follower.as_ref().unwrap().url)
    }

    fn start_nodes(&mut self) {
        let producer = start(
            &self.dir,
            "producer",
            &self.genesis,
            &["--produce-every-ms", "200"],
        );
        let follow = ["--follow", producer.url.as_str(), "--poll-ms", "150"];
        self.follower = Some(start(&self.dir, "follower", &self.genesis, &follow));
        self.producer = Some(producer);
    }

    async fn sync_all(&mut self) {
        let f = self.f();
        for w in [&mut self.a, &mut self.b, &mut self.c] {
            f.sync(w).await.unwrap();
        }
    }

    /// Submits through the follower (forwarded to the producer), waits until
    /// the follower has the block, then every wallet syncs from the follower.
    async fn send(&mut self, env: Envelope) -> [u8; 32] {
        let f = self.f();
        let txid = f.submit(&env).await.unwrap();
        until("inclusion on the follower", || async {
            f.tx(&txid).await.is_ok_and(|t| t.height.is_some())
        })
        .await;
        self.sync_all().await;
        txid
    }

    async fn page(&mut self, path: &str) -> String {
        let url = format!("{}{path}", self.follower.as_ref().unwrap().url);
        let body = reqwest::get(url).await.unwrap().text().await.unwrap();
        self.seen.push(body.clone());
        body
    }

    fn conserved(&self, burned: u64) {
        let held: u64 = [&self.a, &self.b, &self.c]
            .iter()
            .map(|w| w.balance().unwrap())
            .sum();
        assert_eq!(held + burned, SUPPLY);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_v2_end_to_end() {
    let dir = std::env::temp_dir().join(format!("orvexa-v2-accept-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let (a, b, c) = (
        SeedWallet::generate(),
        SeedWallet::generate(),
        SeedWallet::generate(),
    );
    let genesis = dir.join("genesis.bin");
    let made = Command::new(NODE)
        .args([
            "genesis",
            "--to",
            &hex::encode(a.address().to_raw_address_bytes()),
            "--out",
            genesis.to_str().unwrap(),
        ])
        .status()
        .unwrap();
    assert!(made.success());

    let mut w = World {
        dir: dir.clone(),
        genesis,
        producer: None,
        follower: None,
        a,
        b,
        c,
        seen: Vec::new(),
    };
    w.start_nodes();
    w.sync_all().await;
    assert_eq!(w.a.balance(), Some(SUPPLY), "test funds from genesis");
    let chain: [u8; 32] = hex::decode(w.f().status().await.unwrap().chain_id)
        .unwrap()
        .try_into()
        .unwrap();

    // A → B → C, then back C → B → A.
    let mut txids = Vec::new();
    for (from, to, amount) in [
        ("a", "b", 300_000),
        ("b", "c", 100_000),
        ("c", "b", 40_000),
        ("b", "a", 50_000),
    ] {
        let to_address = match to {
            "a" => w.a.address(),
            "b" => w.b.address(),
            _ => w.c.address(),
        };
        let wallet = match from {
            "a" => &mut w.a,
            "b" => &mut w.b,
            _ => &mut w.c,
        };
        let env = wallet.transfer(chain, to_address, amount, 10, 20).unwrap();
        txids.push(w.send(env).await);
    }
    assert_eq!(w.a.balance(), Some(SUPPLY - 300_000 - 10 + 50_000));
    assert_eq!(
        w.b.balance(),
        Some(300_000 - 100_000 - 10 + 40_000 - 50_000 - 10)
    );
    assert_eq!(w.c.balance(), Some(100_000 - 40_000 - 10));
    w.conserved(40);

    // Explorer: find each transaction by hash; block, height, confirmations.
    let tip = w.f().status().await.unwrap().height;
    for txid in &txids {
        let t = w.f().tx(txid).await.unwrap();
        let height = t.height.unwrap();
        assert_eq!(t.confirmations, tip - height + 1);
        assert_eq!(t.fee, 10);
        assert!(t.actions >= 2 && !t.nullifiers.is_empty());
        let block = w.page(&format!("/v2/blocks/{height}")).await;
        assert!(
            block.contains(&hex::encode(txid)),
            "block lists its transaction"
        );
        w.page(&format!("/v2/tx/{}", hex::encode(txid))).await;
    }
    w.page("/v2/status").await;
    w.page("/v2/blocks").await;
    assert!(w.page("/").await.contains("Обозреватель ORVEXA v2"));

    // Double spend through the API: replay, and a conflicting pending spend.
    let first = w.f().tx_envelope(&txids[0]).await.unwrap();
    match w.p().submit(&first).await {
        Err(ClientError::Rejected { status: 409, code }) => {
            assert_eq!(code, "NullifierAlreadySpent")
        }
        other => panic!("replay accepted: {other:?}"),
    }
    let mut twin = SeedWallet::from_phrase(&w.a.phrase().unwrap(), 0).unwrap();
    w.f().sync(&mut twin).await.unwrap();
    let c_address = w.c.address();
    let one = w.a.transfer(chain, c_address, 1_000, 10, 20).unwrap();
    let two = twin.transfer(chain, c_address, 2_000, 10, 20).unwrap();
    let p = w.p();
    let one_id = p.submit(&one).await.unwrap();
    match p.submit(&two).await {
        Err(ClientError::Rejected { status: 409, code }) => assert_eq!(code, "MempoolConflict"),
        other => panic!("conflicting spend accepted: {other:?}"),
    }
    until("conflict winner included", || async {
        w.f().tx(&one_id).await.is_ok_and(|t| t.height.is_some())
    })
    .await;
    w.sync_all().await;
    w.conserved(50);

    // Restart both nodes with SIGKILL; state and tip survive.
    let before = w.f().status().await.unwrap();
    kill(w.follower.take().unwrap());
    kill(w.producer.take().unwrap());
    w.start_nodes();
    let p_after = w.p().status().await.unwrap();
    assert_eq!(
        (p_after.height, &p_after.tip),
        (before.height, &before.tip),
        "producer resumed at the same tip"
    );
    until("follower resumed", || async {
        w.f().status().await.is_ok_and(|s| s.tip == before.tip)
    })
    .await;
    let env = w.c.transfer(chain, w.a.address(), 5_000, 10, 20).unwrap();
    w.send(env).await;
    w.conserved(60);

    // Restore B on another device from its 24 words and spend from it.
    let mut b2 = SeedWallet::from_phrase(&w.b.phrase().unwrap(), 0).unwrap();
    w.f().sync(&mut b2).await.unwrap();
    assert_eq!(b2.balance(), w.b.balance());
    assert_eq!(b2.spent_nullifiers(), w.b.spent_nullifiers());
    let b_before = w.b.balance().unwrap();
    let from_b2 = b2.transfer(chain, w.c.address(), 1_234, 10, 20).unwrap();
    w.send(from_b2).await;
    assert_eq!(
        w.b.balance(),
        Some(b_before - 1_234 - 10),
        "the original sees the restored wallet's spend"
    );
    let mut b_after = SeedWallet::from_phrase(&w.b.phrase().unwrap(), 0).unwrap();
    w.f().sync(&mut b_after).await.unwrap();
    assert_eq!(b_after.balance(), w.b.balance(), "a second restore agrees");

    // Protected data never appears in API views, pages or node logs.
    let mut logs = String::new();
    for name in ["producer.log", "follower.log"] {
        logs.push_str(&std::fs::read_to_string(dir.join(name)).unwrap());
    }
    let addresses: Vec<String> = [&w.a, &w.b, &w.c]
        .iter()
        .map(|x| hex::encode(x.address().to_raw_address_bytes()))
        .collect();
    for text in w.seen.iter().chain(std::iter::once(&logs)) {
        for addr in &addresses {
            assert!(!text.contains(addr), "an address leaked");
        }
    }
    for view in &w.seen {
        for amount in ["300000", "100000", "40000", "50000", "1234", "749990"] {
            assert!(
                !view.contains(amount),
                "amount {amount} appears in a public view"
            );
        }
    }
    assert!(
        logs.contains("produced block") && logs.contains("followed block"),
        "nodes logged their work"
    );
}
