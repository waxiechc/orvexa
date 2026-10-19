//! Private v2 service for ORVEXA Desktop: a separate local test network.
//!
//! Unlike the v1 desktop control plane (which never holds keys; v1 signs in
//! the browser), v2 wallets live in this native process because Halo2 proving
//! is too heavy for the page. Keys still never leave the device:
//!
//! * seeds are stored only as password-encrypted files (`seed_file`);
//! * an unlocked wallet exists only in this process' memory and is locked
//!   again after [`IDLE_LOCK`] without use;
//! * every `/v2/desktop/*` request must carry the per-launch session token
//!   that only the app window receives, on top of the loopback Host/Origin
//!   guard shared with v1;
//! * showing the phrase or exporting a viewing key asks for the password again.
//!
//! Public chain views (`/v2/status`, `/v2/blocks…`, `/v2/tx…`) are proxied to
//! the embedded node and carry no private data.

use crate::address;
use crate::bft::engine::{default_timeouts, EngineConfig};
use crate::disclosure::{PaymentReceipt, PublicConsent, ViewingKey};
use crate::envelope::Envelope;
use crate::ledger::Params;
use crate::node::{embed, NodeConfig, NodeHandle, Role, ValidatorKey};
use crate::profile::{create_validator_key, read_validator_key, NetworkProfile};
use crate::seed_file::{self, SeedFileError};
use crate::seed_wallet::{Access, SeedWallet, WORDS};
use crate::wallet::{build_genesis, WalletError};
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path as UrlPath, Query, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use rand::rand_core::Rng as _;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
use tower::ServiceExt;

pub const SESSION_HEADER: &str = "x-orvexa-session";
pub const IDLE_LOCK: Duration = Duration::from_secs(10 * 60);
pub const SUPPLY: u64 = 1_000_000;
pub const DEFAULT_FEE: u64 = crate::fees::PER_ACTION * crate::fees::MIN_ACTIONS as u64;
const BLOCK_EVERY: Duration = Duration::from_secs(2);
const DRAFT_TTL: Duration = Duration::from_secs(15 * 60);
const CHECK_WORDS: usize = 3;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexEntry {
    id: String,
    name: String,
    address: String,
}

struct Session {
    wallet: SeedWallet,
    used: Instant,
}

struct Draft {
    name: String,
    wallet: SeedWallet,
    check: Vec<usize>,
    created: Instant,
}

struct Inner {
    node: Option<(Router, NodeHandle)>,
    profile_name: Option<String>,
    index: Vec<IndexEntry>,
    unlocked: HashMap<String, Session>,
    watch: HashMap<String, (String, SeedWallet)>,
    drafts: HashMap<String, Draft>,
    /// Networks from the last verified directory download.
    found: Vec<NetworkProfile>,
}

/// Where the signed list of public networks is published, and the keys
/// allowed to sign it (the app's release keys).
#[derive(Clone, Debug)]
pub struct DirectorySource {
    pub url: String,
    pub trusted: Vec<[u8; 32]>,
}

pub struct DesktopV2 {
    root: PathBuf,
    token: String,
    block_every: Duration,
    directory: Option<DirectorySource>,
    http: reqwest::Client,
    inner: Mutex<Inner>,
}

/// API error: `{ "error": "…" }`, the same shape the v1 UI already reads.
#[derive(Debug)]
pub struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

fn bad(message: &str) -> Failure {
    Failure(StatusCode::BAD_REQUEST, message.into())
}

type Reply<T> = Result<Json<T>, Failure>;

impl DesktopV2 {
    /// Opens `<data>/v2`. If a local network already exists its node starts.
    pub async fn open(root: &Path, token: String) -> Result<Arc<Self>, String> {
        Self::open_with(root, token, BLOCK_EVERY).await
    }

    pub async fn open_with(
        root: &Path,
        token: String,
        block_every: Duration,
    ) -> Result<Arc<Self>, String> {
        Self::open_full(root, token, block_every, None).await
    }

    /// Like [`Self::open`], with a published network list to join from.
    pub async fn open_with_directory(
        root: &Path,
        token: String,
        directory: DirectorySource,
    ) -> Result<Arc<Self>, String> {
        Self::open_full(root, token, BLOCK_EVERY, Some(directory)).await
    }

