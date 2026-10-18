//! Default-loopback node with opt-in private VPN peers and a replay-verified journal.
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use orvexa_core::{Block, Genesis, Ledger, MAX_BLOCK_TXS, SignedTransaction, VERSION};
use orvexa_crypto::{Address, Hash32};
use orvexa_rpc::{
    Accepted, ApiError, BLOCK_PAGE, Balance, BlockDetails, HistoryEntry, MAX_BODY, Status,
    TransactionDetails, UiState,
};
use rusqlite::{Connection, params};
use serde::Deserialize;
use std::{
    fs::OpenOptions,
    path::Path as FilePath,
    sync::{Arc, Mutex},
    time::Duration,
};

pub const MAX_MEMPOOL: usize = 512;
pub type SharedNode = Arc<Mutex<Node>>;

pub mod desktop;
pub mod mesh;
pub mod p2p;
pub mod web;

pub struct Node {
    connection: Connection,
    pub ledger: Ledger,
    mempool: Vec<SignedTransaction>,
    pub runtime: orvexa_rpc::RuntimeStatus,
}

fn database(path: &FilePath) -> Result<Connection> {
    let connection =
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    connection.busy_timeout(Duration::from_millis(100))?;
    // Exclusive locking mode retains the database lock after a committed transaction.
    connection.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF; BEGIN EXCLUSIVE; COMMIT;")
        .context("lock node database (another node may be using it)")?;
    Ok(connection)
}

impl Node {
    pub fn create(path: &FilePath, genesis: Genesis) -> Result<Self> {
        let ledger = Ledger::new(genesis.clone())?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .context("create database (refusing to overwrite existing file)")?;
        let mut connection = database(path)?;
        let transaction = connection.transaction()?;
        transaction.execute_batch("CREATE TABLE genesis (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL); CREATE TABLE blocks (height INTEGER PRIMARY KEY, json TEXT NOT NULL); PRAGMA user_version=1;")?;
        transaction.execute(
            "INSERT INTO genesis(id,json) VALUES(1,?1)",
            [serde_json::to_string(&genesis)?],
        )?;
        transaction.commit()?;
        Ok(Self {
            connection,
            ledger,
            mempool: Vec::new(),
            runtime: Default::default(),
        })
    }
    pub fn open(path: &FilePath) -> Result<Self> {
        let connection = database(path)?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        ensure!(version == 1, "unsupported node database schema");
        let json: String =
            connection.query_row("SELECT json FROM genesis WHERE id=1", [], |row| row.get(0))?;
        let mut ledger = Ledger::new(serde_json::from_str(&json)?)?;
        {
            let mut statement =
                connection.prepare("SELECT height,json FROM blocks ORDER BY height")?;
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                let height: u64 = row.get(0)?;
                let json: String = row.get(1)?;
                ensure!(json.len() <= MAX_BODY, "stored block exceeds size limit");
                let block: Block =
                    serde_json::from_str(&json).context("corrupt stored block JSON")?;
                ensure!(block.header.height == height, "corrupt stored block height");
                ledger
                    .apply_block(block)
                    .with_context(|| format!("stored block {height} failed validation"))?;
            }
        }
        Ok(Self {
            connection,
            ledger,
            mempool: Vec::new(),
            runtime: Default::default(),
        })
    }
    pub fn shared(self) -> SharedNode {
        Arc::new(Mutex::new(self))
    }
    pub fn mempool(&self) -> &[SignedTransaction] {
        &self.mempool
    }
    pub fn status(&self) -> Status {
        Status {
            protocol: VERSION,
            network: if self.runtime.transport == "libp2p-tcp-noise" {
                "ORVEXA native P2P testnet — test atoms only"
            } else if self.runtime.enabled {
                "ORVEXA friends testnet — test atoms only"
            } else {
                "ORVEXA local devnet — test atoms only"
            }
            .into(),
            privacy: "TRANSPARENT: amounts, addresses and sender links are public".into(),
            consensus: "EXPERIMENTAL signed single sequencer; no Byzantine finality".into(),
            chain_id: self.ledger.genesis.chain_id(),
            height: self.ledger.height(),
            tip: self.ledger.tip(),
            burned_fees: self.ledger.burned(),
            pending: self.mempool.len(),
        }
    }
    pub fn balance(&self, address: Address) -> Result<Balance> {
        let projected = self.ledger.project(&self.mempool)?;
        Ok(Balance {
            address,
            confirmed: self.ledger.account(address),
            pending_nonce: projected.account(address).nonce,
        })
    }
    pub fn submit(&mut self, tx: SignedTransaction) -> Result<Hash32> {
        ensure!(
            !self.runtime.halted,
            "network quarantined: inspect authority conflict evidence"
        );
        ensure!(self.mempool.len() < MAX_MEMPOOL, "pending pool is full");
        let id = tx.id()?;
        let mut pending = self.mempool.clone();
        pending.push(tx.clone());
        self.ledger.project(&pending)?;
        self.mempool.push(tx);
        Ok(id)
    }
    pub fn template(&self) -> Result<Block> {
        ensure!(
            !self.runtime.halted,
            "network quarantined: inspect authority conflict evidence"
        );
        Ok(self
            .ledger
            .template(self.mempool.iter().take(MAX_BLOCK_TXS).cloned().collect())?)
    }
    pub fn commit(&mut self, block: Block) -> Result<Hash32> {
        ensure!(
            !self.runtime.halted,
            "network quarantined: inspect authority conflict evidence before continuing"
        );
        let mut staged = self.ledger.clone();
        staged.apply_block(block.clone())?;
        let id = block.id()?;
        let json = serde_json::to_string(&block)?;
        ensure!(json.len() <= MAX_BODY, "block exceeds size limit");
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO blocks(height,json) VALUES(?1,?2)",
            params![block.header.height, json],
        )?;
        transaction
            .commit()
            .context("persist block before updating in-memory state")?;
        self.ledger = staged;
        let mut projection = self.ledger.clone();
        self.mempool.retain(|tx| {
            if let Ok(next) = projection.project(std::slice::from_ref(tx)) {
                projection = next;
                true
            } else {
                false
            }
        });
        Ok(id)
    }
}

