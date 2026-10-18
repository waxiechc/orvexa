//! Opt-in friends testnet. Transport encryption belongs to the private VPN;
//! signatures/accounting remain the exact v1 rules. One authority, no failover.
use crate::{MAX_MEMPOOL, SharedNode, locked};
use anyhow::{Context, Result, ensure};
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use orvexa_core::{Block, VERSION, transaction_commitment};
use orvexa_crypto::{Hash32, SecretKey, random_bytes, verify};
use orvexa_rpc::{BLOCK_PAGE, Client, PeerStatus, RuntimeStatus};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const MAX_PEERS: usize = 8;

pub fn create_token(path: &Path) -> Result<()> {
    let bytes = Zeroizing::new(random_bytes::<32>()?);
    let token = Zeroizing::new(Hash32(*bytes).to_string());
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("create mesh token (refusing overwrite)")?;
    file.write_all(token.as_bytes())?;
    file.sync_all()?;
    Ok(())
}

pub fn read_token(path: &Path) -> Result<Zeroizing<String>> {
    ensure!(
        std::fs::metadata(path)?.len() <= 128,
        "invalid mesh token file size"
    );
    let mut token = Zeroizing::new(std::fs::read_to_string(path)?);
    let length = token.trim_end_matches(['\r', '\n']).len();
    token.truncate(length);
    ensure!(
        token.len() == 64
            && token
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "invalid mesh token file"
    );
    Ok(token)
}

#[derive(Clone)]
struct Guard {
    token: Arc<Zeroizing<String>>,
    quota: Arc<Mutex<(Instant, u32)>>,
    slots: Arc<tokio::sync::Semaphore>,
}

/// Read-only peer surface. No wallet, signing, balance, UI or mutation routes.
pub fn router(node: SharedNode, token: Zeroizing<String>) -> Router {
    Router::new()
        .route("/v1/status", get(crate::status))
        .route("/v1/genesis", get(crate::genesis))
        .route("/v1/blocks", get(crate::blocks))
        .route("/v1/mempool", get(crate::mempool))
        .with_state(node)
        .layer(middleware::from_fn_with_state(
            Guard {
                token: Arc::new(token),
                quota: Arc::new(Mutex::new((Instant::now(), 0))),
                slots: Arc::new(tokio::sync::Semaphore::new(16)),
            },
            guard,
        ))
}

