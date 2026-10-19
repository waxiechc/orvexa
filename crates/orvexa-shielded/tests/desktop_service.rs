//! The ORVEXA Desktop private v2 service through its HTTP router: session
//! token, phrase confirmation, local network, transfers, receipts, viewing
//! keys, locking and restart.

use axum::{body::Body, http::Request, Router};
use orvexa_shielded::desktop::{DesktopV2, SESSION_HEADER};
use serde_json::{json, Value};
use std::time::Duration;
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef-test-session";
const PASSWORD: &str = "correct horse battery staple";

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    body: Option<Value>,
    token: bool,
) -> (u16, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if token {
        req = req.header(SESSION_HEADER, TOKEN);
    }
    let req = req
        .body(body.map_or(Body::empty(), |b| Body::from(b.to_string())))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn ok(app: &Router, method: &str, path: &str, body: Option<Value>) -> Value {
    let (status, v) = call(app, method, path, body, true).await;
    assert!(status < 300, "{method} {path} -> {status} {v}");
    v
}

/// New wallet through the draft → confirm flow.
async fn create(app: &Router, name: &str) -> (String, Vec<String>) {
    let draft = ok(
        app,
        "POST",
        "/v2/desktop/wallets/new",
        Some(json!({ "name": name })),
    )
    .await;
    let words: Vec<String> = serde_json::from_value(draft["words"].clone()).unwrap();
    assert_eq!(words.len(), 24);
    let check: Vec<usize> = serde_json::from_value(draft["check"].clone()).unwrap();
    let picks: Vec<(usize, String)> = check.iter().map(|i| (*i, words[*i].clone())).collect();
    let created = ok(
        app,
        "POST",
        "/v2/desktop/wallets/confirm",
        Some(json!({ "draft": draft["draft"], "words": picks, "password": PASSWORD })),
    )
    .await;
    (created["id"].as_str().unwrap().to_owned(), words)
}

async fn wallet(app: &Router, id: &str) -> Value {
    ok(app, "GET", &format!("/v2/desktop/wallets/{id}"), None).await
}

async fn balance(app: &Router, id: &str) -> Option<u64> {
    wallet(app, id).await["balance"]
        .as_str()
        .map(|b| b.parse().unwrap())
}

async fn mined(app: &Router, txid: &str) {
    for _ in 0..200 {
        let (status, t) = call(app, "GET", &format!("/v2/tx/{txid}"), None, false).await;
        if status == 200 && !t["height"].is_null() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("transaction {txid} was not mined");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_v2_inside_the_desktop_app() {
    let dir = tempfile::tempdir().unwrap();
    let service = DesktopV2::open_with(dir.path(), TOKEN.into(), Duration::from_millis(150))
        .await
        .unwrap();
    let app = service.router();

    // Only the app window (holding the session token) may use wallet routes.
    let (status, _) = call(&app, "GET", "/v2/desktop/state", None, false).await;
    assert_eq!(status, 401);
    let state = ok(&app, "GET", "/v2/desktop/state", None).await;
    assert_eq!(state["network"]["exists"], false);
    assert_eq!(call(&app, "GET", "/v2/status", None, false).await.0, 503);

    // Wrong confirmation words are refused and nothing is stored.
    let draft = ok(
        &app,
        "POST",
        "/v2/desktop/wallets/new",
        Some(json!({ "name": "Основной" })),
    )
    .await;
    let check: Vec<usize> = serde_json::from_value(draft["check"].clone()).unwrap();
    let wrong: Vec<(usize, &str)> = check.iter().map(|i| (*i, "abandon")).collect();
    let (status, _) = call(
        &app,
        "POST",
        "/v2/desktop/wallets/confirm",
        Some(json!({ "draft": draft["draft"], "words": wrong, "password": PASSWORD })),
        true,
    )
    .await;
    assert_eq!(status, 400);
    assert!(ok(&app, "GET", "/v2/desktop/state", None).await["wallets"]
        .as_array()
        .unwrap()
        .is_empty());

    let (a, a_words) = create(&app, "Основной").await;
    let (b, _) = create(&app, "Второй").await;
    let b_address = wallet(&app, &b).await["address"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(b_address.starts_with("ovxt1"));

    // A local network whose whole test supply goes to A.
    ok(
        &app,
        "POST",
        "/v2/desktop/network/create",
        Some(json!({ "wallet": a })),
    )
    .await;
    assert_eq!(balance(&app, &a).await, Some(1_000_000));
    let (status, _) = call(
        &app,
        "POST",
        "/v2/desktop/network/create",
        Some(json!({ "wallet": a })),
        true,
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(
        call(&app, "GET", "/v2/status", None, false).await.0,
        200,
        "explorer is public"
    );

    // The recommended fee: one note in, recipient and change out = 2 actions.
    let quote = ok(
        &app,
        "GET",
        &format!("/v2/desktop/wallets/{a}/fee?amount=250000"),
        None,
    )
    .await;
    assert_eq!(quote["fee"], "10");
    assert_eq!(quote["actions"], 2);
    assert_eq!(quote["congestion"], 1);
    let (status, _) = call(
        &app,
        "GET",
        &format!("/v2/desktop/wallets/{a}/fee?amount=2000000"),
        None,
        true,
    )
    .await;
    assert_eq!(status, 400, "more than the balance has no fee");
    let conditions = call(&app, "GET", "/v2/fee", None, false).await.1;
    assert_eq!(conditions["per_action"], 5);

    // Private transfer A → B, paying the recommended fee (none given).
    let sent = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/send"),
        Some(json!({ "to": b_address, "amount": "250000" })),
    )
    .await;
    assert!(
        sent["receipt"].is_null(),
        "a private transfer discloses nothing"
    );
    let txid = sent["txid"].as_str().unwrap().to_owned();
    mined(&app, &txid).await;
    assert_eq!(balance(&app, &b).await, Some(250_000));
    assert_eq!(balance(&app, &a).await, Some(1_000_000 - 250_000 - 10));
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/send"),
        Some(json!({ "to": "ovxt1nope", "amount": "1" })),
        true,
    )
    .await;
    assert_eq!(status, 400);

    // Receipt for that payment from the sender's history, verified on chain.
    let view = wallet(&app, &a).await;
    let entry = view["activity"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "sent" && e["txid"] == txid.as_str())
        .unwrap()
        .clone();
    let r = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/receipt"),
        Some(json!({ "txid": txid, "action": entry["action"] })),
    )
    .await;
    let checked = ok(
        &app,
        "POST",
        "/v2/desktop/receipt/verify",
        Some(json!({ "receipt": r["receipt"] })),
    )
    .await;
    assert_eq!(
        (checked["value"].as_str(), checked["recipient"].as_str()),
        (Some("250000"), Some(b_address.as_str()))
    );
    let (status, _) = call(
        &app,
        "POST",
        "/v2/desktop/receipt/verify",
        Some(json!({ "receipt": "orvexa-receipt-v2:00" })),
        true,
    )
    .await;
    assert_eq!(status, 400);

    // Public payment needs consent that matches exactly.
    let (status, _) = call(&app, "POST", &format!("/v2/desktop/wallets/{a}/send"),
        Some(json!({ "to": b_address, "amount": "500", "public": { "to": b_address, "amount": "499" } })), true).await;
    assert_eq!(status, 400);
    let public = ok(&app, "POST", &format!("/v2/desktop/wallets/{a}/send"),
        Some(json!({ "to": b_address, "amount": "500", "public": { "to": b_address, "amount": "500" } }))).await;
    mined(&app, public["txid"].as_str().unwrap()).await;
    let shown = ok(
        &app,
        "POST",
        "/v2/desktop/receipt/verify",
        Some(json!({ "receipt": public["receipt"] })),
    )
    .await;
    assert_eq!(shown["value"], "500");

    // Phrase and viewing keys need the password again.
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/phrase"),
        Some(json!({ "password": "not the password" })),
        true,
    )
    .await;
    assert_eq!(status, 403);
    let words = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/phrase"),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(
        serde_json::from_value::<Vec<String>>(words["words"].clone()).unwrap(),
        a_words
    );
    let key = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{b}/viewing-key"),
        Some(json!({ "full": false, "password": PASSWORD })),
    )
    .await;
    assert!(key["key"].as_str().unwrap().starts_with("orvexa-ivk-v2:"));
    assert!(!key["scope"].as_array().unwrap().is_empty());
    let watch = ok(
        &app,
        "POST",
        "/v2/desktop/watch",
        Some(json!({ "name": "Наблюдение", "key": key["key"] })),
    )
    .await;
    let watched = wallet(&app, watch["id"].as_str().unwrap()).await;
    assert_eq!(watched["access"], "view_incoming");
    assert!(watched["balance"].is_null());
    assert_eq!(watched["received"], "250500");
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{}/send", watch["id"].as_str().unwrap()),
        Some(json!({ "to": b_address, "amount": "1" })),
        true,
    )
    .await;
    assert_eq!(status, 400, "view-only cannot send");

    // Lock, wrong password, unlock.
    ok(&app, "POST", &format!("/v2/desktop/wallets/{a}/lock"), None).await;
    assert_eq!(
        call(&app, "GET", &format!("/v2/desktop/wallets/{a}"), None, true)
            .await
            .0,
        423
    );
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/unlock"),
        Some(json!({ "password": "wrong password!!" })),
        true,
    )
    .await;
    assert_eq!(status, 403);
    let a_balance = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/unlock"),
        Some(json!({ "password": PASSWORD })),
    )
    .await["balance"]
        .clone();
    assert_eq!(
        a_balance,
        (1_000_000u64 - 250_000 - 10 - 500 - 10).to_string()
    );

    // Idle lock.
    service.lock_idle(Duration::ZERO).await;
    assert_eq!(
        call(&app, "GET", &format!("/v2/desktop/wallets/{b}"), None, true)
            .await
            .0,
        423
    );

    // Restart the app: wallets come back locked, the network resumes.
    let height = ok(&app, "GET", "/v2/desktop/state", None).await["network"]["height"].clone();
    drop(app);
    drop(service);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let service = DesktopV2::open_with(dir.path(), TOKEN.into(), Duration::from_millis(150))
        .await
        .unwrap();
    let app = service.router();
    let state = ok(&app, "GET", "/v2/desktop/state", None).await;
    assert_eq!(state["network"]["height"], height);
    assert!(
        state["wallets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w["unlocked"] == false),
        "watch-only wallets are session-only"
    );
    let unlocked = ok(
        &app,
        "POST",
        &format!("/v2/desktop/wallets/{a}/unlock"),
        Some(json!({ "password": PASSWORD })),
    )
    .await;
    assert_eq!(unlocked["balance"], a_balance);

    // Restoring B's phrase as a third wallet is refused as a duplicate only if it is the same wallet.
    let (status, _) = call(
        &app,
        "POST",
        "/v2/desktop/wallets/restore",
        Some(json!({ "name": "Копия", "phrase": a_words.join(" "), "password": PASSWORD })),
        true,
    )
    .await;
    assert_eq!(status, 409);
}

