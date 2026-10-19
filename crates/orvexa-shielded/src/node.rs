//! Local v2 node: durable state ([`Store`]), mempool, a JSON API on
//! localhost and one of two roles.
//!
//! * Validator: runs BFT consensus ([`crate::bft`]) with the other
//!   validators of the network. A block is stored only with precommits of
//!   more than 2/3 of them. Transactions are passed on to the other
//!   validators; a validator that fell behind catches up from them, checking
//!   every certificate.
//! * Follower: pulls blocks from a node over HTTP and re-verifies each one
//!   (commit certificate and proofs) before storing it; it forwards submitted
//!   transactions to that node.
//!
//! Logs carry heights, txids and error codes only.

use crate::api::{
    BlockView, ErrorView, RawView, StatusView, SubmitRequest, SubmitResponse, TxView,
};
use crate::bft::codec::NetMsg;
use crate::bft::engine::{self, EngineConfig, Event, Host};
use crate::envelope::Envelope;
use crate::ledger::{Block, Commit, LedgerError, Params, ProofCheck};
use crate::store::{Store, StoreError};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const EXPLORER: &str = include_str!("explorer.html");
/// Largest request body: one maximal envelope in hex plus JSON framing.
const MAX_BODY: usize = 2 * 64 * 1024 + 1024;

/// A validator's Ed25519 signing key. `Debug` never prints it.
#[derive(Clone)]
pub struct ValidatorKey(pub Arc<ed25519_dalek::SigningKey>);

impl std::fmt::Debug for ValidatorKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ValidatorKey(..)")
    }
}

#[derive(Clone, Debug)]
pub enum Role {
    Validator {
        key: ValidatorKey,
        engine: EngineConfig,
    },
    Follower {
        upstream: String,
        poll: Duration,
    },
}

/// Most transactions a proposer puts in one block.
const MAX_BLOCK_TXS: usize = 500;
/// How often a validator asks its peers whether it fell behind.
const CATCH_UP_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct NodeConfig {
    pub data_dir: PathBuf,
    /// Required on first start; afterwards the store holds it.
    pub genesis: Option<Vec<u8>>,
    pub params: Params,
    pub listen: SocketAddr,
    pub role: Role,
}

struct Pending {
    txid: [u8; 32],
    bytes: Vec<u8>,
    nullifiers: Vec<[u8; 32]>,
}

struct Node {
    store: Store,
    mempool: Vec<Pending>,
    mempool_nullifiers: HashSet<[u8; 32]>,
    /// Latest height the followed node reported (followers only).
    upstream_height: Option<u64>,
    /// Inbox of the consensus engine (validators only).
    engine: Option<engine::Handle>,
}

type Shared = Arc<Mutex<Node>>;

#[derive(Clone)]
struct AppState {
    node: Shared,
    role: Role,
    http: reqwest::Client,
}