async fn guard(State(guard): State<Guard>, request: Request, next: Next) -> Response {
    let allowed = !request.headers().contains_key("origin")
        && request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .is_some_and(|value| bool::from(value.as_bytes().ct_eq(guard.token.as_bytes())));
    if !allowed {
        return (StatusCode::UNAUTHORIZED, "Mesh authentication required").into_response();
    }
    {
        let Ok(mut quota) = guard.quota.lock() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        if quota.0.elapsed() >= Duration::from_secs(1) {
            *quota = (Instant::now(), 0);
        }
        if quota.1 >= 64 {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        quota.1 += 1;
    }
    let Ok(_slot) = guard.slots.clone().try_acquire_owned() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

pub struct Peer {
    url: String,
    client: Client,
}
impl Peer {
    pub fn new(url: &str, token: &str) -> Result<Self> {
        Ok(Self {
            url: url.trim_end_matches('/').into(),
            client: Client::new_mesh(url, token)?,
        })
    }
}

pub struct Network {
    node: SharedNode,
    peers: Vec<Peer>,
    authority: Option<SecretKey>,
    evidence: PathBuf,
    block_every: Duration,
    last_produced: Instant,
}
impl Network {
    pub fn new(
        node: SharedNode,
        peers: Vec<Peer>,
        authority: Option<SecretKey>,
        expected_chain: Hash32,
        evidence_dir: &Path,
        block_every: Duration,
    ) -> Result<Self> {
        ensure!(
            peers.len() <= MAX_PEERS,
            "at most eight explicitly configured peers are supported"
        );
        for (i, peer) in peers.iter().enumerate() {
            ensure!(
                !peers[..i].iter().any(|p| p.url == peer.url),
                "duplicate peer URL"
            );
        }
        ensure!(
            (1..=300).contains(&block_every.as_secs()),
            "block interval must be 1..300 seconds"
        );
        std::fs::create_dir_all(evidence_dir).context("create conflict evidence directory")?;
        let evidence = evidence_dir.join("authority-conflict.json");
        {
            let mut node = locked(&node)?;
            ensure!(
                node.ledger.genesis.chain_id() == expected_chain,
                "configured chain ID does not match database"
            );
            if let Some(key) = &authority {
                ensure!(
                    key.address() == node.ledger.genesis.authority,
                    "wallet is not the genesis authority"
                );
                ensure!(
                    node.ledger.genesis.authority != node.ledger.genesis.recipient,
                    "automatic production requires a separate authority key; do not load the funded genesis wallet"
                );
            }
            node.runtime = RuntimeStatus {
                enabled: true,
                transport: "private-vpn-http".into(),
                role: if authority.is_some() {
                    "authority"
                } else {
                    "replica"
                }
                .into(),
                halted: evidence.try_exists()?,
                peers: peers
                    .iter()
                    .map(|p| PeerStatus {
                        url: p.url.clone(),
                        state: "connecting".into(),
                        height: None,
                    })
                    .collect(),
                produced_blocks: 0,
                ..Default::default()
            };
        }
        Ok(Self {
            node,
            peers,
            authority,
            evidence,
            block_every,
            last_produced: Instant::now(),
        })
    }

    /// Runs one bounded page per peer; no state lock is held during network I/O.
    pub async fn step(&mut self) -> Result<()> {
        if locked(&self.node)?.runtime.halted {
            return Ok(());
        }
        let mut catching_up = false;
        for index in 0..self.peers.len() {
            let outcome = self.pull(index).await;
            // Evidence takes priority over all ordinary connectivity outcomes.
            if locked(&self.node)?.runtime.halted {
                ensure!(
                    self.evidence.try_exists()?,
                    "authority conflict could not be persisted; keep this node stopped and preserve peer histories"
                );
                return Ok(());
            }
            let (state, height) = match outcome {
                Ok((behind, height)) => {
                    catching_up |= behind;
                    (
                        if behind { "catching_up" } else { "connected" },
                        Some(height),
                    )
                }
                Err(_) => ("unavailable_or_rejected", None),
            };
            let mut node = locked(&self.node)?;
            let peer = &mut node.runtime.peers[index];
            if peer.state != state {
                eprintln!("ORVEXA peer {}: {state}", self.peers[index].url);
            }
            peer.state = state.into();
            peer.height = peer.height.max(height);
        }
        {
            let node = locked(&self.node)?;
            catching_up |= node
                .runtime
                .peers
                .iter()
                .any(|p| p.height.is_some_and(|h| h > node.ledger.height()));
        }
        if !catching_up && self.last_produced.elapsed() >= self.block_every {
            if let Some(key) = &self.authority {
                let mut node = locked(&self.node)?;
                if !node.mempool.is_empty() {
                    let block =
                        orvexa_wallet::sign_block(key, &node.ledger.genesis, node.template()?)?;
                    let id = node.commit(block)?;
                    node.runtime.produced_blocks = node.runtime.produced_blocks.saturating_add(1);
                    eprintln!(
                        "ORVEXA authority accepted block {} {id}",
                        node.ledger.height()
                    );
                }
            }
            self.last_produced = Instant::now();
        }
        Ok(())
    }

    pub async fn run(mut self, sync_every: Duration) -> Result<()> {
        ensure!(
            (1..=60).contains(&sync_every.as_secs()),
            "sync interval must be 1..60 seconds"
        );
        let mut clock = tokio::time::interval(sync_every);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            clock.tick().await;
            self.step().await?;
        }
    }

    async fn pull(&self, index: usize) -> Result<(bool, u64)> {
        let client = &self.peers[index].client;
        let status = client.status().await?;
        let genesis = { locked(&self.node)?.ledger.genesis.clone() };
        ensure!(
            status.protocol == VERSION && status.chain_id == genesis.chain_id(),
            "foreign peer network"
        );
        ensure!(client.genesis().await? == genesis, "peer genesis mismatch");
        // A peer advertising a newer chain must not turn a later fetch failure
        // into permission for this authority to sign from an older prefix.
        {
            let mut node = locked(&self.node)?;
            let peer = &mut node.runtime.peers[index];
            peer.height = peer.height.max(Some(status.height));
        }
        let height = { locked(&self.node)?.ledger.height() };
        let common = height.min(status.height);
        if common > 0 {
            let prefix = client.blocks(common).await?;
            let remote = prefix.first().context("missing peer prefix block")?;
            ensure!(
                prefix.len() <= BLOCK_PAGE && remote.header.height == common,
                "invalid peer prefix page"
            );
            self.overlap(remote)?;
            if common == status.height {
                ensure!(remote.id()? == status.tip, "peer tip observation changed");
            }
        } else if status.height == 0 {
            ensure!(status.tip == genesis.chain_id(), "invalid genesis tip");
        }
        if status.height > height {
            let page = client
                .blocks(height.checked_add(1).context("height overflow")?)
                .await?;
            ensure!(
                !page.is_empty() && page.len() <= BLOCK_PAGE,
                "invalid peer block page"
            );
            for (offset, block) in page
                .into_iter()
                .take((status.height - height).min(BLOCK_PAGE as u64) as usize)
                .enumerate()
            {
                ensure!(
                    block.header.height == height + offset as u64 + 1,
                    "noncontiguous peer blocks"
                );
                // A local UI/manual commit may have advanced while HTTP was in flight.
                if block.header.height <= locked(&self.node)?.ledger.height() {
                    self.overlap(&block)?;
                } else {
                    locked(&self.node)?.commit(block)?;
                }
            }
        }
        let behind = { locked(&self.node)?.ledger.height() < status.height };
        // Never import pending transactions against a known incomplete prefix.
        if !behind {
            let pool = client.mempool().await?;
            ensure!(pool.len() <= MAX_MEMPOOL, "peer pending pool exceeds bound");
            for tx in pool {
                let mut node = locked(&self.node)?;
                if !node.mempool.contains(&tx) {
                    let _ = node.submit(tx);
                }
            }
        }
        Ok((behind, status.height))
    }

    fn overlap(&self, remote: &Block) -> Result<()> {
        check_overlap(&self.node, remote, &self.evidence)
    }
}

pub(crate) fn check_overlap(
    shared: &SharedNode,
    remote: &Block,
    evidence_path: &Path,
) -> Result<()> {
    let mut node = locked(shared)?;
    let index = usize::try_from(
        remote
            .header
            .height
            .checked_sub(1)
            .context("invalid overlap height")?,
    )?;
    let local = node
        .ledger
        .blocks()
        .get(index)
        .context("missing local prefix")?;
    if local.id()? == remote.id()? {
        return Ok(());
    }
    ensure!(
        remote.header.version == VERSION
            && remote.header.chain_id == node.ledger.genesis.chain_id(),
        "invalid conflict network/version"
    );
    ensure!(
        remote.header.transactions_hash == transaction_commitment(&remote.transactions)?,
        "invalid conflict body"
    );
    verify(
        node.ledger.genesis.authority,
        &remote.header.signing_bytes(),
        &remote.signature,
    )?;
    for tx in &remote.transactions {
        tx.verify(node.ledger.genesis.chain_id())?;
    }
    // Two authority-signed headers at the same height prove equivocation.
    // This does not claim the foreign branch's entire state was validated.
    let evidence = serde_json::json!({
        "kind": "conflicting_authority_signed_headers", "chain_id": node.ledger.genesis.chain_id(),
        "height": remote.header.height, "local": local, "remote": remote,
    });
    node.runtime.halted = true; // Fail closed even if durable evidence fails to write.
    if !evidence_path.try_exists()? {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(evidence_path)
            .context("cannot persist conflict evidence; stop this node")?;
        file.write_all(&serde_json::to_vec_pretty(&evidence)?)?;
        file.sync_all()?;
        #[cfg(unix)]
        std::fs::File::open(
            evidence_path
                .parent()
                .context("missing evidence directory")?,
        )?
        .sync_all()?;
    }
    eprintln!(
        "ORVEXA HALTED: conflicting authority signatures at height {}; inspect evidence directory",
        remote.header.height
    );
    anyhow::bail!("authority equivocation detected; no fork was selected")
}