    pub async fn open_full(
        root: &Path,
        token: String,
        block_every: Duration,
        directory: Option<DirectorySource>,
    ) -> Result<Arc<Self>, String> {
        if token.len() < 32 {
            return Err("session token must be at least 32 characters".into());
        }
        let wallets = root.join("wallets");
        std::fs::create_dir_all(&wallets).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&wallets, std::fs::Permissions::from_mode(0o700));
        }
        let index = match std::fs::read(wallets.join("index.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| "private wallet index is damaged".to_string())?,
            Err(_) => Vec::new(),
        };
        let service = Arc::new(DesktopV2 {
            root: root.into(),
            token,
            block_every,
            directory,
            http: reqwest::Client::new(),
            inner: Mutex::new(Inner {
                node: None,
                profile_name: None,
                index,
                unlocked: HashMap::new(),
                watch: HashMap::new(),
                drafts: HashMap::new(),
                found: Vec::new(),
            }),
        });
        if service.profile_path().exists() {
            let text =
                std::fs::read_to_string(service.profile_path()).map_err(|e| e.to_string())?;
            match NetworkProfile::parse(&text) {
                Ok(profile) => {
                    let node = service.start_node(&profile).await?;
                    let mut inner = service.inner.lock().await;
                    inner.node = Some(node);
                    inner.profile_name = Some(profile.name);
                }
                // A test network from an earlier format (one producer) cannot be
                // joined any more: set it aside and start without a network.
                Err(e) => service.retire_network(&e)?,
            }
        }
        let weak = Arc::downgrade(&service);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let Some(service) = weak.upgrade() else { break };
                service.lock_idle(IDLE_LOCK).await;
            }
        });
        Ok(service)
    }

    /// Moves an unusable network file and its node data to `retired-<time>`.
    /// Wallets stay: they are seeds, not tied to one network.
    fn retire_network(&self, reason: &str) -> Result<(), String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let retired = self.root.join(format!("retired-{stamp}"));
        std::fs::create_dir_all(&retired).map_err(|e| e.to_string())?;
        std::fs::rename(self.profile_path(), retired.join("network.json"))
            .map_err(|e| e.to_string())?;
        if self.root.join("node").exists() {
            std::fs::rename(self.root.join("node"), retired.join("node"))
                .map_err(|e| e.to_string())?;
        }
        eprintln!(
            "ORVEXA v2 network set aside ({reason}); moved to {}",
            retired.display()
        );
        Ok(())
    }

    /// Validator when this computer holds one of the network's validator
    /// keys, otherwise a follower of the profile's first public node.
    async fn start_node(&self, profile: &NetworkProfile) -> Result<(Router, NodeHandle), String> {
        let key_path = self.root.join("node").join("validator.key");
        let key = if key_path.exists() {
            Some(read_validator_key(&key_path)?)
        } else {
            None
        };
        let role = if let Some(key) =
            key.filter(|k| profile.is_validator(&k.verifying_key().to_bytes()))
        {
            Role::Validator {
                engine: EngineConfig {
                    peers: profile.peers_of(&key.verifying_key().to_bytes()),
                    block_every: self.block_every,
                    timeouts: default_timeouts(),
                },
                key: ValidatorKey(Arc::new(key)),
            }
        } else {
            let upstream = profile
                .public_nodes()
                .first()
                .cloned()
                .ok_or("this network lists no public node to follow")?;
            Role::Follower {
                upstream,
                poll: self.block_every.max(Duration::from_millis(500)),
            }
        };
        embed(NodeConfig {
            data_dir: self.root.join("node"),
            genesis: Some(profile.genesis()?),
            params: profile.params()?,
            listen: "127.0.0.1:0".parse().expect("address"),
            role,
        })
        .await
    }

    fn profile_path(&self) -> PathBuf {
        self.root.join("network.json")
    }

    fn save_profile(&self, profile: &NetworkProfile) -> Result<(), String> {
        let temp = self.root.join(format!(".network-{}.tmp", random_id()));
        std::fs::write(&temp, profile.to_json()).map_err(|e| e.to_string())?;
        std::fs::rename(&temp, self.profile_path()).map_err(|e| e.to_string())
    }

    /// Locks every wallet unused for longer than `idle`.
    pub async fn lock_idle(&self, idle: Duration) {
        let mut inner = self.inner.lock().await;
        inner.unlocked.retain(|_, s| s.used.elapsed() < idle);
        inner.drafts.retain(|_, d| d.created.elapsed() < DRAFT_TTL);
    }

    /// Routes to merge into the desktop app under its loopback guard.
    pub fn router(self: &Arc<Self>) -> Router {
        let private = Router::new()
            .route("/v2/desktop/state", get(state))
            .route("/v2/desktop/wallets/new", post(new_wallet))
            .route("/v2/desktop/wallets/confirm", post(confirm_wallet))
            .route("/v2/desktop/wallets/restore", post(restore_wallet))
            .route("/v2/desktop/wallets/{id}", get(wallet_view))
            .route("/v2/desktop/wallets/{id}/unlock", post(unlock))
            .route("/v2/desktop/wallets/{id}/lock", post(lock))
            .route("/v2/desktop/wallets/{id}/address", post(new_address))
            .route("/v2/desktop/wallets/{id}/send", post(send))
            .route("/v2/desktop/wallets/{id}/fee", get(fee_quote))
            .route("/v2/desktop/wallets/{id}/receipt", post(receipt))
            .route("/v2/desktop/wallets/{id}/phrase", post(phrase))
            .route("/v2/desktop/wallets/{id}/viewing-key", post(viewing_key))
            .route("/v2/desktop/watch", post(watch))
            .route("/v2/desktop/receipt/verify", post(verify_receipt))
            .route("/v2/desktop/network/create", post(create_network))
            .route("/v2/desktop/network/discover", post(discover_networks))
            .route("/v2/desktop/network/join", post(join_network))
            .route("/v2/desktop/network/profile", get(network_profile))
            .route_layer(middleware::from_fn_with_state(
                self.clone(),
                require_session,
            ))
            .layer(DefaultBodyLimit::max(16 * 1024));
        Router::new()
            .merge(private)
            .route("/v2/{*path}", any(proxy))
            .with_state(self.clone())
    }

    fn wallet_path(&self, id: &str) -> PathBuf {
        self.root.join("wallets").join(format!("{id}.orvexa-seed"))
    }

    fn save_index(&self, index: &[IndexEntry]) -> Result<(), Failure> {
        let dir = self.root.join("wallets");
        let mut tag = [0u8; 8];
        crate::keys::os_rng().fill_bytes(&mut tag);
        let temp = dir.join(format!(".index-{}.tmp", hex::encode(tag)));
        let body = serde_json::to_vec_pretty(index).expect("plain struct");
        let write = || -> std::io::Result<()> {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)?;
            f.write_all(&body)?;
            f.sync_all()?;
            std::fs::rename(&temp, dir.join("index.json"))?;
            #[cfg(unix)]
            std::fs::File::open(&dir)?.sync_all()?;
            Ok(())
        };
        write().map_err(|_| {
            Failure(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not save the private wallet list.".into(),
            )
        })
    }
}