/// Opens (or creates) the store, starts the role loop and returns the API
/// router (without the standalone explorer page) and a handle for in-process
/// callers such as the desktop wallet service.
pub async fn embed(config: NodeConfig) -> Result<(Router, NodeHandle), String> {
    std::fs::create_dir_all(&config.data_dir).map_err(|e| e.to_string())?;
    let path = config.data_dir.join("v2.sqlite");
    let store = if path.exists() {
        Store::open(&path, ProofCheck::Skip).map_err(|e| format!("open store: {e:?}"))?
    } else {
        let genesis = config
            .genesis
            .as_ref()
            .ok_or("first start needs --genesis")?;
        Store::create(&path, config.params.clone(), genesis)
            .map_err(|e| format!("create store: {e:?}"))?
    };
    eprintln!(
        "ORVEXA v2 store height {} tip {}",
        store.ledger().height(),
        hex::encode(store.ledger().tip())
    );
    let node = Arc::new(Mutex::new(Node {
        store,
        mempool: Vec::new(),
        mempool_nullifiers: HashSet::new(),
        upstream_height: None,
        engine: None,
    }));
    let state = AppState {
        node: node.clone(),
        role: config.role.clone(),
        http: reqwest::Client::new(),
    };

    let task = match config.role.clone() {
        Role::Validator {
            key,
            engine: engine_config,
        } => {
            let (params, chain_id) = {
                let n = node.lock().expect("node lock");
                (
                    n.store.ledger().params().clone(),
                    n.store.ledger().chain_id(),
                )
            };
            let (inbox, events) = std::sync::mpsc::channel();
            let host = NodeHost {
                node: node.clone(),
                http: state.http.clone(),
                rt: tokio::runtime::Handle::current(),
            };
            engine::spawn(
                host,
                &params,
                chain_id,
                key.0.to_bytes(),
                engine_config.clone(),
                &config.data_dir.join("consensus.sqlite"),
                events,
            )?;
            node.lock().expect("node lock").engine = Some(inbox);
            tokio::spawn(catch_up_loop(
                node.clone(),
                state.http.clone(),
                engine_config.peers,
            ))
        }
        Role::Follower { upstream, poll } => tokio::spawn(follow_loop(
            node.clone(),
            state.http.clone(),
            upstream,
            poll,
        )),
    };

    let api = Router::new()
        .route("/v2/status", get(status))
        .route("/v2/genesis/raw", get(genesis_raw))
        .route("/v2/blocks", get(blocks))
        .route("/v2/blocks/{height}", get(block))
        .route("/v2/blocks/{height}/raw", get(block_raw))
        .route("/v2/tx", axum::routing::post(submit))
        .route("/v2/tx/{txid}", get(tx))
        .route("/v2/tx/{txid}/raw", get(tx_raw))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY))
        .merge(
            Router::new()
                .route("/v2/consensus", axum::routing::post(consensus))
                .layer(axum::extract::DefaultBodyLimit::max(
                    crate::bft::codec::MAX_BODY + 1024,
                )),
        )
        .with_state(state);
    Ok((
        api,
        NodeHandle {
            node,
            task: Arc::new(task),
            role: config.role,
            http: reqwest::Client::new(),
        },
    ))
}

pub async fn run(config: NodeConfig) -> Result<(), String> {
    let listen = config.listen;
    let (api, _handle) = embed(config).await?;
    let app = api.route("/", get(|| async { Html(EXPLORER) }));
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .map_err(|e| e.to_string())?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    println!("ORVEXA_V2_LISTEN=http://{local}");
    eprintln!("ORVEXA v2 node listening on http://{local}");
    axum::serve(listener, app).await.map_err(|e| e.to_string())
}

/// Why the mempool refused a transaction: HTTP status and a stable code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rejection {
    pub status: u16,
    pub code: String,
}

/// In-process access to a running node (same rules as the HTTP API).
#[derive(Clone)]
pub struct NodeHandle {
    node: Shared,
    task: Arc<tokio::task::JoinHandle<()>>,
    role: Role,
    http: reqwest::Client,
}

impl NodeHandle {
    pub fn chain_id(&self) -> [u8; 32] {
        self.node
            .lock()
            .expect("node lock")
            .store
            .ledger()
            .chain_id()
    }
    pub fn height(&self) -> u64 {
        self.node.lock().expect("node lock").store.ledger().height()
    }
    pub fn mempool_len(&self) -> usize {
        self.node.lock().expect("node lock").mempool.len()
    }

    /// Runs `f` against the current ledger state.
    pub fn with_ledger<R>(&self, f: impl FnOnce(&crate::ledger::Ledger) -> R) -> R {
        f(self.node.lock().expect("node lock").store.ledger())
    }

