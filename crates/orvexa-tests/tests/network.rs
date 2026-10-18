use orvexa_core::{Genesis, SignedTransaction, Transaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey};
use orvexa_node::Node;
use orvexa_rpc::{Client, HistoryEntry};
use tempfile::tempdir;

struct Server {
    client: Client,
    task: tokio::task::JoinHandle<()>,
}
impl Server {
    async fn start(node: Node) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = Client::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let app = orvexa_node::router(node.shared());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self { client, task }
    }
    async fn stop(self) {
        self.task.abort();
        let _ = self.task.await;
    }
}

#[tokio::test]
async fn three_nodes_relay_pending_blocks_reject_replay_and_restart() {
    let directory = tempdir().unwrap();
    let alice = SecretKey::from_bytes(&[1; 32]);
    let bob = SecretKey::from_bytes(&[2; 32]);
    let genesis = Genesis::new(alice.address(), alice.address(), Hash32([4; 32]));
    let a =
        Server::start(Node::create(&directory.path().join("a.sqlite"), genesis.clone()).unwrap())
            .await;
    let b =
        Server::start(Node::create(&directory.path().join("b.sqlite"), genesis.clone()).unwrap())
            .await;
    let c_path = directory.path().join("c.sqlite");
    let c = Server::start(Node::create(&c_path, genesis.clone()).unwrap()).await;
    let signed = Transaction {
        version: VERSION,
        chain_id: genesis.chain_id(),
        sender: alice.address(),
        recipient: bob.address(),
        amount: 250,
        fee: 2,
        nonce: 0,
    }
    .sign(&alice)
    .unwrap();
    a.client.submit(&signed).await.unwrap();
    assert!(a.client.submit(&signed).await.is_err());
    let mut tampered = signed.clone();
    tampered.transaction.amount = 251;
    assert!(b.client.submit(&tampered).await.is_err());
    let report = orvexa_rpc::sync(&b.client, &a.client).await.unwrap();
    assert_eq!(report.transactions, 1);
    assert_eq!(report.blocks, 0);
    assert_eq!(
        orvexa_rpc::sync(&b.client, &a.client)
            .await
            .unwrap()
            .transactions,
        0
    );
    let block =
        orvexa_wallet::sign_block(&alice, &genesis, b.client.template().await.unwrap()).unwrap();
    b.client.commit(&block).await.unwrap();
    assert_eq!(
        orvexa_rpc::sync(&a.client, &b.client).await.unwrap().blocks,
        1
    );
    assert_eq!(
        orvexa_rpc::sync(&c.client, &a.client).await.unwrap().blocks,
        1
    );
    let tip = a.client.status().await.unwrap().tip;
    for client in [&a.client, &b.client, &c.client] {
        let status = client.status().await.unwrap();
        assert_eq!(status.height, 1);
        assert_eq!(status.tip, tip);
        assert_eq!(status.burned_fees, 2);
        assert_eq!(status.pending, 0);
        assert_eq!(
            client
                .balance(bob.address())
                .await
                .unwrap()
                .confirmed
                .balance,
            250
        );
        assert!(client.submit(&signed).await.is_err());
        let history: Vec<HistoryEntry> = client.history(bob.address()).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, signed.id().unwrap());
    }
    c.stop().await;
    let c = Server::start(Node::open(&c_path).unwrap()).await;
    assert_eq!(c.client.status().await.unwrap().tip, tip);
    assert_eq!(
        c.client
            .balance(bob.address())
            .await
            .unwrap()
            .confirmed
            .balance,
        250
    );
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

