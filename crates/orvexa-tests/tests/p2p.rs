use libp2p::identity;
use orvexa_core::{Genesis, Transaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey};
use orvexa_node::{
    Node, SharedNode,
    p2p::{self, Config, Network},
};
use std::{path::Path, time::Duration};
use tempfile::tempdir;
use tokio::task::JoinHandle;

fn key(n: u8) -> SecretKey {
    SecretKey::from_bytes(&[n; 32])
}
fn genesis() -> Genesis {
    Genesis::new(key(1).address(), key(2).address(), Hash32([71; 32]))
}
struct Running {
    node: SharedNode,
    task: JoinHandle<anyhow::Result<()>>,
    address: String,
}
impl Running {
    async fn start(path: &Path, index: usize, seeds: Vec<String>, authority: bool) -> Self {
        let db = path.join(format!("{index}.sqlite"));
        let node = if db.exists() {
            Node::open(&db)
        } else {
            Node::create(&db, genesis())
        }
        .unwrap()
        .shared();
        let config = Config {
            identity: p2p::load_identity(&path.join(format!("{index}.peerkey"))).unwrap(),
            listen: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            seeds,
            announce: vec![],
            allow_private: true,
            expected_chain: genesis().chain_id(),
            evidence_dir: path.join(format!("evidence-{index}")),
            authority: authority.then(|| key(1)),
            block_every: Duration::from_secs(1),
            sync_every: Duration::from_secs(1),
        };
        let network = Network::new(node.clone(), config).unwrap();
        let task = tokio::spawn(network.run());
        let address = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(a) = node
                    .lock()
                    .unwrap()
                    .runtime
                    .listen_addresses
                    .first()
                    .cloned()
                {
                    break a;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        Self {
            node,
            task,
            address,
        }
    }
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}
async fn height(node: &SharedNode, expected: u64) {
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if node.lock().unwrap().ledger.height() >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("native peer did not synchronize");
}
fn transfer(node: &SharedNode, sender: u8, recipient: u8, amount: u64, nonce: u64) {
    let mut n = node.lock().unwrap();
    let tx = Transaction {
        version: VERSION,
        chain_id: n.ledger.genesis.chain_id(),
        sender: key(sender).address(),
        recipient: key(recipient).address(),
        amount,
        fee: 1,
        nonce,
    }
    .sign(&key(sender))
    .unwrap();
    n.submit(tx).unwrap();
}

#[test]
fn native_identity_and_address_boundaries() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("node.peerkey");
    let id = p2p::load_identity(&file).unwrap().public().to_peer_id();
    assert_eq!(id, p2p::load_identity(&file).unwrap().public().to_peer_id());
    let suffix = format!("/tcp/4243/p2p/{id}");
    assert!(p2p::peer_address(&format!("/ip4/8.8.8.8{suffix}"), false).is_ok());
    for prefix in [
        "/ip4/127.0.0.1",
        "/ip4/192.168.1.2",
        "/ip4/169.254.169.254",
        "/ip4/100.64.0.1",
        "/ip4/0.0.0.0",
        "/ip6/::ffff:127.0.0.1",
        "/dns4/example.com",
    ] {
        assert!(
            p2p::peer_address(&format!("{prefix}{suffix}"), false).is_err(),
            "{prefix}"
        );
    }
    assert!(p2p::peer_address(&format!("/ip4/127.0.0.1{suffix}"), true).is_ok());
    assert!(p2p::peer_address(&format!("/ip4/169.254.169.254{suffix}"), true).is_err());
    assert!(p2p::peer_address("/ip4/8.8.8.8/tcp/4243", false).is_err());
    std::fs::write(&file, b"corrupt").unwrap();
    assert!(p2p::load_identity(&file).is_err());
    let n = Node::create(&dir.path().join("pin.sqlite"), genesis())
        .unwrap()
        .shared();
    let config = Config {
        identity: identity::Keypair::generate_ed25519(),
        listen: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
        seeds: vec![],
        announce: vec![],
        allow_private: true,
        expected_chain: Hash32([0; 32]),
        evidence_dir: dir.path().join("e"),
        authority: None,
        block_every: Duration::from_secs(1),
        sync_every: Duration::from_secs(1),
    };
    assert!(Network::new(n, config).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_three_nodes_transfers_and_restart_without_owner() {
    let dir = tempdir().unwrap();
    let authority = Running::start(dir.path(), 0, vec![], true).await;
    let a = Running::start(dir.path(), 1, vec![authority.address.clone()], false).await;
    let b = Running::start(dir.path(), 2, vec![authority.address.clone()], false).await;
    transfer(&a.node, 2, 3, 250, 0);
    height(&b.node, 1).await;
    assert_eq!(
        b.node
            .lock()
            .unwrap()
            .ledger
            .account(key(3).address())
            .balance,
        250
    );
    a.stop().await;
    transfer(&b.node, 3, 2, 50, 0);
    height(&authority.node, 2).await;
    let a = Running::start(dir.path(), 1, vec![authority.address.clone()], false).await;
    height(&a.node, 2).await;
    assert_eq!(
        a.node
            .lock()
            .unwrap()
            .ledger
            .account(key(3).address())
            .balance,
        199
    );
    authority.stop().await;
    transfer(&a.node, 2, 3, 1, 1);
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(a.node.lock().unwrap().ledger.height(), 2);
    let authority = Running::start(
        dir.path(),
        0,
        vec![a.address.clone(), b.address.clone()],
        true,
    )
    .await;
    height(&authority.node, 3).await;
    height(&a.node, 3).await;
    height(&b.node, 3).await;
    let tip = authority.node.lock().unwrap().ledger.tip();
    assert_eq!(a.node.lock().unwrap().ledger.tip(), tip);
    assert_eq!(b.node.lock().unwrap().ledger.tip(), tip);
    assert_eq!(b.node.lock().unwrap().ledger.burned(), 3);
    a.stop().await;
    b.stop().await;
    authority.stop().await;
    let restored = Node::open(&dir.path().join("0.sqlite")).unwrap();
    assert_eq!(restored.ledger.tip(), tip);
}

/// Real TCP/Noise connections; local capacity exercise, not an Internet claim.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_one_hundred_nodes_replicate_one_chain() {
    let dir = tempdir().unwrap();
    let authority = Running::start(dir.path(), 0, vec![], true).await;
    let mut nodes = Vec::new();
    for index in 1..100 {
        nodes.push(Running::start(dir.path(), index, vec![authority.address.clone()], false).await);
    }
    transfer(&nodes[98].node, 2, 3, 100, 0);
    for n in &nodes {
        height(&n.node, 1).await;
    }
    let tip = authority.node.lock().unwrap().ledger.tip();
    for n in &nodes {
        let n = n.node.lock().unwrap();
        assert_eq!(n.ledger.tip(), tip);
        assert_eq!(n.ledger.account(key(3).address()).balance, 100);
        assert_eq!(n.ledger.burned(), 1);
        assert!(!n.runtime.peers.is_empty());
    }
    for n in nodes {
        n.stop().await;
    }
    authority.stop().await;
}