    /// Blocks `from..=tip` as decoded envelopes (genesis is height 0).
    pub fn blocks_from(&self, from: u64) -> Result<Vec<(u64, Vec<Envelope>)>, String> {
        let n = self.node.lock().expect("node lock");
        let tip = n.store.ledger().height();
        let mut out = Vec::new();
        for height in from..=tip {
            let block = block_at(&n, height)
                .map_err(|e| format!("{e:?}"))?
                .ok_or("missing block")?;
            let envs = block
                .txs
                .iter()
                .map(|t| Envelope::decode(t))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{e:?}"))?;
            out.push((height, envs));
        }
        Ok(out)
    }

    /// A confirmed or pending transaction by txid.
    pub fn tx(&self, txid: &[u8; 32]) -> Option<(Option<u64>, Envelope)> {
        let n = self.node.lock().expect("node lock");
        let (height, bytes) = find_tx(&n, txid)?;
        Envelope::decode(&bytes).ok().map(|e| (height, e))
    }

    /// `"validator"` or `"follower"`.
    pub fn role(&self) -> &'static str {
        role_name(&self.role)
    }

    /// The node a follower syncs from.
    pub fn upstream(&self) -> Option<&str> {
        match &self.role {
            Role::Follower { upstream, .. } => Some(upstream),
            Role::Validator { .. } => None,
        }
    }

    /// Height last reported by the followed node.
    pub fn upstream_height(&self) -> Option<u64> {
        self.node.lock().expect("node lock").upstream_height
    }

    pub async fn submit(&self, env: &Envelope) -> Result<[u8; 32], Rejection> {
        if let Role::Follower { upstream, .. } = &self.role {
            // Followers do not vote on blocks; a validator must see the transfer.
            let request = SubmitRequest {
                tx: hex::encode(env.encode()),
            };
            let response = self
                .http
                .post(format!("{upstream}/v2/tx"))
                .json(&request)
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(|_| Rejection {
                    status: 502,
                    code: "upstream".into(),
                })?;
            let status = response.status().as_u16();
            if !response.status().is_success() {
                let code = response
                    .json::<ErrorView>()
                    .await
                    .map(|e| e.code)
                    .unwrap_or_else(|_| "upstream".into());
                return Err(Rejection { status, code });
            }
            let body: SubmitResponse = response.json().await.map_err(|_| Rejection {
                status: 502,
                code: "upstream".into(),
            })?;
            return if body.txid == hex::encode(env.txid()) {
                Ok(env.txid())
            } else {
                Err(Rejection {
                    status: 502,
                    code: "upstream".into(),
                })
            };
        }
        let node = self.node.clone();
        let bytes = env.encode();
        let result = tokio::task::spawn_blocking(move || accept(&node, bytes))
            .await
            .unwrap_or_else(|_| {
                Err(Rejection {
                    status: 500,
                    code: "internal".into(),
                })
            })?;
        announce(&self.node, &self.http, &self.role, &result, &env.encode());
        Ok(result.0)
    }

    /// Stops the role loop (consensus or following).
    pub fn stop(&self) {
        self.task.abort();
        if let Some(engine) = self.node.lock().expect("node lock").engine.take() {
            let _ = engine.send(Event::Stop);
        }
    }
}

fn role_name(role: &Role) -> &'static str {
    match role {
        Role::Validator { .. } => "validator",
        Role::Follower { .. } => "follower",
    }
}

/// A transaction new to this validator's mempool: wake consensus and pass
/// it on to the other validators, so whoever proposes next includes it.
fn announce(
    node: &Shared,
    http: &reqwest::Client,
    role: &Role,
    accepted: &([u8; 32], bool),
    bytes: &[u8],
) {
    let (_, new) = accepted;
    let Role::Validator { engine, .. } = role else {
        return;
    };
    if !new {
        return;
    }
    if let Some(inbox) = &node.lock().expect("node lock").engine {
        let _ = inbox.send(Event::Work);
    }
    let request = SubmitRequest {
        tx: hex::encode(bytes),
    };
    for peer in &engine.peers {
        let http = http.clone();
        let url = format!("{peer}/v2/tx");
        let request = SubmitRequest {
            tx: request.tx.clone(),
        };
        tokio::spawn(async move {
            let _ = http
                .post(url)
                .json(&request)
                .timeout(Duration::from_secs(30))
                .send()
                .await;
        });
    }
}

