use orvexa_core::{Genesis, Transaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey};
use orvexa_node::{
    Node, SharedNode,
    mesh::{self, Network, Peer},
};
use orvexa_rpc::{Client, RuntimeStatus};
use std::{path::Path, time::Duration};
use tempfile::tempdir;
use tokio::{net::TcpListener, task::JoinHandle};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
fn key(value: u8) -> SecretKey {
    SecretKey::from_bytes(&[value; 32])
}
fn genesis() -> Genesis {
    Genesis::new(key(1).address(), key(2).address(), Hash32([90; 32]))
}
async fn listener() -> TcpListener {
    TcpListener::bind("127.0.0.1:0").await.unwrap()
}
fn url(listener: &TcpListener) -> String {
    format!("http://{}", listener.local_addr().unwrap())
}

struct Server {
    node: SharedNode,
    local: Client,
    tasks: Vec<JoinHandle<()>>,
}
impl Server {
    async fn start(
        node: Node,
        mesh_listener: TcpListener,
        urls: &[String],
        authority: bool,
        evidence: &Path,
    ) -> Self {
        let local_listener = listener().await;
        let local = Client::new(&url(&local_listener)).unwrap();
        let chain = node.ledger.genesis.chain_id();
        let node = node.shared();
        let network = Network::new(
            node.clone(),
            urls.iter().map(|s| Peer::new(s, TOKEN).unwrap()).collect(),
            authority.then(|| key(1)),
            chain,
            evidence,
            Duration::from_secs(1),
        )
        .unwrap();
        let app = orvexa_node::router(node.clone());
        let peer_app = mesh::router(node.clone(), TOKEN.to_string().into());
        let tasks = vec![
            tokio::spawn(async move {
                axum::serve(local_listener, app).await.unwrap();
            }),
            tokio::spawn(async move {
                axum::serve(mesh_listener, peer_app).await.unwrap();
            }),
            tokio::spawn(async move {
                network.run(Duration::from_secs(1)).await.unwrap();
            }),
        ];
        Self { node, local, tasks }
    }
    async fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
        for task in self.tasks {
            let _ = task.await;
        }
    }
}
async fn at_height(client: &Client, expected: u64) {
    for _ in 0..200 {
        if client.status().await.unwrap().height == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("node did not reach height {expected}");
}

#[test]
fn mesh_credentials_urls_and_chain_pin_are_explicit() {
    let directory = tempdir().unwrap();
    let token_path = directory.path().join("mesh.secret");
    mesh::create_token(&token_path).unwrap();
    let token = mesh::read_token(&token_path).unwrap();
    assert_eq!(token.len(), 64);
    assert!(mesh::create_token(&token_path).is_err());
    assert_eq!(*mesh::read_token(&token_path).unwrap(), *token);
    for address in [
        "http://100.64.0.1:4243",
        "http://100.127.255.254:4243",
        "http://[fd7a:115c:a1e0::1234]:4243",
        "http://127.0.0.1:4243",
    ] {
        assert!(Client::new_mesh(address, &token).is_ok(), "{address}");
    }
    for address in [
        "http://100.63.0.1:4243",
        "http://100.128.0.1:4243",
        "http://192.168.1.1:4243",
        "http://169.254.169.254",
        "http://0.0.0.0:4243",
        "http://example.com",
        "https://100.64.0.1",
        "http://user:pass@100.64.0.1",
        "http://100.64.0.1/a",
        "http://100.64.0.1?x=1",
        "http://[fd00::1]:4243",
    ] {
        assert!(Client::new_mesh(address, &token).is_err(), "{address}");
    }
    assert!(Client::new("http://100.64.0.1:4243").is_err());
    assert!(Client::new_mesh("http://100.64.0.1:4243", "bad-token").is_err());
    let node = Node::create(&directory.path().join("node.sqlite"), genesis())
        .unwrap()
        .shared();
    let new = |chain, authority| {
        Network::new(
            node.clone(),
            vec![],
            authority,
            chain,
            &directory.path().join("evidence"),
            Duration::from_secs(1),
        )
    };
    assert!(new(Hash32::ZERO, None).is_err());
    assert!(new(genesis().chain_id(), Some(key(2))).is_err());
    let funded = Genesis::new(key(1).address(), key(1).address(), Hash32([91; 32]));
    let funded_node = Node::create(&directory.path().join("funded.sqlite"), funded.clone())
        .unwrap()
        .shared();
    assert!(
        Network::new(
            funded_node,
            vec![],
            Some(key(1)),
            funded.chain_id(),
            &directory.path().join("funded-evidence"),
            Duration::from_secs(1)
        )
        .is_err()
    );
}

#[tokio::test]
async fn mesh_is_authenticated_read_only_bounded_and_rejects_browser_origins() {
    let directory = tempdir().unwrap();
    let node = Node::create(&directory.path().join("node.sqlite"), genesis()).unwrap();
    let bound = listener().await;
    let origin = url(&bound);
    let app = mesh::router(node.shared(), TOKEN.to_string().into());
    let task = tokio::spawn(async move {
        axum::serve(bound, app).await.unwrap();
    });
    let http = reqwest::Client::new();
    assert_eq!(
        http.get(format!("{origin}/v1/status"))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("{origin}/v1/status"))
            .bearer_auth("wrong")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        http.get(format!("{origin}/v1/status"))
            .bearer_auth(TOKEN)
            .header("origin", &origin)
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let client = Client::new_mesh(&origin, TOKEN).unwrap();
    assert_eq!(
        client.status().await.unwrap().chain_id,
        genesis().chain_id()
    );
    for path in [
        "/",
        "/v1/block-template",
        "/v1/ui-state",
        "/v1/runtime",
        "/v1/balance/address",
        "/wallet.json",
    ] {
        assert_eq!(
            http.get(format!("{origin}{path}"))
                .bearer_auth(TOKEN)
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    assert_eq!(
        http.post(format!("{origin}/v1/blocks"))
            .bearer_auth(TOKEN)
            .json(&serde_json::json!({}))
            .send()
            .await
            .unwrap()
            .status(),
        405
    );
    let mut limited = false;
    for _ in 0..100 {
        if http
            .get(format!("{origin}/v1/status"))
            .bearer_auth(TOKEN)
            .send()
            .await
            .unwrap()
            .status()
            == 429
        {
            limited = true;
            break;
        }
    }
    assert!(limited);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn three_nodes_auto_confirm_while_owner_is_offline_rejoin_and_wait_for_authority() {
    let directory = tempdir().unwrap();
    let a_listener = listener().await;
    let b_listener = listener().await;
    let c_listener = listener().await;
    let a_url = url(&a_listener);
    let b_url = url(&b_listener);
    let c_url = url(&c_listener);
    let a_path = directory.path().join("a.sqlite");
    let b_path = directory.path().join("b.sqlite");
    let a_peers = [b_url.clone(), c_url.clone()];
    let b_peers = [a_url.clone(), c_url.clone()];
    let c_peers = [a_url.clone(), b_url.clone()];
    let a_evidence = directory.path().join("a-evidence");
    let b_evidence = directory.path().join("b-evidence");
    let a = Server::start(
        Node::create(&a_path, genesis()).unwrap(),
        a_listener,
        &a_peers,
        false,
        &a_evidence,
    )
    .await;
    let b = Server::start(
        Node::create(&b_path, genesis()).unwrap(),
        b_listener,
        &b_peers,
        true,
        &b_evidence,
    )
    .await;
    let c = Server::start(
        Node::create(&directory.path().join("c.sqlite"), genesis()).unwrap(),
        c_listener,
        &c_peers,
        false,
        &directory.path().join("c-evidence"),
    )
    .await;
    let transfer = |sender: &SecretKey, recipient: &SecretKey, amount, fee, nonce| {
        Transaction {
            version: VERSION,
            chain_id: genesis().chain_id(),
            sender: sender.address(),
            recipient: recipient.address(),
            amount,
            fee,
            nonce,
        }
        .sign(sender)
        .unwrap()
    };
    let first = transfer(&key(2), &key(3), 250, 2, 0);
    a.local.submit(&first).await.unwrap();
    for s in [&a, &b, &c] {
        at_height(&s.local, 1).await;
    }
    a.stop().await; // The owner's computer is now absent; friend B owns only authority.
    c.local
        .submit(&transfer(&key(3), &key(2), 40, 1, 0))
        .await
        .unwrap();
    at_height(&b.local, 2).await;
    at_height(&c.local, 2).await;
    assert_eq!(
        c.local
            .balance(key(2).address())
            .await
            .unwrap()
            .confirmed
            .balance,
        999_788
    );
    assert_eq!(
        c.local
            .balance(key(3).address())
            .await
            .unwrap()
            .confirmed
            .balance,
        209
    );
    assert_eq!(
        c.local
            .balance(key(1).address())
            .await
            .unwrap()
            .confirmed
            .balance,
        0
    );
    let bound = TcpListener::bind(a_url.trim_start_matches("http://"))
        .await
        .unwrap();
    let a = Server::start(
        Node::open(&a_path).unwrap(),
        bound,
        &a_peers,
        false,
        &a_evidence,
    )
    .await;
    at_height(&a.local, 2).await;
    assert_eq!(
        a.local.status().await.unwrap().tip,
        c.local.status().await.unwrap().tip
    );
    assert!(a.local.submit(&first).await.is_err());
    b.stop().await; // Ordinary replicas must NOT promote themselves or sign blocks.
    c.local
        .submit(&transfer(&key(3), &key(2), 10, 1, 1))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1400)).await;
    assert_eq!(c.local.status().await.unwrap().height, 2);
    assert_eq!(a.local.status().await.unwrap().height, 2);
    let bound = TcpListener::bind(b_url.trim_start_matches("http://"))
        .await
        .unwrap();
    let b = Server::start(
        Node::open(&b_path).unwrap(),
        bound,
        &b_peers,
        true,
        &b_evidence,
    )
    .await;
    for s in [&a, &b, &c] {
        at_height(&s.local, 3).await;
    }
    let status: RuntimeStatus = b.local.get("/v1/runtime").await.unwrap();
    assert_eq!(status.role, "authority");
    assert!(!status.halted);
    assert_eq!(a.node.lock().unwrap().runtime.role, "replica");
    assert_eq!(
        c.local
            .balance(key(3).address())
            .await
            .unwrap()
            .confirmed
            .balance,
        198
    );
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

#[tokio::test]
async fn authority_conflict_is_preserved_quarantined_and_remains_halted_after_restart() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("local.sqlite");
    let evidence = directory.path().join("evidence");
    let mut local = Node::create(&path, genesis()).unwrap();
    local
        .commit(local.template().unwrap().sign(&key(1)))
        .unwrap();
    let original = local.ledger.tip();
    let mut remote = Node::create(&directory.path().join("remote.sqlite"), genesis()).unwrap();
    let mut alternative = remote.template().unwrap();
    alternative.header.timestamp = 2;
    remote.commit(alternative.sign(&key(1))).unwrap();
    let bound = listener().await;
    let peer_url = url(&bound);
    let app = mesh::router(remote.shared(), TOKEN.to_string().into());
    let task = tokio::spawn(async move {
        axum::serve(bound, app).await.unwrap();
    });
    let shared = local.shared();
    let mut network = Network::new(
        shared.clone(),
        vec![Peer::new(&peer_url, TOKEN).unwrap()],
        Some(key(1)),
        genesis().chain_id(),
        &evidence,
        Duration::from_secs(1),
    )
    .unwrap();
    network.step().await.unwrap();
    assert!(shared.lock().unwrap().runtime.halted);
    assert_eq!(shared.lock().unwrap().ledger.tip(), original);
    let proof: serde_json::Value =
        serde_json::from_slice(&std::fs::read(evidence.join("authority-conflict.json")).unwrap())
            .unwrap();
    assert_eq!(proof["height"], 1);
    assert_ne!(proof["local"], proof["remote"]);
    assert!(shared.lock().unwrap().template().is_err());
    let block = shared
        .lock()
        .unwrap()
        .ledger
        .template(vec![])
        .unwrap()
        .sign(&key(1));
    assert!(shared.lock().unwrap().commit(block).is_err());
    drop(network);
    drop(shared);
    let reopened = Node::open(&path).unwrap().shared();
    let mut network = Network::new(
        reopened.clone(),
        vec![],
        Some(key(1)),
        genesis().chain_id(),
        &evidence,
        Duration::from_secs(1),
    )
    .unwrap();
    network.step().await.unwrap();
    assert!(reopened.lock().unwrap().runtime.halted);
    assert_eq!(reopened.lock().unwrap().ledger.tip(), original);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn authority_does_not_sign_a_stale_prefix_after_newer_peer_fetch_fails() {
    let directory = tempdir().unwrap();
    let mut node = Node::create(&directory.path().join("node.sqlite"), genesis()).unwrap();
    node.submit(
        Transaction {
            version: VERSION,
            chain_id: genesis().chain_id(),
            sender: key(2).address(),
            recipient: key(3).address(),
            amount: 1,
            fee: 1,
            nonce: 0,
        }
        .sign(&key(2))
        .unwrap(),
    )
    .unwrap();
    let mut status = node.status();
    status.height = 1;
    status.tip = Hash32([94; 32]);
    let app = axum::Router::new()
        .route(
            "/v1/status",
            axum::routing::get(move || async move { axum::Json(status) }),
        )
        .route(
            "/v1/genesis",
            axum::routing::get(|| async { axum::Json(genesis()) }),
        );
    // The blocks request will fail after a newer same-genesis height was seen.
    let bound = listener().await;
    let peer_url = url(&bound);
    let task = tokio::spawn(async move {
        axum::serve(bound, app).await.unwrap();
    });
    let shared = node.shared();
    let mut network = Network::new(
        shared.clone(),
        vec![Peer::new(&peer_url, TOKEN).unwrap()],
        Some(key(1)),
        genesis().chain_id(),
        &directory.path().join("evidence"),
        Duration::from_secs(1),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(1100)).await;
    network.step().await.unwrap();
    assert_eq!(shared.lock().unwrap().ledger.height(), 0);
    assert_eq!(shared.lock().unwrap().mempool().len(), 1);
    assert_eq!(shared.lock().unwrap().runtime.peers[0].height, Some(1));
    assert!(!shared.lock().unwrap().runtime.halted);
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn foreign_network_and_forged_conflict_cannot_change_history_or_trigger_quarantine() {
    let directory = tempdir().unwrap();
    let mut node = Node::create(&directory.path().join("node.sqlite"), genesis()).unwrap();
    node.commit(node.template().unwrap().sign(&key(1))).unwrap();
    let shared = node.shared();
    let original = shared.lock().unwrap().ledger.tip();
    let mut forged = shared.lock().unwrap().template().unwrap();
    forged.header.height = 1;
    forged.header.timestamp = 99;
    forged = forged.sign(&key(4)); // A peer cannot invent authority evidence.
    let status = shared.lock().unwrap().status();
    let app = axum::Router::new()
        .route(
            "/v1/status",
            axum::routing::get(move || async move { axum::Json(status) }),
        )
        .route(
            "/v1/genesis",
            axum::routing::get(|| async { axum::Json(genesis()) }),
        )
        .route(
            "/v1/blocks",
            axum::routing::get(move || async move { axum::Json(vec![forged]) }),
        );
    let bound = listener().await;
    let peer_url = url(&bound);
    let task = tokio::spawn(async move {
        axum::serve(bound, app).await.unwrap();
    });
    let evidence = directory.path().join("evidence");
    let mut network = Network::new(
        shared.clone(),
        vec![Peer::new(&peer_url, TOKEN).unwrap()],
        None,
        genesis().chain_id(),
        &evidence,
        Duration::from_secs(1),
    )
    .unwrap();
    network.step().await.unwrap();
    assert!(!shared.lock().unwrap().runtime.halted);
    assert!(!evidence.join("authority-conflict.json").exists());
    assert_eq!(shared.lock().unwrap().ledger.tip(), original);
    task.abort();
    let _ = task.await;
    let foreign = Genesis::new(key(1).address(), key(2).address(), Hash32([92; 32]));
    let bound = listener().await;
    let peer_url = url(&bound);
    let app = mesh::router(
        Node::create(&directory.path().join("foreign.sqlite"), foreign)
            .unwrap()
            .shared(),
        TOKEN.to_string().into(),
    );
    let task = tokio::spawn(async move {
        axum::serve(bound, app).await.unwrap();
    });
    let mut network = Network::new(
        shared.clone(),
        vec![Peer::new(&peer_url, TOKEN).unwrap()],
        None,
        genesis().chain_id(),
        &evidence,
        Duration::from_secs(1),
    )
    .unwrap();
    network.step().await.unwrap();
    assert!(!shared.lock().unwrap().runtime.halted);
    assert_eq!(shared.lock().unwrap().ledger.tip(), original);
    task.abort();
    let _ = task.await;
}
