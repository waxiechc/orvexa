//! Typed HTTP/JSON API for local wallets and explicitly opted-in VPN peers.
use anyhow::{Context, Result, bail, ensure};
use futures_util::StreamExt;
use orvexa_core::{Account, Block, Genesis, SignedTransaction};
use orvexa_crypto::{Address, Hash32};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{net::IpAddr, time::Duration};

pub const MAX_BODY: usize = 2 * 1024 * 1024;
pub const BLOCK_PAGE: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub protocol: u32,
    pub network: String,
    pub privacy: String,
    pub consensus: String,
    pub chain_id: Hash32,
    pub height: u64,
    pub tip: Hash32,
    pub burned_fees: u64,
    pub pending: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Balance {
    pub address: Address,
    pub confirmed: Account,
    pub pending_nonce: u64,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Accepted {
    pub id: Hash32,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub height: u64,
    pub id: Hash32,
    pub transaction: SignedTransaction,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct BlockDetails {
    pub id: Hash32,
    pub transaction_ids: Vec<Hash32>,
    pub confirmations: u64,
    pub block: Block,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionDetails {
    pub id: Hash32,
    pub status: String,
    pub block_height: Option<u64>,
    pub block_id: Option<Hash32>,
    pub confirmations: u64,
    pub transaction: SignedTransaction,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct UiState {
    pub status: Status,
    pub genesis: Genesis,
    pub blocks: Vec<BlockDetails>,
    pub pending: Vec<TransactionDetails>,
    pub balance: Option<Balance>,
    pub history: Vec<HistoryEntry>,
}
#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SyncReport {
    pub blocks: usize,
    pub transactions: usize,
}

/// Operational observations, never consensus or a claim of multiple authorities.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RuntimeStatus {
    pub enabled: bool,
    #[serde(default)]
    pub transport: String,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub listen_addresses: Vec<String>,
    pub role: String,
    pub halted: bool,
    pub peers: Vec<PeerStatus>,
    pub produced_blocks: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerStatus {
    pub url: String,
    pub state: String,
    pub height: Option<u64>,
}

/// Tailscale's documented address ranges, plus loopback for process tests.
/// Address range alone does NOT provide encryption: an active VPN is required.
pub fn mesh_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(a) => {
            a.is_loopback() || (a.octets()[0] == 100 && (64..=127).contains(&a.octets()[1]))
        }
        IpAddr::V6(a) => a.is_loopback() || a.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
}
impl Client {
    /// Explicit opt-in for authenticated, read-only peer RPC over an encrypted
    /// private overlay. Never enables arbitrary Internet URLs or DNS resolution.
    pub fn new_mesh(url: &str, token: &str) -> Result<Self> {
        let parsed = reqwest::Url::parse(url).context("invalid mesh URL")?;
        ensure!(
            parsed.scheme() == "http"
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.path() == "/",
            "mesh URL must be a plain numeric HTTP origin"
        );
        let address: IpAddr = parsed
            .host_str()
            .context("missing mesh host")?
            .trim_matches(['[', ']'])
            .parse()
            .context("mesh host must be numeric")?;
        ensure!(
            mesh_address(address),
            "mesh peer must be a Tailscale or loopback address"
        );
        ensure!(
            token.len() == 64
                && token
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "invalid mesh token format"
        );
        let mut authorization = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?;
        authorization.set_sensitive(true);
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(reqwest::header::AUTHORIZATION, authorization);
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(3))
            .default_headers(headers)
            .build()?;
        Ok(Self {
            http,
            base: parsed.as_str().trim_end_matches('/').into(),
        })
    }
    pub fn new(url: &str) -> Result<Self> {
        let parsed = reqwest::Url::parse(url).context("invalid node URL")?;
        ensure!(
            parsed.scheme() == "http"
                && parsed.username().is_empty()
                && parsed.password().is_none()
                && parsed.query().is_none()
                && parsed.fragment().is_none()
                && parsed.path() == "/",
            "use a plain numeric loopback HTTP origin such as http://127.0.0.1:4242"
        );
        let host = parsed
            .host_str()
            .context("missing host")?
            .trim_matches(['[', ']']);
        let address: IpAddr = host
            .parse()
            .context("node host must be a numeric loopback IP")?;
        ensure!(address.is_loopback(), "only loopback nodes are supported");
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()?;
        Ok(Self {
            http,
            base: url.trim_end_matches('/').into(),
        })
    }
    async fn response<T: DeserializeOwned>(&self, request: reqwest::RequestBuilder) -> Result<T> {
        let response = request.send().await.context("node request failed")?;
        let status = response.status();
        ensure!(
            response.content_length().unwrap_or(0) <= MAX_BODY as u64,
            "node response exceeds size limit"
        );
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            ensure!(
                bytes.len() + chunk.len() <= MAX_BODY,
                "node response exceeds size limit"
            );
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            if let Ok(error) = serde_json::from_slice::<ApiError>(&bytes) {
                bail!("node rejected request ({status}): {}", error.error);
            }
            bail!("node returned HTTP {status}");
        }
        serde_json::from_slice(&bytes).context("invalid node JSON response")
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.response(self.http.get(format!("{}{path}", self.base)))
            .await
    }
    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T> {
        self.response(self.http.post(format!("{}{path}", self.base)).json(body))
            .await
    }
    pub async fn status(&self) -> Result<Status> {
        self.get("/v1/status").await
    }
    pub async fn genesis(&self) -> Result<Genesis> {
        self.get("/v1/genesis").await
    }
    pub async fn balance(&self, address: Address) -> Result<Balance> {
        self.get(&format!("/v1/balance/{address}")).await
    }
    pub async fn submit(&self, tx: &SignedTransaction) -> Result<Accepted> {
        self.post("/v1/transactions", tx).await
    }
    pub async fn template(&self) -> Result<Block> {
        self.get("/v1/block-template").await
    }
    pub async fn commit(&self, block: &Block) -> Result<Accepted> {
        self.post("/v1/blocks", block).await
    }
    pub async fn blocks(&self, from: u64) -> Result<Vec<Block>> {
        self.get(&format!("/v1/blocks?from={from}")).await
    }
    pub async fn mempool(&self) -> Result<Vec<SignedTransaction>> {
        self.get("/v1/mempool").await
    }
    pub async fn history(&self, address: Address) -> Result<Vec<HistoryEntry>> {
        self.get(&format!("/v1/history/{address}")).await
    }
}