/// The engine's view of this node.
struct NodeHost {
    node: Shared,
    http: reqwest::Client,
    rt: tokio::runtime::Handle,
}

impl Host for NodeHost {
    fn tip(&self) -> (u64, [u8; 32]) {
        let n = self.node.lock().expect("node lock");
        (n.store.ledger().height(), n.store.ledger().tip())
    }

    fn has_work(&self) -> bool {
        !self.node.lock().expect("node lock").mempool.is_empty()
    }

    /// Mempool transactions still valid on the tip, without conflicts.
    fn candidates(&self) -> Vec<Vec<u8>> {
        let n = self.node.lock().expect("node lock");
        let mut chosen = Vec::new();
        let mut seen = HashSet::new();
        for p in &n.mempool {
            if chosen.len() == MAX_BLOCK_TXS || p.nullifiers.iter().any(|nf| seen.contains(nf)) {
                continue;
            }
            if n.store
                .ledger()
                .check_candidates(std::slice::from_ref(&p.bytes), ProofCheck::Skip)
                .is_ok()
            {
                seen.extend(p.nullifiers.iter().copied());
                chosen.push(p.bytes.clone());
            }
        }
        chosen
    }

    fn check_body(&self, block: &Block) -> bool {
        let n = self.node.lock().expect("node lock");
        let l = n.store.ledger();
        block.height == l.height() + 1
            && block.prev == l.tip()
            && block.txs.len() <= MAX_BLOCK_TXS
            && l.check_candidates(&block.txs, ProofCheck::Verify).is_ok()
    }

    fn commit(&self, block: Block) -> Result<(), String> {
        let mut n = self.node.lock().expect("node lock");
        n.store.apply_block(&block).map_err(|e| format!("{e:?}"))?;
        prune_mempool(&mut n, &block);
        Ok(())
    }

    fn send(&self, peer: &str, bytes: Vec<u8>) {
        let http = self.http.clone();
        let url = format!("{peer}/v2/consensus");
        self.rt.spawn(async move {
            let _ = http
                .post(url)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .body(bytes)
                .timeout(Duration::from_secs(10))
                .send()
                .await;
        });
    }
}

/// Drops included transactions and those the new tip made invalid.
fn prune_mempool(n: &mut Node, block: &Block) {
    let included: HashSet<[u8; 32]> = block
        .txs
        .iter()
        .map(|t| crate::envelope::txid_of(t))
        .collect();
    let pending = std::mem::take(&mut n.mempool);
    n.mempool_nullifiers.clear();
    for p in pending {
        if included.contains(&p.txid) {
            continue;
        }
        let still_valid = p
            .nullifiers
            .iter()
            .all(|nf| !n.mempool_nullifiers.contains(nf))
            && n.store
                .ledger()
                .check_candidates(std::slice::from_ref(&p.bytes), ProofCheck::Skip)
                .is_ok();
        if still_valid {
            n.mempool_nullifiers.extend(p.nullifiers.iter().copied());
            n.mempool.push(p);
        } else {
            eprintln!("ORVEXA v2 dropped tx {} from mempool", hex::encode(p.txid));
        }
    }
}

