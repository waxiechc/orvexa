use orvexa_core::{Genesis, Transaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey};
use orvexa_node::Node;
use orvexa_rpc::{BlockDetails, TransactionDetails};
use tempfile::tempdir;

#[tokio::test]
async fn browser_host_is_opt_in_same_origin_and_serves_only_assets() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("index.html"),
        "<!doctype html><title>ORVEXA</title>",
    )
    .unwrap();
    std::fs::write(dir.path().join("secret.wallet.json"), "never serve me").unwrap();
    let key = SecretKey::from_bytes(&[44; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([31; 32]));
    let node = Node::create(&dir.path().join("node.sqlite"), genesis.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = orvexa_node::web::router(node.shared(), dir.path(), &origin).unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();
    let page = http.get(&origin).send().await.unwrap();
    assert!(page.status().is_success());
    assert!(
        page.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );
    assert_eq!(page.headers()["cache-control"], "no-store");
    for (header, value) in [
        ("origin", "https://evil.example"),
        ("origin", "null"),
        ("host", "attacker.example"),
        ("sec-fetch-site", "cross-site"),
    ] {
        assert_eq!(
            http.get(format!("{origin}/v1/status"))
                .header(header, value)
                .send()
                .await
                .unwrap()
                .status(),
            403
        );
    }
    let tx = Transaction {
        version: VERSION,
        chain_id: genesis.chain_id(),
        sender: key.address(),
        recipient: key.address(),
        amount: 1,
        fee: 1,
        nonce: 0,
    }
    .sign(&key)
    .unwrap();
    assert!(
        http.post(format!("{origin}/v1/transactions"))
            .header("origin", &origin)
            .json(&tx)
            .send()
            .await
            .unwrap()
            .status()
            .is_success()
    );
    assert_eq!(
        http.post(format!("{origin}/v1/transactions"))
            .header("origin", "https://evil.example")
            .json(&tx)
            .send()
            .await
            .unwrap()
            .status(),
        403
    );
    for path in [
        "secret.wallet.json",
        "node.sqlite",
        "%2e%2e/secret.wallet.json",
        "v1/nonexistent",
    ] {
        assert_eq!(
            http.get(format!("{origin}/{path}"))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
    }
    task.abort();
    let _ = task.await;
}

#[tokio::test]
async fn explorer_reports_actual_pending_inclusion_confirmations_and_restart() {
    let dir = tempdir().unwrap();
    let key = SecretKey::from_bytes(&[11; 32]);
    let receiver = SecretKey::from_bytes(&[12; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([32; 32]));
    let db = dir.path().join("node.sqlite");
    let node = Node::create(&db, genesis.clone()).unwrap().shared();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = orvexa_node::router(node.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = orvexa_rpc::Client::new(&origin).unwrap();
    let signed = Transaction {
        version: VERSION,
        chain_id: genesis.chain_id(),
        sender: key.address(),
        recipient: receiver.address(),
        amount: 250,
        fee: 2,
        nonce: 0,
    }
    .sign(&key)
    .unwrap();
    let id = client.submit(&signed).await.unwrap().id;
    let pending: TransactionDetails = client.get(&format!("/v1/transactions/{id}")).await.unwrap();
    assert_eq!(pending.status, "pending");
    assert_eq!(pending.confirmations, 0);
    assert!(pending.block_height.is_none());
    let pool: Vec<TransactionDetails> = client.get("/v1/explorer/mempool").await.unwrap();
    assert_eq!(pool[0].id, id);
    let state: orvexa_rpc::UiState = client
        .get(&format!("/v1/ui-state?address={}", key.address()))
        .await
        .unwrap();
    assert_eq!(state.status.height, 0);
    assert_eq!(state.pending[0].id, id);
    assert_eq!(state.balance.unwrap().pending_nonce, 1);
    assert!(state.history.is_empty());
    for _ in 0..2 {
        let block = client.template().await.unwrap().sign(&key);
        client.commit(&block).await.unwrap();
    }
    let confirmed: TransactionDetails =
        client.get(&format!("/v1/transactions/{id}")).await.unwrap();
    assert_eq!(confirmed.status, "confirmed");
    assert_eq!(confirmed.confirmations, 2);
    assert_eq!(confirmed.block_height, Some(1));
    let state: orvexa_rpc::UiState = client
        .get(&format!("/v1/ui-state?address={}", receiver.address()))
        .await
        .unwrap();
    assert_eq!(state.status.height, 2);
    assert!(state.pending.is_empty());
    assert_eq!(state.balance.unwrap().confirmed.balance, 250);
    assert_eq!(state.history[0].id, id);
    let block: BlockDetails = client.get("/v1/blocks/1").await.unwrap();
    assert_eq!(block.id, confirmed.block_id.unwrap());
    assert_eq!(block.transaction_ids, [id]);
    let recent: Vec<BlockDetails> = client.get("/v1/explorer/blocks?before=2").await.unwrap();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].block.header.height, 1);
    let missing = reqwest::get(format!("{origin}/v1/transactions/{}", Hash32::ZERO))
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    task.abort();
    let _ = task.await;
    drop(node);
    let replayed = Node::open(&db).unwrap();
    assert_eq!(replayed.ledger.height(), 2);
    assert_eq!(replayed.ledger.account(receiver.address()).balance, 250);
    assert_eq!(replayed.ledger.burned(), 2);
}

#[test]
fn browser_container_is_native_v1_and_authenticates_before_restore() {
    let encrypted = orvexa_wallet::create_encrypted("test-password-only-42").unwrap();
    let address = orvexa_wallet::address_from_json(&encrypted).unwrap();
    assert_eq!(
        orvexa_wallet::unlock_json(&encrypted, "test-password-only-42")
            .unwrap()
            .address(),
        address
    );
    assert!(orvexa_wallet::unlock_json(&encrypted, "wrong-password").is_err());
    let dir = tempdir().unwrap();
    let path = dir.path().join("browser.wallet.json");
    std::fs::write(&path, encrypted).unwrap();
    assert_eq!(
        orvexa_wallet::unlock(&path, "test-password-only-42")
            .unwrap()
            .address(),
        address
    );
    assert!(orvexa_wallet::address_from_json(&"x".repeat(8193)).is_err());
}