struct ApiFailure(anyhow::Error);
impl<E: Into<anyhow::Error>> From<E> for ApiFailure {
    fn from(error: E) -> Self {
        Self(error.into())
    }
}
impl IntoResponse for ApiFailure {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: self.0.to_string(),
            }),
        )
            .into_response()
    }
}
fn locked(node: &SharedNode) -> Result<std::sync::MutexGuard<'_, Node>> {
    node.lock()
        .map_err(|_| anyhow::anyhow!("node state lock poisoned"))
}

// Browser origins are unnecessary for the CLI API. Reject them to reduce local
// cross-origin submission and do not add permissive CORS headers.
async fn local_api(headers: HeaderMap, request: axum::extract::Request, next: Next) -> Response {
    if headers.contains_key("origin") {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                error: "browser-origin requests are disabled".into(),
            }),
        )
            .into_response();
    }
    next.run(request).await
}

pub fn router(node: SharedNode) -> Router {
    api_router(node).layer(middleware::from_fn(local_api))
}

/// Shared route definitions; the optional browser host supplies its own strict
/// same-origin/Host guard. The default CLI API continues rejecting all Origins.
pub(crate) fn api_router(node: SharedNode) -> Router {
    Router::new()
        .route("/v1/status", get(status))
        .route("/v1/runtime", get(runtime_status))
        .route("/v1/genesis", get(genesis))
        .route("/v1/balance/{address}", get(balance))
        .route("/v1/history/{address}", get(history))
        .route("/v1/mempool", get(mempool))
        .route("/v1/transactions", post(submit))
        .route("/v1/transactions/{id}", get(transaction_details))
        .route("/v1/block-template", get(template))
        .route("/v1/blocks", get(blocks).post(commit))
        .route("/v1/blocks/{height}", get(block_details))
        .route("/v1/explorer/blocks", get(recent_blocks))
        .route("/v1/explorer/mempool", get(pending_details))
        .route("/v1/ui-state", get(ui_state))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .with_state(node)
}