impl Drop for DesktopV2 {
    /// Stops block production so the store is released with the service.
    fn drop(&mut self) {
        if let Some((_, handle)) = self.inner.get_mut().node.as_ref() {
            handle.stop();
        }
    }
}

async fn require_session(
    State(s): State<Arc<DesktopV2>>,
    request: Request,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(SESSION_HEADER)
        .map(|v| v.as_bytes())
        .unwrap_or_default();
    if supplied.len() != s.token.len() || !bool::from(supplied.ct_eq(s.token.as_bytes())) {
        return Failure(
            StatusCode::UNAUTHORIZED,
            "Private wallet requests are only accepted from the ORVEXA app window.".into(),
        )
        .into_response();
    }
    next.run(request).await
}

async fn proxy(State(s): State<Arc<DesktopV2>>, request: Request<Body>) -> Response {
    let api = s.inner.lock().await.node.as_ref().map(|(r, _)| r.clone());
    match api {
        Some(api) => {
            let (mut parts, body) = request.into_parts();
            parts.extensions.clear();
            api.oneshot(Request::from_parts(parts, body))
                .await
                .unwrap_or_else(|e| match e {})
        }
        None => Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "The private v2 test network has not been created yet.".into(),
        )
        .into_response(),
    }
}

// ------------------------------------------------------------ views ----

#[derive(Serialize)]
struct NetworkView {
    exists: bool,
    name: Option<String>,
    /// "validator" (this computer votes on blocks) or "follower".
    role: Option<&'static str>,
    upstream: Option<String>,
    upstream_height: Option<u64>,
    chain_id: Option<String>,
    height: Option<u64>,
    remaining_supply: Option<String>,
    mempool: usize,
    /// A built-in public test network is available to join.
    default_available: bool,
    /// This build can look up published networks (one-button join).
    discovery: bool,
}

#[derive(Serialize)]
struct WalletSummary {
    id: String,
    name: String,
    address: Option<String>,
    unlocked: bool,
    access: &'static str,
}

#[derive(Serialize)]
struct StateView {
    network: NetworkView,
    wallets: Vec<WalletSummary>,
    supply: String,
    default_fee: String,
    idle_lock_minutes: u64,
}

#[derive(Serialize)]
struct Activity {
    kind: &'static str,
    value: String,
    height: u64,
    txid: String,
    action: u8,
}

#[derive(Serialize)]
struct WalletView {
    id: String,
    name: String,
    access: &'static str,
    address: String,
    balance: Option<String>,
    pending: String,
    received: String,
    scanned_to: u64,
    activity: Vec<Activity>,
}

fn access_name(a: Access) -> &'static str {
    match a {
        Access::Spend => "spend",
        Access::ViewFull => "view_full",
        Access::ViewIncoming => "view_incoming",
    }
}