/// Returns the txid and whether it is new to the mempool.
fn accept(node: &Shared, bytes: Vec<u8>) -> Result<([u8; 32], bool), Rejection> {
    let reject = |e: &LedgerError| Rejection {
        status: match e {
            LedgerError::NullifierAlreadySpent | LedgerError::DuplicateNullifier => 409,
            _ => 400,
        },
        code: format!("{e:?}"),
    };
    let mut n = node.lock().expect("node lock");
    let env = Envelope::decode(&bytes).map_err(|e| reject(&LedgerError::Envelope(e)))?;
    let txid = env.txid();
    if n.mempool.iter().any(|p| p.txid == txid) {
        return Ok((txid, false));
    }
    let nullifiers: Vec<[u8; 32]> = env.nullifiers().collect();
    if nullifiers
        .iter()
        .any(|nf| n.mempool_nullifiers.contains(nf))
    {
        return Err(Rejection {
            status: 409,
            code: "MempoolConflict".into(),
        });
    }
    n.store
        .ledger()
        .check_candidates(std::slice::from_ref(&bytes), ProofCheck::Verify)
        .map_err(|e| reject(&e))?;
    n.mempool_nullifiers.extend(nullifiers.iter().copied());
    n.mempool.push(Pending {
        txid,
        bytes,
        nullifiers,
    });
    eprintln!("ORVEXA v2 accepted tx {}", hex::encode(txid));
    Ok((txid, true))
}

// ------------------------------------------------------------ roles ----

/// Pulls every block `upstream` has above our tip, checking each one in
/// full (commit certificate and proofs). Returns how many were applied.
async fn sync_from(node: &Shared, http: &reqwest::Client, upstream: &str) -> usize {
    let Ok(status) = fetch::<StatusView>(http, &format!("{upstream}/v2/status")).await else {
        return 0;
    };
    let ours = {
        let mut n = node.lock().expect("node lock");
        n.upstream_height = Some(status.height);
        n.store.ledger().chain_id()
    };
    if status.chain_id != hex::encode(ours) {
        eprintln!("ORVEXA v2 {upstream} is a different chain; not syncing from it");
        return 0;
    }
    let mut applied = 0;
    loop {
        let next = node.lock().expect("node lock").store.ledger().height() + 1;
        if next > status.height {
            break;
        }
        let Ok(raw) = fetch::<RawView>(http, &format!("{upstream}/v2/blocks/{next}/raw")).await
        else {
            break;
        };
        let Some(block) = hex::decode(&raw.hex).ok().and_then(|b| Block::decode(&b)) else {
            eprintln!("ORVEXA v2 {upstream} sent an undecodable block {next}");
            break;
        };
        let node = node.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut n = node.lock().expect("node lock");
            let result = n.store.apply_block(&block);
            if result.is_ok() {
                prune_mempool(&mut n, &block);
            }
            result
        })
        .await;
        match result {
            Ok(Ok(id)) => {
                applied += 1;
                eprintln!("ORVEXA v2 synced block {next} {}", hex::encode(id));
            }
            Ok(Err(e)) => {
                eprintln!("ORVEXA v2 rejected block {next} from {upstream}: {e:?}");
                break;
            }
            Err(_) => break,
        }
    }
    applied
}

async fn follow_loop(node: Shared, http: reqwest::Client, upstream: String, poll: Duration) {
    let mut tick = tokio::time::interval(poll);
    loop {
        tick.tick().await;
        sync_from(&node, &http, &upstream).await;
    }
}

/// A validator that was offline or cut off takes decided blocks from its
/// peers instead of waiting for consensus messages it missed.
async fn catch_up_loop(node: Shared, http: reqwest::Client, peers: Vec<String>) {
    let mut tick = tokio::time::interval(CATCH_UP_EVERY);
    loop {
        tick.tick().await;
        for peer in &peers {
            if sync_from(&node, &http, peer).await > 0 {
                if let Some(inbox) = &node.lock().expect("node lock").engine {
                    let _ = inbox.send(Event::LedgerAdvanced);
                }
            }
        }
    }
}

async fn fetch<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &str) -> Result<T, ()> {
    let response = http
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    response.json::<T>().await.map_err(|_| ())
}

// ---------------------------------------------------------- handlers ----

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorView {
            code: code.into(),
            message: message.into(),
        }),
    )
        .into_response()
}

fn hash32(text: &str) -> Option<[u8; 32]> {
    hex::decode(text).ok()?.try_into().ok()
}