async fn runtime_status(
    State(node): State<SharedNode>,
) -> Result<Json<orvexa_rpc::RuntimeStatus>, ApiFailure> {
    Ok(Json(locked(&node)?.runtime.clone()))
}

async fn pending_details(
    State(node): State<SharedNode>,
) -> Result<Json<Vec<TransactionDetails>>, ApiFailure> {
    let node = locked(&node)?;
    Ok(Json(describe_pending(&node)?))
}

fn describe_pending(node: &Node) -> Result<Vec<TransactionDetails>> {
    node.mempool
        .iter()
        .map(|tx| {
            Ok(TransactionDetails {
                id: tx.id()?,
                status: "pending".into(),
                block_height: None,
                block_id: None,
                confirmations: 0,
                transaction: tx.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()
}

#[derive(Deserialize)]
struct UiQuery {
    address: Option<Address>,
}

/// One lock gives the UI a coherent balance/history/mempool/tip observation.
async fn ui_state(
    State(node): State<SharedNode>,
    Query(query): Query<UiQuery>,
) -> Result<Json<UiState>, ApiFailure> {
    let node = locked(&node)?;
    Ok(Json(UiState {
        status: node.status(),
        genesis: node.ledger.genesis.clone(),
        blocks: node
            .ledger
            .blocks()
            .iter()
            .rev()
            .take(BLOCK_PAGE)
            .map(|b| describe_block(&node, b))
            .collect::<Result<Vec<_>>>()?,
        pending: describe_pending(&node)?,
        balance: query.address.map(|a| node.balance(a)).transpose()?,
        history: query
            .address
            .map(|a| address_history(&node, a))
            .transpose()?
            .unwrap_or_default(),
    }))
}

async fn transaction_details(
    State(node): State<SharedNode>,
    Path(id): Path<Hash32>,
) -> Result<Response, ApiFailure> {
    let node = locked(&node)?;
    for block in node.ledger.blocks().iter().rev() {
        for tx in &block.transactions {
            if tx.id()? == id {
                return Ok(Json(TransactionDetails {
                    id,
                    status: "confirmed".into(),
                    block_height: Some(block.header.height),
                    block_id: Some(block.id()?),
                    confirmations: node.ledger.height() - block.header.height + 1,
                    transaction: tx.clone(),
                })
                .into_response());
            }
        }
    }
    for tx in &node.mempool {
        if tx.id()? == id {
            return Ok(Json(TransactionDetails {
                id,
                status: "pending".into(),
                block_height: None,
                block_id: None,
                confirmations: 0,
                transaction: tx.clone(),
            })
            .into_response());
        }
    }
    Ok((
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "transaction not found in the accepted chain or current mempool".into(),
        }),
    )
        .into_response())
}

async fn block_details(
    State(node): State<SharedNode>,
    Path(height): Path<u64>,
) -> Result<Response, ApiFailure> {
    let node = locked(&node)?;
    if let Some(block) = node
        .ledger
        .blocks()
        .iter()
        .find(|b| b.header.height == height)
    {
        return Ok(Json(describe_block(&node, block)?).into_response());
    }
    Ok((
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "block not found (genesis is available at /v1/genesis)".into(),
        }),
    )
        .into_response())
}

fn describe_block(node: &Node, block: &Block) -> Result<BlockDetails> {
    Ok(BlockDetails {
        id: block.id()?,
        transaction_ids: block
            .transactions
            .iter()
            .map(|t| t.id())
            .collect::<std::result::Result<Vec<_>, _>>()?,
        confirmations: node.ledger.height() - block.header.height + 1,
        block: block.clone(),
    })
}

#[derive(Deserialize)]
struct RecentQuery {
    before: Option<u64>,
}

