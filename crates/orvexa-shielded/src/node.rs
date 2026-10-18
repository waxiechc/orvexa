//! Local v2 node: durable state ([`Store`]), mempool, a JSON API on
//! localhost and one of two roles.
//!
//! * Producer: like the v1 authority, it seals the mempool into a block at a
//!   fixed interval. Every block still passes the full ledger rules.
//! * Follower: pulls blocks from a producer over HTTP and re-verifies each
//!   one (proofs included) before storing it; it forwards submitted
//!   transactions to the producer.
//!
//! Logs carry heights, txids and error codes only.

use crate::api::{
    BlockView, ErrorView, RawView, StatusView, SubmitRequest, SubmitResponse, TxView,
};
use crate::envelope::Envelope;
use crate::ledger::{Block, LedgerError, Params, ProofCheck};
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

#[derive(Clone, Debug)]
pub enum Role {
    Producer { every: Duration },
    Follower { upstream: String, poll: Duration },
}

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
    }));
    let state = AppState {
        node: node.clone(),
        role: config.role.clone(),
        http: reqwest::Client::new(),
    };

    let task = match config.role.clone() {
        Role::Producer { every } => tokio::spawn(produce_loop(node.clone(), every)),
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
        .with_state(state);
    Ok((
        api,
        NodeHandle {
            node,
            task: Arc::new(task),
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

    /// Validates and queues a transaction for the next block.
    pub async fn submit(&self, env: &Envelope) -> Result<[u8; 32], Rejection> {
        let node = self.node.clone();
        let bytes = env.encode();
        tokio::task::spawn_blocking(move || accept(&node, bytes))
            .await
            .unwrap_or_else(|_| {
                Err(Rejection {
                    status: 500,
                    code: "internal".into(),
                })
            })
    }

    /// Stops the role loop (block production or following).
    pub fn stop(&self) {
        self.task.abort();
    }
}

fn accept(node: &Shared, bytes: Vec<u8>) -> Result<[u8; 32], Rejection> {
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
        return Ok(txid);
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
    let probe = Block {
        height: n.store.ledger().height() + 1,
        prev: n.store.ledger().tip(),
        txs: vec![bytes.clone()],
    };
    n.store
        .ledger()
        .validate_block(&probe, ProofCheck::Verify)
        .map_err(|e| reject(&e))?;
    n.mempool_nullifiers.extend(nullifiers.iter().copied());
    n.mempool.push(Pending {
        txid,
        bytes,
        nullifiers,
    });
    eprintln!("ORVEXA v2 accepted tx {}", hex::encode(txid));
    Ok(txid)
}

// ------------------------------------------------------------ roles ----

async fn produce_loop(node: Shared, every: Duration) {
    let mut tick = tokio::time::interval(every);
    loop {
        tick.tick().await;
        let node = node.clone();
        let _ = tokio::task::spawn_blocking(move || produce_once(&node)).await;
    }
}

fn produce_once(node: &Shared) {
    let mut n = node.lock().expect("node lock");
    if n.mempool.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut n.mempool);
    n.mempool_nullifiers.clear();
    let height = n.store.ledger().height() + 1;
    let prev = n.store.ledger().tip();
    // Keep only transactions that are still valid on top of the tip (expiry,
    // anchors) and do not conflict with one another.
    let mut chosen: Vec<Pending> = Vec::new();
    let mut seen = HashSet::new();
    for p in pending {
        if p.nullifiers.iter().any(|nf| seen.contains(nf)) {
            continue;
        }
        let probe = Block {
            height,
            prev,
            txs: vec![p.bytes.clone()],
        };
        if n.store
            .ledger()
            .validate_block(&probe, ProofCheck::Skip)
            .is_ok()
        {
            seen.extend(p.nullifiers.iter().copied());
            chosen.push(p);
        } else {
            eprintln!("ORVEXA v2 dropped tx {} from mempool", hex::encode(p.txid));
        }
    }
    if chosen.is_empty() {
        return;
    }
    let block = Block {
        height,
        prev,
        txs: chosen.iter().map(|p| p.bytes.clone()).collect(),
    };
    match n.store.apply_block(&block) {
        Ok(id) => eprintln!(
            "ORVEXA v2 produced block {height} {} with {} tx",
            hex::encode(id),
            block.txs.len()
        ),
        Err(e) => eprintln!("ORVEXA v2 block {height} rejected: {e:?}"),
    }
}

async fn follow_loop(node: Shared, http: reqwest::Client, upstream: String, poll: Duration) {
    let mut tick = tokio::time::interval(poll);
    loop {
        tick.tick().await;
        let Ok(status) = fetch::<StatusView>(&http, &format!("{upstream}/v2/status")).await else {
            continue;
        };
        let ours = node.lock().expect("node lock").store.ledger().chain_id();
        if status.chain_id != hex::encode(ours) {
            eprintln!("ORVEXA v2 upstream is a different chain; not following");
            continue;
        }
        loop {
            let next = node.lock().expect("node lock").store.ledger().height() + 1;
            if next > status.height {
                break;
            }
            let Ok(raw) =
                fetch::<RawView>(&http, &format!("{upstream}/v2/blocks/{next}/raw")).await
            else {
                break;
            };
            let Some(block) = hex::decode(&raw.hex).ok().and_then(|b| Block::decode(&b)) else {
                eprintln!("ORVEXA v2 upstream sent an undecodable block {next}");
                break;
            };
            let node = node.clone();
            let applied = tokio::task::spawn_blocking(move || {
                let mut n = node.lock().expect("node lock");
                let result = n.store.apply_block(&block);
                if result.is_ok() {
                    let included: HashSet<[u8; 32]> = block
                        .txs
                        .iter()
                        .map(|t| crate::envelope::txid_of(t))
                        .collect();
                    n.mempool.retain(|p| !included.contains(&p.txid));
                }
                result
            })
            .await;
            match applied {
                Ok(Ok(id)) => eprintln!("ORVEXA v2 followed block {next} {}", hex::encode(id)),
                Ok(Err(e)) => {
                    eprintln!("ORVEXA v2 rejected upstream block {next}: {e:?}");
                    break;
                }
                Err(_) => break,
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
        role: match s.role {
            Role::Producer { .. } => "producer".into(),
            Role::Follower { .. } => "follower".into(),
        },
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
        // Followers do not seal blocks; hand the transaction to the producer.
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
            Err(_) => error(StatusCode::BAD_GATEWAY, "upstream", "producer unreachable"),
        };
    }
    let Ok(bytes) = hex::decode(req.tx.trim()) else {
        return error(StatusCode::BAD_REQUEST, "bad_hex", "tx is hex");
    };
    let node = s.node.clone();
    match tokio::task::spawn_blocking(move || accept(&node, bytes)).await {
        Ok(Ok(txid)) => (
            StatusCode::ACCEPTED,
            Json(SubmitResponse {
                txid: hex::encode(txid),
            }),
        )
            .into_response(),
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