async fn status(State(s): State<AppState>) -> Json<StatusView> {
    let n = s.node.lock().expect("node lock");
    let l = n.store.ledger();
    Json(StatusView {
        network: "orvexa-v2-test".into(),
        privacy: "shielded (Ironwood v3)".into(),
        role: role_name(&s.role).into(),
        chain_id: hex::encode(l.chain_id()),
        height: l.height(),
        tip: hex::encode(l.tip()),
        burned_fees: l.burned(),
        remaining_supply: l.remaining_supply(),
        mempool: n.mempool.len(),
    })
}

fn block_at(n: &Node, height: u64) -> Result<Option<Block>, StoreError> {
    if height == 0 {
        return Ok(Some(Block {
            height: 0,
            prev: [0; 32],
            commit: Commit::default(),
            txs: vec![n.store.genesis_tx()?],
        }));
    }
    n.store.block(height)
}

fn block_view(n: &Node, block: &Block) -> BlockView {
    let l = n.store.ledger();
    let envs: Vec<Envelope> = block
        .txs
        .iter()
        .filter_map(|t| Envelope::decode(t).ok())
        .collect();
    BlockView {
        height: block.height,
        id: hex::encode(crate::ledger::block_id(&l.chain_id(), block)),
        prev: hex::encode(block.prev),
        confirmations: l.height() - block.height + 1,
        txids: block
            .txs
            .iter()
            .map(|t| hex::encode(crate::envelope::txid_of(t)))
            .collect(),
        actions: envs.iter().map(|e| e.bundle.actions().len() as u64).sum(),
        fees: envs.iter().map(|e| e.fee).sum(),
    }
}

async fn genesis_raw(State(s): State<AppState>) -> Response {
    let n = s.node.lock().expect("node lock");
    match n.store.genesis_tx() {
        Ok(g) => Json(RawView {
            hex: hex::encode(g),
        })
        .into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "store",
            "genesis unavailable",
        ),
    }
}

#[derive(Deserialize)]
struct Range {
    from: Option<u64>,
    limit: Option<u64>,
}

/// Newest first, at most 50.
async fn blocks(State(s): State<AppState>, Query(q): Query<Range>) -> Response {
    let n = s.node.lock().expect("node lock");
    let tip = n.store.ledger().height();
    let from = q.from.unwrap_or(tip).min(tip);
    let limit = q.limit.unwrap_or(20).clamp(1, 50);
    let mut out = Vec::new();
    for height in (from.saturating_sub(limit - 1)..=from).rev() {
        if let Ok(Some(b)) = block_at(&n, height) {
            out.push(block_view(&n, &b));
        }
    }
    Json(out).into_response()
}

async fn block(State(s): State<AppState>, Path(height): Path<u64>) -> Response {
    let n = s.node.lock().expect("node lock");
    match block_at(&n, height) {
        Ok(Some(b)) => Json(block_view(&n, &b)).into_response(),
        _ => error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no block at this height",
        ),
    }
}

async fn block_raw(State(s): State<AppState>, Path(height): Path<u64>) -> Response {
    let n = s.node.lock().expect("node lock");
    match block_at(&n, height) {
        Ok(Some(b)) => Json(RawView {
            hex: hex::encode(b.encode()),
        })
        .into_response(),
        _ => error(
            StatusCode::NOT_FOUND,
            "not_found",
            "no block at this height",
        ),
    }
}

fn find_tx(n: &Node, txid: &[u8; 32]) -> Option<(Option<u64>, Vec<u8>)> {
    if let Some(height) = n.store.ledger().tx_height(txid) {
        let block = block_at(n, height).ok()??;
        let bytes = block
            .txs
            .into_iter()
            .find(|t| crate::envelope::txid_of(t) == *txid)?;
        return Some((Some(height), bytes));
    }
    n.mempool
        .iter()
        .find(|p| p.txid == *txid)
        .map(|p| (None, p.bytes.clone()))
}

