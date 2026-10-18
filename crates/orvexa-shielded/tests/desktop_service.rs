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

    // Private transfer A → B.
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