fn network_view(inner: &Inner, discovery: bool) -> NetworkView {
    let default_available = default_profile().is_some();
    match &inner.node {
        Some((_, h)) => NetworkView {
            exists: true,
            name: inner.profile_name.clone(),
            role: Some(h.role()),
            upstream: h.upstream().map(str::to_owned),
            upstream_height: h.upstream_height(),
            chain_id: Some(hex::encode(h.chain_id())),
            height: Some(h.height()),
            remaining_supply: Some(h.with_ledger(|l| l.remaining_supply()).to_string()),
            mempool: h.mempool_len(),
            default_available,
            discovery,
        },
        None => NetworkView {
            exists: false,
            name: None,
            role: None,
            upstream: None,
            upstream_height: None,
            chain_id: None,
            height: None,
            remaining_supply: None,
            mempool: 0,
            default_available,
            discovery,
        },
    }
}

/// The public test network this build ships with, if one is published.
pub fn default_profile() -> Option<NetworkProfile> {
    let text = include_str!("../networks/default.json").trim();
    if text == "null" {
        return None;
    }
    NetworkProfile::parse(text).ok()
}

fn sync(node: &Option<(Router, NodeHandle)>, wallet: &mut SeedWallet) -> Result<(), Failure> {
    let Some((_, handle)) = node else {
        return Ok(());
    };
    let blocks = handle
        .blocks_from(wallet.scanned_to())
        .map_err(|_| bad("Could not read the local v2 chain."))?;
    for (height, envs) in blocks {
        wallet
            .scan_block(height, &envs)
            .map_err(|_| bad("Could not scan the local v2 chain."))?;
    }
    Ok(())
}

fn view(id: &str, name: &str, w: &SeedWallet) -> WalletView {
    let mut activity: Vec<Activity> = w
        .notes
        .iter()
        .filter(|n| n.note.value().inner() > 0)
        .map(|n| Activity {
            kind: if n.scope == orchard::keys::Scope::Internal {
                "change"
            } else {
                "received"
            },
            value: n.note.value().inner().to_string(),
            height: n.height,
            txid: hex::encode(n.txid),
            action: n.action,
        })
        .chain(w.sent.iter().map(|p| Activity {
            kind: "sent",
            value: p.note.value().inner().to_string(),
            height: p.height,
            txid: hex::encode(p.txid),
            action: p.action,
        }))
        .collect();
    activity.sort_by(|a, b| b.height.cmp(&a.height).then(a.kind.cmp(b.kind)));
    WalletView {
        id: id.into(),
        name: name.into(),
        access: access_name(w.access()),
        address: address::encode(&w.address()),
        balance: w.balance().map(|b| b.to_string()),
        pending: w.pending().to_string(),
        received: w.received().to_string(),
        scanned_to: w.scanned_to(),
        activity,
    }
}

fn name_ok(name: &str) -> Result<String, Failure> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(bad("Enter a wallet name of up to 40 characters."));
    }
    Ok(name.to_owned())
}

fn file_error(e: SeedFileError) -> Failure {
    match e {
        SeedFileError::Decrypt => Failure(StatusCode::FORBIDDEN, "Wrong password.".into()),
        SeedFileError::WeakPassword => bad("Use a password of at least 12 characters."),
        _ => Failure(
            StatusCode::INTERNAL_SERVER_ERROR,
            "The encrypted wallet file could not be used.".into(),
        ),
    }
}

fn wallet_error(e: WalletError) -> Failure {
    bad(match e {
        WalletError::ZeroAmount => "Enter an amount greater than zero.",
        WalletError::FeeBelowMinimum => "The fee must be at least 1 atom.",
        WalletError::Overflow => "The amount is too large.",
        WalletError::InsufficientFunds => "Not enough confirmed funds for this amount and fee.",
        WalletError::MissingWitness => "Wait until the wallet has synced the private chain.",
        WalletError::ViewOnly => "A view-only wallet cannot send.",
        WalletError::ConsentMismatch => "The public payment does not match what you confirmed.",
        WalletError::Builder(_) => "The private transfer could not be built.",
    })
}

fn random_id() -> String {
    let mut b = [0u8; 16];
    crate::keys::os_rng().fill_bytes(&mut b);
    hex::encode(b)
}

/// Picks the session of an unlocked spend or watch wallet and syncs it.
fn session<'a>(inner: &'a mut Inner, id: &str) -> Result<(&'a mut SeedWallet, String), Failure> {
    let Inner {
        node,
        index,
        unlocked,
        watch,
        ..
    } = inner;
    let wallet = if let Some(s) = unlocked.get_mut(id) {
        s.used = Instant::now();
        let name = index
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        (&mut s.wallet, name)
    } else if let Some((name, w)) = watch.get_mut(id) {
        (w, name.clone())
    } else if index.iter().any(|e| e.id == id) {
        return Err(Failure(
            StatusCode::LOCKED,
            "Unlock this wallet first.".into(),
        ));
    } else {
        return Err(Failure(
            StatusCode::NOT_FOUND,
            "Unknown private wallet.".into(),
        ));
    };
    tokio::task::block_in_place(|| sync(node, wallet.0))?;
    Ok(wallet)
}

// --------------------------------------------------------- handlers ----

