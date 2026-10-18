//! The update service against a local server standing in for the site and
//! the GitHub release: signed manifest, checksum, rollback, session token.

use axum::{Router, body::Body, http::Request, routing::get};
use ed25519_dalek::SigningKey;
use orvexa_update::{
    SESSION_HEADER, Updater,
    manifest::{Artifact, FORMAT, Manifest, VERSION, platform, sign},
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const TOKEN: &str = "0123456789abcdef0123456789abcdef-update-test";

#[derive(Default)]
struct Site {
    manifest: Vec<u8>,
    sig: String,
    artifact: Vec<u8>,
}

async fn serve(site: Arc<Mutex<Site>>) -> String {
    let (m, s, a) = (site.clone(), site.clone(), site);
    let app = Router::new()
        .route(
            "/updates/preview.json",
            get(move || {
                let m = m.clone();
                async move { m.lock().unwrap().manifest.clone() }
            }),
        )
        .route(
            "/updates/preview.json.sig",
            get(move || {
                let s = s.clone();
                async move { s.lock().unwrap().sig.clone() }
            }),
        )
        .route(
            "/dl/{file}",
            get(move || {
                let a = a.clone();
                async move { a.lock().unwrap().artifact.clone() }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn publish(
    site: &Mutex<Site>,
    base: &str,
    key: &SigningKey,
    release: &str,
    artifact: &[u8],
    announced: &[u8],
) {
    let mut files = BTreeMap::new();
    files.insert(
        platform().to_owned(),
        Artifact {
            url: format!("{base}/dl/ORVEXA-update.bin"),
            sha256: hex::encode(Sha256::digest(announced)),
            bytes: announced.len() as u64,
        },
    );
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let manifest = serde_json::to_vec(&Manifest {
        format: FORMAT.into(),
        version: VERSION,
        channel: "preview".into(),
        release: release.into(),
        expires: now + 3600,
        notes_ru: "Тест".into(),
        notes_en: "Test".into(),
        files,
    })
    .unwrap();
    let mut s = site.lock().unwrap();
    s.sig = sign(&manifest, key);
    s.manifest = manifest;
    s.artifact = artifact.to_vec();
}

fn updater(dir: &std::path::Path, base: &str, key: &SigningKey) -> Arc<Updater> {
    Updater::with(
        dir,
        TOKEN.into(),
        Some("0.1.0-desktop-preview.4".into()),
        format!("{base}/updates/preview.json"),
        format!("{base}/dl/"),
        vec![key.verifying_key()],
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checks_downloads_and_refuses_what_it_should() {
    let key = SigningKey::from_bytes(&[11; 32]);
    let site = Arc::new(Mutex::new(Site::default()));
    let base = serve(site.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let u = updater(dir.path(), &base, &key);

    // Same version: nothing to do.
    publish(
        &site,
        &base,
        &key,
        "0.1.0-desktop-preview.4",
        b"build 4",
        b"build 4",
    );
    u.check().await.unwrap();
    assert!(u.view().await.available.is_none());

    // A file that differs from its signed checksum is discarded.
    publish(
        &site,
        &base,
        &key,
        "0.1.0-desktop-preview.5",
        b"build X",
        b"build 5",
    );
    assert!(u.check().await.is_err());
    let v = u.view().await;
    assert!(!v.ready);
    assert!(v.error.unwrap().contains("checksum"));

    // Newer and intact: downloaded and ready (a test binary is not an installed app).
    publish(
        &site,
        &base,
        &key,
        "0.1.0-desktop-preview.5",
        b"build 5",
        b"build 5",
    );
    u.check().await.unwrap();
    let v = u.view().await;
    assert_eq!(
        v.available.as_ref().unwrap().release,
        "0.1.0-desktop-preview.5"
    );
    assert!(v.ready, "{:?}", v.error);
    assert!(!v.can_install);
    assert!(
        u.install().await.is_err(),
        "never installs outside the packaged app"
    );

    // Signed by another key: ignored.
    let other = SigningKey::from_bytes(&[12; 32]);
    publish(
        &site,
        &base,
        &other,
        "0.1.0-desktop-preview.9",
        b"evil",
        b"evil",
    );
    assert!(u.check().await.unwrap_err().contains("not signed"));

    // Rollback to an older release than one already seen: refused, even if signed.
    publish(
        &site,
        &base,
        &key,
        "0.1.0-desktop-preview.3",
        b"old",
        b"old",
    );
    assert!(u.check().await.unwrap_err().contains("older"));

    // The highest seen release survives a restart.
    let reopened = updater(dir.path(), &base, &key);
    assert!(reopened.check().await.unwrap_err().contains("older"));

    // Routes need the session token.
    let app = u.router();
    let req = |token: bool| {
        let mut r = Request::builder().uri("/desktop/update/state");
        if token {
            r = r.header(SESSION_HEADER, TOKEN);
        }
        r.body(Body::empty()).unwrap()
    };
    assert_eq!(app.clone().oneshot(req(false)).await.unwrap().status(), 401);
    assert_eq!(app.oneshot(req(true)).await.unwrap().status(), 200);
}

#[test]
fn the_build_trusts_the_published_key() {
    assert_eq!(orvexa_update::trusted_keys().len(), 1);
}