#[tokio::test]
async fn sync_pages_more_than_one_batch_and_rejects_foreign_genesis() {
    let directory = tempdir().unwrap();
    let key = SecretKey::from_bytes(&[1; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([5; 32]));
    let mut source = Node::create(&directory.path().join("a.sqlite"), genesis.clone()).unwrap();
    for _ in 0..11 {
        let block = source.template().unwrap().sign(&key);
        source.commit(block).unwrap();
    }
    let a = Server::start(source).await;
    let b = Server::start(Node::create(&directory.path().join("b.sqlite"), genesis).unwrap()).await;
    assert_eq!(
        orvexa_rpc::sync(&b.client, &a.client).await.unwrap().blocks,
        11
    );
    let foreign = Genesis::new(key.address(), key.address(), Hash32([6; 32]));
    let c = Server::start(Node::create(&directory.path().join("c.sqlite"), foreign).unwrap()).await;
    assert!(orvexa_rpc::sync(&c.client, &a.client).await.is_err());
    assert_eq!(c.client.status().await.unwrap().height, 0);
    a.stop().await;
    b.stop().await;
    c.stop().await;
}

#[tokio::test]
async fn rejects_conflicting_authority_history_without_replacing_tip() {
    let directory = tempdir().unwrap();
    let key = SecretKey::from_bytes(&[1; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([5; 32]));
    let mut first = Node::create(&directory.path().join("a.sqlite"), genesis.clone()).unwrap();
    let mut second = Node::create(&directory.path().join("b.sqlite"), genesis).unwrap();
    let mut alternative = second.template().unwrap();
    alternative.header.timestamp = 2;
    second.commit(alternative.sign(&key)).unwrap();
    first.commit(first.template().unwrap().sign(&key)).unwrap();
    let a = Server::start(first).await;
    let b = Server::start(second).await;
    let tip = a.client.status().await.unwrap().tip;
    assert!(orvexa_rpc::sync(&a.client, &b.client).await.is_err());
    assert_eq!(a.client.status().await.unwrap().tip, tip);
    a.stop().await;
    b.stop().await;
}

#[test]
fn client_rejects_nonlocal_urls_credentials_redirect_targets_and_hostnames() {
    for url in [
        "https://127.0.0.1:4242",
        "http://192.0.2.1:4242",
        "http://localhost:4242",
        "http://user:password@127.0.0.1:4242",
        "http://127.0.0.1:4242/path",
        "http://127.0.0.1:4242?peer=external",
    ] {
        assert!(Client::new(url).is_err(), "{url}");
    }
    assert!(Client::new("http://127.0.0.1:4242").is_ok());
    assert!(Client::new("http://[::1]:4242").is_ok());
}

#[tokio::test]
async fn api_rejects_browser_origins_malformed_signatures_and_oversized_bodies() {
    let directory = tempdir().unwrap();
    let key = SecretKey::from_bytes(&[1; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([5; 32]));
    let server = Server::start(
        Node::create(&directory.path().join("node.sqlite"), genesis.clone()).unwrap(),
    )
    .await;
    // Use a separate listener URL obtained from the server's typed client request
    // via a second controlled listener to exercise raw HTTP limits.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let node = Node::create(&directory.path().join("raw.sqlite"), genesis.clone()).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, orvexa_node::router(node.shared()))
            .await
            .unwrap();
    });
    let http = reqwest::Client::new();
    let response = http
        .get(format!("{origin}/v1/status"))
        .header("origin", "https://example.test")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
    let signed = SignedTransaction {
        transaction: Transaction {
            version: VERSION,
            chain_id: genesis.chain_id(),
            sender: key.address(),
            recipient: key.address(),
            amount: 1,
            fee: 1,
            nonce: 0,
        },
        signature: "00".repeat(64),
    };
    assert!(server.client.submit(&signed).await.is_err());
    let response = http
        .post(format!("{origin}/v1/transactions"))
        .header("content-type", "application/json")
        .body(" ".repeat(orvexa_rpc::MAX_BODY + 1))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(server.client.status().await.unwrap().pending, 0);
    task.abort();
    let _ = task.await;
    server.stop().await;
}

#[tokio::test]
async fn malformed_peer_page_fails_instead_of_looping_without_progress() {
    use axum::{Json, Router, routing::get};
    let directory = tempdir().unwrap();
    let key = SecretKey::from_bytes(&[1; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([5; 32]));
    let local = Node::create(&directory.path().join("local.sqlite"), genesis).unwrap();
    let mut claimed_status = local.status();
    claimed_status.height = 1;
    let mut invalid_page_block = local.template().unwrap();
    invalid_page_block.header.height = 2;
    let app = Router::new()
        .route(
            "/v1/status",
            get(move || {
                let status = claimed_status.clone();
                async move { Json(status) }
            }),
        )
        .route(
            "/v1/blocks",
            get(move || {
                let block = invalid_page_block.clone();
                async move { Json(vec![block]) }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = Client::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let server = Server::start(local).await;
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        orvexa_rpc::sync(&server.client, &peer),
    )
    .await
    .expect("malformed peer must not cause an endless sync loop");
    assert!(result.is_err());
    assert_eq!(server.client.status().await.unwrap().height, 0);
    task.abort();
    let _ = task.await;
    server.stop().await;
}