async fn recent_blocks(
    State(node): State<SharedNode>,
    Query(query): Query<RecentQuery>,
) -> Result<Json<Vec<BlockDetails>>, ApiFailure> {
    let node = locked(&node)?;
    let entries = node
        .ledger
        .blocks()
        .iter()
        .rev()
        .filter(|b| query.before.is_none_or(|h| b.header.height < h))
        .take(BLOCK_PAGE)
        .map(|b| describe_block(&node, b))
        .collect::<Result<Vec<_>>>()?;
    Ok(Json(entries))
}
async fn status(State(node): State<SharedNode>) -> Result<Json<Status>, ApiFailure> {
    Ok(Json(locked(&node)?.status()))
}
async fn genesis(State(node): State<SharedNode>) -> Result<Json<Genesis>, ApiFailure> {
    Ok(Json(locked(&node)?.ledger.genesis.clone()))
}
async fn balance(
    State(node): State<SharedNode>,
    Path(address): Path<Address>,
) -> Result<Json<Balance>, ApiFailure> {
    Ok(Json(locked(&node)?.balance(address)?))
}
async fn mempool(
    State(node): State<SharedNode>,
) -> Result<Json<Vec<SignedTransaction>>, ApiFailure> {
    Ok(Json(locked(&node)?.mempool.clone()))
}
async fn submit(
    State(node): State<SharedNode>,
    Json(tx): Json<SignedTransaction>,
) -> Result<Json<Accepted>, ApiFailure> {
    Ok(Json(Accepted {
        id: locked(&node)?.submit(tx)?,
    }))
}
async fn template(State(node): State<SharedNode>) -> Result<Json<Block>, ApiFailure> {
    Ok(Json(locked(&node)?.template()?))
}
async fn commit(
    State(node): State<SharedNode>,
    Json(block): Json<Block>,
) -> Result<Json<Accepted>, ApiFailure> {
    Ok(Json(Accepted {
        id: locked(&node)?.commit(block)?,
    }))
}
#[derive(Deserialize)]
struct BlockQuery {
    from: Option<u64>,
}
async fn blocks(
    State(node): State<SharedNode>,
    Query(query): Query<BlockQuery>,
) -> Result<Json<Vec<Block>>, ApiFailure> {
    let node = locked(&node)?;
    let from = query.from.unwrap_or(1).max(1);
    Ok(Json(
        node.ledger
            .blocks()
            .iter()
            .filter(|b| b.header.height >= from)
            .take(BLOCK_PAGE)
            .cloned()
            .collect(),
    ))
}
async fn history(
    State(node): State<SharedNode>,
    Path(address): Path<Address>,
) -> Result<Json<Vec<HistoryEntry>>, ApiFailure> {
    let node = locked(&node)?;
    Ok(Json(address_history(&node, address)?))
}

fn address_history(node: &Node, address: Address) -> Result<Vec<HistoryEntry>> {
    let mut entries = Vec::new();
    for block in node.ledger.blocks().iter().rev() {
        for tx in block.transactions.iter().rev() {
            if tx.transaction.sender == address || tx.transaction.recipient == address {
                entries.push(HistoryEntry {
                    height: block.header.height,
                    id: tx.id()?,
                    transaction: tx.clone(),
                });
                if entries.len() == 256 {
                    return Ok(entries);
                }
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvexa_crypto::SecretKey;
    #[test]
    fn storage_failure_does_not_advance_memory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.sqlite");
        let key = SecretKey::from_bytes(&[7; 32]);
        let genesis = Genesis::new(key.address(), key.address(), Hash32([6; 32]));
        let mut node = Node::create(&path, genesis).unwrap();
        let tip = node.ledger.tip();
        node.connection.execute_batch("CREATE TRIGGER fail_write BEFORE INSERT ON blocks BEGIN SELECT RAISE(ABORT, 'simulated disk write failure'); END;").unwrap();
        let block = node.template().unwrap().sign(&key);
        assert!(node.commit(block.clone()).is_err());
        assert_eq!(node.ledger.height(), 0);
        assert_eq!(node.ledger.tip(), tip);
        node.connection
            .execute_batch("DROP TRIGGER fail_write")
            .unwrap();
        node.commit(block).unwrap();
        assert_eq!(node.ledger.height(), 1);
    }
}