/// Explicit pull replication. Local node validates every submitted item.
/// A conflicting suffix fails without replacing any accepted history.
pub async fn sync(local: &Client, peer: &Client) -> Result<SyncReport> {
    let ours = local.status().await?;
    let theirs = peer.status().await?;
    ensure!(
        ours.chain_id == theirs.chain_id,
        "peer has a different genesis / network"
    );
    ensure!(
        ours.height <= theirs.height,
        "peer is behind local node; sync in the opposite direction"
    );
    let mut report = SyncReport::default();
    let mut height = ours.height;
    while height < theirs.height {
        let page = peer
            .blocks(height.checked_add(1).context("height overflow")?)
            .await?;
        ensure!(
            !page.is_empty() && page.len() <= BLOCK_PAGE,
            "peer returned an invalid block page"
        );
        let remaining = (theirs.height - height).min(BLOCK_PAGE as u64) as usize;
        for block in page.into_iter().take(remaining) {
            ensure!(
                block.header.height == height + 1,
                "peer block sequence is not contiguous"
            );
            local.commit(&block).await?;
            height += 1;
            report.blocks += 1;
        }
    }
    let now = local.status().await?;
    ensure!(
        now.tip == theirs.tip,
        "peer history conflicts with local history or changed during sync"
    );
    let existing = local.mempool().await?;
    for tx in peer.mempool().await? {
        if existing.iter().any(|pending| pending == &tx) {
            continue;
        }
        local.submit(&tx).await?;
        report.transactions += 1;
    }
    Ok(report)
}