async fn state(State(s): State<Arc<DesktopV2>>) -> Reply<StateView> {
    let inner = s.inner.lock().await;
    let mut wallets: Vec<WalletSummary> = inner
        .index
        .iter()
        .map(|e| WalletSummary {
            id: e.id.clone(),
            name: e.name.clone(),
            address: Some(e.address.clone()),
            unlocked: inner.unlocked.contains_key(&e.id),
            access: "spend",
        })
        .collect();
    wallets.extend(inner.watch.iter().map(|(id, (name, w))| WalletSummary {
        id: id.clone(),
        name: name.clone(),
        address: Some(address::encode(&w.address())),
        unlocked: true,
        access: access_name(w.access()),
    }));
    Ok(Json(StateView {
        network: network_view(&inner, s.directory.is_some()),
        wallets,
        supply: SUPPLY.to_string(),
        default_fee: DEFAULT_FEE.to_string(),
        idle_lock_minutes: IDLE_LOCK.as_secs() / 60,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewWallet {
    name: String,
}

#[derive(Serialize)]
struct DraftView {
    draft: String,
    words: Vec<String>,
    check: Vec<usize>,
}

async fn new_wallet(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<NewWallet>,
) -> Reply<DraftView> {
    let name = name_ok(&req.name)?;
    let wallet = SeedWallet::generate();
    let words: Vec<String> = wallet
        .phrase()
        .expect("seed wallet")
        .split(' ')
        .map(str::to_owned)
        .collect();
    let mut check = Vec::new();
    while check.len() < CHECK_WORDS {
        let mut b = [0u8; 1];
        crate::keys::os_rng().fill_bytes(&mut b);
        let i = b[0] as usize % WORDS;
        if !check.contains(&i) {
            check.push(i);
        }
    }
    check.sort_unstable();
    let draft = random_id();
    s.inner.lock().await.drafts.insert(
        draft.clone(),
        Draft {
            name,
            wallet,
            check: check.clone(),
            created: Instant::now(),
        },
    );
    Ok(Json(DraftView {
        draft,
        words,
        check,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirm {
    draft: String,
    words: Vec<(usize, String)>,
    password: String,
}

#[derive(Serialize)]
struct Created {
    id: String,
}

async fn store_new(
    s: &DesktopV2,
    inner: &mut Inner,
    name: String,
    mut wallet: SeedWallet,
    password: &str,
) -> Result<String, Failure> {
    let address = address::encode(&wallet.address());
    if let Some(existing) = inner.index.iter().find(|e| e.address == address) {
        return Err(Failure(
            StatusCode::CONFLICT,
            format!("This wallet is already stored as {}.", existing.name),
        ));
    }
    let id = random_id();
    let chain = inner.node.as_ref().map_or([0; 32], |(_, h)| h.chain_id());
    let path = s.wallet_path(&id);
    tokio::task::block_in_place(|| seed_file::create(&path, &wallet, chain, password))
        .map_err(file_error)?;
    let mut index = inner.index.clone();
    index.push(IndexEntry {
        id: id.clone(),
        name,
        address,
    });
    s.save_index(&index)?;
    inner.index = index;
    tokio::task::block_in_place(|| sync(&inner.node, &mut wallet))?;
    inner.unlocked.insert(
        id.clone(),
        Session {
            wallet,
            used: Instant::now(),
        },
    );
    Ok(id)
}

async fn confirm_wallet(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<Confirm>,
) -> Reply<Created> {
    let mut inner = s.inner.lock().await;
    let draft = inner
        .drafts
        .get(&req.draft)
        .ok_or_else(|| bad("This recovery phrase has expired. Start again."))?;
    let indices: Vec<usize> = req.words.iter().map(|(i, _)| *i).collect();
    let picks: Vec<(usize, &str)> = req.words.iter().map(|(i, w)| (*i, w.as_str())).collect();
    if indices != draft.check || !draft.wallet.verify_words(&picks) {
        return Err(bad(
            "The words do not match your recovery phrase. Check them and try again.",
        ));
    }
    let draft = inner.drafts.remove(&req.draft).expect("checked above");
    let id = store_new(&s, &mut inner, draft.name, draft.wallet, &req.password).await?;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Restore {
    name: String,
    phrase: String,
    password: String,
}

async fn restore_wallet(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<Restore>,
) -> Reply<Created> {
    let name = name_ok(&req.name)?;
    let wallet = SeedWallet::from_phrase(&req.phrase, 0).map_err(|_| {
        bad("This is not a valid 24-word recovery phrase. Check every word and its order.")
    })?;
    let mut inner = s.inner.lock().await;
    let id = store_new(&s, &mut inner, name, wallet, &req.password).await?;
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    password: String,
}

async fn unlock(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<Password>,
) -> Reply<WalletView> {
    let mut inner = s.inner.lock().await;
    let entry = inner
        .index
        .iter()
        .find(|e| e.id == id)
        .cloned()
        .ok_or_else(|| Failure(StatusCode::NOT_FOUND, "Unknown private wallet.".into()))?;
    if !inner.unlocked.contains_key(&id) {
        let path = s.wallet_path(&id);
        let (mut wallet, _) =
            tokio::task::block_in_place(|| seed_file::unlock(&path, &req.password))
                .map_err(file_error)?;
        tokio::task::block_in_place(|| sync(&inner.node, &mut wallet))?;
        inner.unlocked.insert(
            id.clone(),
            Session {
                wallet,
                used: Instant::now(),
            },
        );
    }
    let (w, _) = session(&mut inner, &id)?;
    Ok(Json(view(&id, &entry.name, w)))
}

async fn lock(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
) -> Reply<serde_json::Value> {
    let mut inner = s.inner.lock().await;
    inner.unlocked.remove(&id);
    inner.watch.remove(&id);
    Ok(Json(serde_json::json!({ "locked": true })))
}

async fn wallet_view(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
) -> Reply<WalletView> {
    let mut inner = s.inner.lock().await;
    let (w, name) = session(&mut inner, &id)?;
    Ok(Json(view(&id, &name, w)))
}

async fn new_address(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
) -> Reply<serde_json::Value> {
    let mut inner = s.inner.lock().await;
    let (w, _) = session(&mut inner, &id)?;
    Ok(Json(
        serde_json::json!({ "address": address::encode(&w.new_address()) }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    to: String,
    amount: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Send {
    to: String,
    amount: String,
    fee: Option<String>,
    /// Present only when the user confirmed a public payment.
    public: Option<Consent>,
}

#[derive(Serialize)]
struct Sent {
    txid: String,
    receipt: Option<String>,
}

fn atoms(text: &str) -> Result<u64, Failure> {
    let t = text.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) || t.len() > 19 {
        return Err(bad("Enter a whole number of atoms."));
    }
    t.parse().map_err(|_| bad("Enter a whole number of atoms."))
}

async fn send(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<Send>,
) -> Reply<Sent> {
    let to = address::decode(&req.to)
        .ok_or_else(|| bad("This is not a valid ORVEXA v2 test address (ovxt1…)."))?;
    let amount = atoms(&req.amount)?;
    let fee = match req.fee.as_deref() {
        Some(text) => Some(atoms(text)?),
        None => None,
    };
    let consent = match &req.public {
        Some(c) => Some(PublicConsent {
            to: address::decode(&c.to)
                .ok_or_else(|| bad("The public payment does not match what you confirmed."))?,
            amount: atoms(&c.amount)?,
        }),
        None => None,
    };
    let mut inner = s.inner.lock().await;
    let handle = inner
        .node
        .as_ref()
        .map(|(_, h)| h.clone())
        .ok_or_else(|| bad("Create the private v2 test network first."))?;
    let chain = handle.chain_id();
    // No fee given: the recommended one, computed just like the quote shown.
    let fee = match fee {
        Some(fee) => fee,
        None => {
            let multiplier = handle.fee_conditions().await.congestion;
            let (w, _) = session(&mut inner, &id)?;
            w.recommended_fee(amount, multiplier)
                .map_err(wallet_error)?
                .0
        }
    };
    let (w, _) = session(&mut inner, &id)?;
    let (env, receipt) = tokio::task::block_in_place(|| match &consent {
        Some(c) => w
            .public_transfer(chain, c, to, amount, fee, 20)
            .map(|(e, r)| (e, Some(r))),
        None => w.transfer(chain, to, amount, fee, 20).map(|e| (e, None)),
    })
    .map_err(wallet_error)?;
    match handle.submit(&env).await {
        Ok(txid) => Ok(Json(Sent {
            txid: hex::encode(txid),
            receipt: receipt.map(|r| r.encode()),
        })),
        Err(rejection) => {
            w.release(&env);
            eprintln!("ORVEXA v2 desktop transfer refused: {}", rejection.code);
            Err(Failure(StatusCode::CONFLICT, "The private network refused the transfer. Wait for the wallet to sync and try again.".into()))
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeeQuery {
    amount: String,
}

#[derive(Serialize)]
struct FeeQuote {
    /// Recommended fee in atoms.
    fee: String,
    actions: usize,
    per_action: u64,
    /// 1 normally; 2 or 4 while many transfers wait for a block.
    congestion: u64,
    mempool: usize,
}

/// The recommended fee for sending `amount` from this wallet now.
async fn fee_quote(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Query(q): Query<FeeQuery>,
) -> Reply<FeeQuote> {
    let amount = atoms(&q.amount)?;
    let mut inner = s.inner.lock().await;
    let handle = inner
        .node
        .as_ref()
        .map(|(_, h)| h.clone())
        .ok_or_else(|| bad("Create the private v2 test network first."))?;
    let conditions = handle.fee_conditions().await;
    let (w, _) = session(&mut inner, &id)?;
    let (fee, actions) = w
        .recommended_fee(amount, conditions.congestion)
        .map_err(wallet_error)?;
    Ok(Json(FeeQuote {
        fee: fee.to_string(),
        actions,
        per_action: conditions.per_action,
        congestion: conditions.congestion,
        mempool: conditions.mempool,
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReceiptRequest {
    txid: String,
    action: u8,
}

async fn receipt(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<ReceiptRequest>,
) -> Reply<serde_json::Value> {
    let txid: [u8; 32] = hex::decode(&req.txid)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| bad("Unknown transaction."))?;
    let mut inner = s.inner.lock().await;
    let chain = inner
        .node
        .as_ref()
        .map(|(_, h)| h.chain_id())
        .ok_or_else(|| bad("Create the private v2 test network first."))?;
    let (w, _) = session(&mut inner, &id)?;
    let r = w
        .payment_receipt(chain, txid, req.action)
        .ok_or_else(|| bad("This wallet has no payment at that position."))?;
    Ok(Json(serde_json::json!({ "receipt": r.encode() })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    receipt: String,
}

async fn verify_receipt(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<VerifyRequest>,
) -> Reply<serde_json::Value> {
    let receipt = PaymentReceipt::decode(&req.receipt)
        .map_err(|_| bad("This is not a valid ORVEXA payment receipt."))?;
    let inner = s.inner.lock().await;
    let (_, handle) = inner
        .node
        .as_ref()
        .ok_or_else(|| bad("Create the private v2 test network first."))?;
    let (_, env) = handle
        .tx(&receipt.txid)
        .ok_or_else(|| bad("The receipt's transaction is not in this private chain."))?;
    let ok = handle
        .with_ledger(|l| receipt.verify(l, &env))
        .map_err(|_| bad("The receipt does not match the chain."))?;
    Ok(Json(serde_json::json!({
        "recipient": address::encode(&ok.recipient),
        "value": ok.value.to_string(),
        "height": ok.height,
        "confirmations": ok.confirmations,
        "txid": hex::encode(receipt.txid),
    })))
}

async fn reauth(s: &DesktopV2, id: &str, password: &str) -> Result<SeedWallet, Failure> {
    let path = s.wallet_path(id);
    if !path.exists() {
        return Err(Failure(
            StatusCode::NOT_FOUND,
            "Unknown private wallet.".into(),
        ));
    }
    tokio::task::block_in_place(|| seed_file::unlock(&path, password))
        .map(|(w, _)| w)
        .map_err(file_error)
}

async fn phrase(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<Password>,
) -> Reply<serde_json::Value> {
    let wallet = reauth(&s, &id, &req.password).await?;
    let words: Vec<String> = wallet
        .phrase()
        .expect("seed wallet")
        .split(' ')
        .map(str::to_owned)
        .collect();
    Ok(Json(serde_json::json!({ "words": words })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyRequest {
    full: bool,
    password: String,
}

async fn viewing_key(
    State(s): State<Arc<DesktopV2>>,
    UrlPath(id): UrlPath<String>,
    Json(req): Json<KeyRequest>,
) -> Reply<serde_json::Value> {
    let wallet = reauth(&s, &id, &req.password).await?;
    let key = wallet
        .viewing_key(req.full)
        .expect("seed wallet has both keys");
    Ok(Json(
        serde_json::json!({ "key": key.encode(), "scope": key.scope_ru() }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchRequest {
    name: String,
    key: String,
}

async fn watch(State(s): State<Arc<DesktopV2>>, Json(req): Json<WatchRequest>) -> Reply<Created> {
    let name = name_ok(&req.name)?;
    let key =
        ViewingKey::decode(&req.key).map_err(|_| bad("This is not a valid ORVEXA viewing key."))?;
    let mut wallet = SeedWallet::view_only(key);
    let mut inner = s.inner.lock().await;
    tokio::task::block_in_place(|| sync(&inner.node, &mut wallet))?;
    let id = format!("watch-{}", random_id());
    inner.watch.insert(id.clone(), (name, wallet));
    Ok(Json(Created { id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNetwork {
    wallet: String,
}

async fn create_network(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<CreateNetwork>,
) -> Reply<serde_json::Value> {
    let mut inner = s.inner.lock().await;
    if inner.node.is_some() {
        return Err(Failure(
            StatusCode::CONFLICT,
            "The private v2 test network already exists.".into(),
        ));
    }
    let recipient = inner
        .unlocked
        .get(&req.wallet)
        .map(|w| w.wallet.address())
        .ok_or_else(|| {
            Failure(
                StatusCode::LOCKED,
                "Unlock the wallet that should receive the test atoms.".into(),
            )
        })?;
    let internal = |e: String| Failure(StatusCode::INTERNAL_SERVER_ERROR, e);
    std::fs::create_dir_all(s.root.join("node")).map_err(|e| internal(e.to_string()))?;
    let key_path = s.root.join("node").join("validator.key");
    let key = if key_path.exists() {
        read_validator_key(&key_path)
    } else {
        create_validator_key(&key_path)
    }
    .map_err(internal)?;
    let mut params = Params::test(SUPPLY);
    crate::keys::os_rng().fill_bytes(&mut params.salt);
    params.validators = vec![key.verifying_key().to_bytes()];
    let genesis = tokio::task::block_in_place(|| build_genesis(&params, &[(recipient, SUPPLY)]));
    let chain_id = Envelope::decode(&genesis)
        .map_err(|e| internal(format!("{e:?}")))?
        .chain_id;
    let profile = NetworkProfile::new("Локальная сеть v2", &params, &genesis, chain_id, Vec::new());
    s.save_profile(&profile).map_err(internal)?;
    let node = s.start_node(&profile).await.map_err(internal)?;
    inner.node = Some(node);
    inner.profile_name = Some(profile.name);
    let Inner {
        node,
        unlocked,
        watch,
        ..
    } = &mut *inner;
    for session in unlocked.values_mut() {
        tokio::task::block_in_place(|| sync(node, &mut session.wallet))?;
    }
    for (_, w) in watch.values_mut() {
        tokio::task::block_in_place(|| sync(node, w))?;
    }
    Ok(Json(
        serde_json::json!({ "network": network_view(&inner, s.directory.is_some()) }),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JoinNetwork {
    /// Profile JSON text, or omitted for the built-in public test network.
    profile: Option<String>,
    /// A network from the last `discover`, by chain ID.
    chain_id: Option<String>,
}

#[derive(Serialize)]
struct FoundNetwork {
    name: String,
    chain_id: String,
    validators: usize,
    nodes: usize,
}

/// Downloads and verifies the published network list (only when asked).
async fn discover_networks(State(s): State<Arc<DesktopV2>>) -> Reply<serde_json::Value> {
    let source = s
        .directory
        .clone()
        .ok_or_else(|| bad("This build cannot look up public networks."))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let http = s.http.clone();
    let found = crate::directory::fetch(&http, &source.url, &source.trusted, now)
        .await
        .map_err(|e| {
            eprintln!("ORVEXA v2 network list: {e}");
            Failure(StatusCode::BAD_GATEWAY, e)
        })?;
    let view: Vec<FoundNetwork> = found
        .networks
        .iter()
        .map(|p| FoundNetwork {
            name: p.name.clone(),
            chain_id: p.chain_id.clone(),
            validators: p.validators.len(),
            nodes: p.public_nodes().len(),
        })
        .collect();
    s.inner.lock().await.found = found.networks;
    Ok(Json(serde_json::json!({ "networks": view })))
}

async fn join_network(
    State(s): State<Arc<DesktopV2>>,
    Json(req): Json<JoinNetwork>,
) -> Reply<serde_json::Value> {
    let profile = match (&req.profile, &req.chain_id) {
        (Some(text), None) => NetworkProfile::parse(text).map_err(|e| {
            eprintln!("ORVEXA v2 network file refused: {e}");
            bad("This network file cannot be used: it is damaged or belongs to another network.")
        })?,
        (None, Some(chain)) => s
            .inner
            .lock()
            .await
            .found
            .iter()
            .find(|p| &p.chain_id == chain)
            .cloned()
            .ok_or_else(|| bad("Look up the public networks again and pick one from the list."))?,
        (None, None) => default_profile().ok_or_else(|| bad("This build has no public v2 test network yet. Import a network file from its operator."))?,
        (Some(_), Some(_)) => return Err(bad("Choose either a network file or a listed network.")),
    };
    if profile.public_nodes().is_empty() {
        return Err(bad("This network file lists no public node to connect to."));
    }
    let mut inner = s.inner.lock().await;
    if inner.node.is_some() {
        return Err(Failure(
            StatusCode::CONFLICT,
            "The private v2 test network already exists.".into(),
        ));
    }
    let internal = |e: String| Failure(StatusCode::INTERNAL_SERVER_ERROR, e);
    s.save_profile(&profile).map_err(internal)?;
    let node = s.start_node(&profile).await.map_err(internal)?;
    inner.node = Some(node);
    inner.profile_name = Some(profile.name);
    Ok(Json(
        serde_json::json!({ "network": network_view(&inner, s.directory.is_some()) }),
    ))
}

/// The current network file, to share with others.
async fn network_profile(State(s): State<Arc<DesktopV2>>) -> Response {
    match std::fs::read_to_string(s.profile_path()) {
        Ok(text) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            text,
        )
            .into_response(),
        Err(_) => Failure(
            StatusCode::NOT_FOUND,
            "The private v2 test network has not been created yet.".into(),
        )
        .into_response(),
    }
}