async fn tx(State(s): State<AppState>, Path(txid): Path<String>) -> Response {
    let Some(id) = hash32(&txid) else {
        return error(StatusCode::BAD_REQUEST, "bad_txid", "txid is 32 bytes hex");
    };
    let n = s.node.lock().expect("node lock");
    let Some((height, bytes)) = find_tx(&n, &id) else {
        return error(StatusCode::NOT_FOUND, "not_found", "unknown transaction");
    };
    let Ok(env) = Envelope::decode(&bytes) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "store", "stored tx");
    };
    let tip = n.store.ledger().height();
    Json(TxView {
        txid: hex::encode(id),
        height,
        confirmations: height.map_or(0, |h| tip - h + 1),
        fee: env.fee,
        expiry_height: env.expiry_height,
        actions: env.bundle.actions().len(),
        bytes: bytes.len(),
        nullifiers: env.nullifiers().map(hex::encode).collect(),
        commitments: env
            .bundle
            .actions()
            .iter()
            .map(|a| hex::encode(a.cmx().to_bytes()))
            .collect(),
    })
    .into_response()
}

async fn tx_raw(State(s): State<AppState>, Path(txid): Path<String>) -> Response {
    let Some(id) = hash32(&txid) else {
        return error(StatusCode::BAD_REQUEST, "bad_txid", "txid is 32 bytes hex");
    };
    let n = s.node.lock().expect("node lock");
    match find_tx(&n, &id) {
        Some((_, bytes)) => Json(RawView {
            hex: hex::encode(bytes),
        })
        .into_response(),
        None => error(StatusCode::NOT_FOUND, "not_found", "unknown transaction"),
    }
}

async fn submit(State(s): State<AppState>, Json(req): Json<SubmitRequest>) -> Response {
    if let Role::Follower { upstream, .. } = &s.role {
        // Followers do not vote on blocks; hand the transaction to a validator.
        let forwarded = s
            .http
            .post(format!("{upstream}/v2/tx"))
            .json(&req)
            .timeout(Duration::from_secs(30))
            .send()
            .await;
        return match forwarded {
            Ok(r) => {
                let status =
                    StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
                let body = r.bytes().await.unwrap_or_default();
                (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
            }
            Err(_) => error(StatusCode::BAD_GATEWAY, "upstream", "node unreachable"),
        };
    }
    let Ok(bytes) = hex::decode(req.tx.trim()) else {
        return error(StatusCode::BAD_REQUEST, "bad_hex", "tx is hex");
    };
    let node = s.node.clone();
    let raw = bytes.clone();
    match tokio::task::spawn_blocking(move || accept(&node, bytes)).await {
        Ok(Ok(accepted)) => {
            announce(&s.node, &s.http, &s.role, &accepted, &raw);
            let txid = accepted.0;
            (
                StatusCode::ACCEPTED,
                Json(SubmitResponse {
                    txid: hex::encode(txid),
                }),
            )
                .into_response()
        }
        Ok(Err(r)) => error(
            StatusCode::from_u16(r.status).unwrap_or(StatusCode::BAD_REQUEST),
            &r.code,
            if r.code == "MempoolConflict" {
                "a pending transaction already spends this note"
            } else {
                "transaction rejected by v2 ledger rules"
            },
        ),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "validation task failed",
        ),
    }
}

/// Consensus messages from other validators (binary, see `bft::codec`).
async fn consensus(State(s): State<AppState>, body: axum::body::Bytes) -> Response {
    let Some(msg) = NetMsg::decode(&body) else {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_message",
            "not a v2 consensus message",
        );
    };
    let n = s.node.lock().expect("node lock");
    match &n.engine {
        Some(inbox) if inbox.send(Event::Net(Box::new(msg))).is_ok() => {
            StatusCode::ACCEPTED.into_response()
        }
        _ => error(
            StatusCode::NOT_FOUND,
            "not_validator",
            "this node does not take part in consensus",
        ),
    }
}