/// One computer runs the network (its only validator, API reachable over HTTP); a new
/// participant imports the network file and joins with one request.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn second_computer_joins_a_published_network() {
    let operator_dir = tempfile::tempdir().unwrap();
    let operator = DesktopV2::open_with(
        operator_dir.path(),
        TOKEN.into(),
        Duration::from_millis(150),
    )
    .await
    .unwrap();
    let a = operator.router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public = format!("http://{}", listener.local_addr().unwrap());
    let served = a.clone();
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });

    let (alice, _) = create(&a, "Оператор").await;
    ok(
        &a,
        "POST",
        "/v2/desktop/network/create",
        Some(json!({ "wallet": alice })),
    )
    .await;
    // The operator publishes the network file with its public node address.
    let (status, profile) = call(&a, "GET", "/v2/desktop/network/profile", None, true).await;
    assert_eq!(status, 200);
    let mut profile = profile;
    profile["nodes"] = json!([public]);
    let published = profile.to_string();

    // ORVEXA lists it in the signed network directory (served like Pages).
    let release_key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let directory = orvexa_shielded::directory::Directory::new(
        vec![orvexa_shielded::profile::NetworkProfile::parse(&published).unwrap()],
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Duration::from_secs(86_400),
    )
    .to_json();
    let signature = orvexa_shielded::directory::sign(directory.as_bytes(), &release_key);
    let pages = axum::Router::new()
        .route(
            "/networks/directory.json",
            axum::routing::get(move || async move { directory.clone() }),
        )
        .route(
            "/networks/directory.json.sig",
            axum::routing::get(move || async move { signature.clone() }),
        );
    let pages_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let pages_url = format!(
        "http://{}/networks/directory.json",
        pages_listener.local_addr().unwrap()
    );
    tokio::spawn(async move { axum::serve(pages_listener, pages).await.unwrap() });

    // A tampered file is refused before anything is stored.
    let participant_dir = tempfile::tempdir().unwrap();
    let participant = DesktopV2::open_full(
        participant_dir.path(),
        TOKEN.into(),
        Duration::from_millis(150),
        Some(orvexa_shielded::desktop::DirectorySource {
            url: pages_url.clone(),
            trusted: vec![release_key.verifying_key().to_bytes()],
        }),
    )
    .await
    .unwrap();
    let b = participant.router();
    let mut forged = profile.clone();
    forged["validators"][0]["key"] = json!(hex::encode([1u8; 32]));
    let (status, _) = call(
        &b,
        "POST",
        "/v2/desktop/network/join",
        Some(json!({ "profile": forged.to_string() })),
        true,
    )
    .await;
    assert_eq!(status, 400);
    let (status, _) = call(
        &b,
        "POST",
        "/v2/desktop/network/join",
        Some(json!({})),
        true,
    )
    .await;
    assert_eq!(status, 400, "no built-in public network in this build");

    // A list signed by someone else is refused.
    let impostor = DesktopV2::open_full(
        tempfile::tempdir().unwrap().path(),
        TOKEN.into(),
        Duration::from_millis(150),
        Some(orvexa_shielded::desktop::DirectorySource {
            url: pages_url.clone(),
            trusted: vec![ed25519_dalek::SigningKey::from_bytes(&[43; 32])
                .verifying_key()
                .to_bytes()],
        }),
    )
    .await
    .unwrap();
    let (status, _) = call(
        &impostor.router(),
        "POST",
        "/v2/desktop/network/discover",
        Some(json!({})),
        true,
    )
    .await;
    assert_eq!(status, 502);

    // One button: look the network up, then join, follow, verify every block.
    let state = ok(&b, "GET", "/v2/desktop/state", None).await;
    assert_eq!(state["network"]["discovery"], true);
    let found = ok(&b, "POST", "/v2/desktop/network/discover", Some(json!({}))).await;
    assert_eq!(found["networks"].as_array().unwrap().len(), 1);
    assert_eq!(found["networks"][0]["chain_id"], profile["chain_id"]);
    let joined = ok(
        &b,
        "POST",
        "/v2/desktop/network/join",
        Some(json!({ "chain_id": profile["chain_id"] })),
    )
    .await;
    assert_eq!(joined["network"]["role"], "follower");
    assert_eq!(joined["network"]["chain_id"], profile["chain_id"]);
    let (bob, _) = create(&b, "Участник").await;
    let bob_address = wallet(&b, &bob).await["address"]
        .as_str()
        .unwrap()
        .to_owned();

    let sent = ok(
        &a,
        "POST",
        &format!("/v2/desktop/wallets/{alice}/send"),
        Some(json!({ "to": bob_address, "amount": "7000" })),
    )
    .await;
    mined(&a, sent["txid"].as_str().unwrap()).await;
    for _ in 0..200 {
        if balance(&b, &bob).await == Some(7_000) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        balance(&b, &bob).await,
        Some(7_000),
        "follower synced the payment"
    );

    // The participant's transfer is forwarded to the validator and decided.
    let alice_address = wallet(&a, &alice).await["address"]
        .as_str()
        .unwrap()
        .to_owned();
    let back = ok(
        &b,
        "POST",
        &format!("/v2/desktop/wallets/{bob}/send"),
        Some(json!({ "to": alice_address, "amount": "2000" })),
    )
    .await;
    mined(&a, back["txid"].as_str().unwrap()).await;
    for _ in 0..200 {
        if balance(&b, &bob).await == Some(7_000 - 2_000 - 10) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(balance(&b, &bob).await, Some(4_990));
    let state = ok(&b, "GET", "/v2/desktop/state", None).await;
    assert_eq!(state["network"]["upstream"], public);
    assert!(state["network"]["upstream_height"].as_u64().is_some());

    // The participant never holds a validator key.
    assert!(!participant_dir
        .path()
        .join("node")
        .join("validator.key")
        .exists());
    assert!(operator_dir
        .path()
        .join("node")
        .join("validator.key")
        .exists());
}

/// A network file from the one-producer format cannot be used any more: the
/// app sets it aside with its node data, keeps the wallets and starts without
/// a network instead of failing to open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn old_network_format_is_set_aside() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("node")).unwrap();
    std::fs::write(dir.path().join("node").join("producer.key"), [7u8; 32]).unwrap();
    std::fs::write(
        dir.path().join("network.json"),
        r#"{"format":"orvexa-v2-network","version":1,"name":"old","chain_id":"00","salt":"00","supply":1,"anchor_window":1,"max_expiry_delta":1,"producer":"00","genesis":"00","nodes":[]}"#,
    )
    .unwrap();
    let service = DesktopV2::open_with(dir.path(), TOKEN.into(), Duration::from_millis(150))
        .await
        .unwrap();
    let state = ok(&service.router(), "GET", "/v2/desktop/state", None).await;
    assert_eq!(state["network"]["exists"], false);
    assert!(!dir.path().join("network.json").exists());
    let retired: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("retired-"))
        .collect();
    assert_eq!(retired.len(), 1);
    assert!(retired[0].path().join("node").join("producer.key").exists());
}
