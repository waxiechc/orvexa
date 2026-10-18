//! Wallet-side client of the v2 node API. The wallet downloads whole blocks
//! and scans them locally; it never sends a key, address or amount to a node.

use crate::api::{ErrorView, RawView, StatusView, SubmitRequest, SubmitResponse, TxView};
use crate::envelope::Envelope;
use crate::ledger::Block;
use crate::seed_wallet::SeedWallet;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Http(String),
    /// The node refused the request: code from its error body.
    Rejected { status: u16, code: String },
    BadData,
}

#[derive(Clone)]
pub struct NodeClient {
    base: String,
    http: reqwest::Client,
}

impl NodeClient {
    pub fn new(base: &str) -> Self {
        NodeClient { base: base.trim_end_matches('/').to_owned(), http: reqwest::Client::new() }
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let r = self
            .http
            .get(format!("{}{path}", self.base))
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        Self::decode(r).await
    }

    async fn decode<T: serde::de::DeserializeOwned>(r: reqwest::Response) -> Result<T, ClientError> {
        let status = r.status().as_u16();
        if !r.status().is_success() {
            let code = r.json::<ErrorView>().await.map(|e| e.code).unwrap_or_default();
            return Err(ClientError::Rejected { status, code });
        }
        r.json::<T>().await.map_err(|_| ClientError::BadData)
    }

    pub async fn status(&self) -> Result<StatusView, ClientError> {
        self.get("/v2/status").await
    }

    pub async fn tx(&self, txid: &[u8; 32]) -> Result<TxView, ClientError> {
        self.get(&format!("/v2/tx/{}", hex::encode(txid))).await
    }

    pub async fn tx_envelope(&self, txid: &[u8; 32]) -> Result<Envelope, ClientError> {
        let raw: RawView = self.get(&format!("/v2/tx/{}/raw", hex::encode(txid))).await?;
        let bytes = hex::decode(raw.hex).map_err(|_| ClientError::BadData)?;
        let env = Envelope::decode(&bytes).map_err(|_| ClientError::BadData)?;
        // The node cannot substitute another transaction for this txid.
        if env.txid() != *txid {
            return Err(ClientError::BadData);
        }
        Ok(env)
    }

    pub async fn block(&self, height: u64) -> Result<Block, ClientError> {
        let raw: RawView = self.get(&format!("/v2/blocks/{height}/raw")).await?;
        let bytes = hex::decode(raw.hex).map_err(|_| ClientError::BadData)?;
        Block::decode(&bytes).filter(|b| b.height == height).ok_or(ClientError::BadData)
    }

    pub async fn submit(&self, env: &Envelope) -> Result<[u8; 32], ClientError> {
        let r = self
            .http
            .post(format!("{}/v2/tx", self.base))
            .json(&SubmitRequest { tx: hex::encode(env.encode()) })
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| ClientError::Http(e.to_string()))?;
        let body: SubmitResponse = Self::decode(r).await?;
        let txid: [u8; 32] = hex::decode(body.txid).ok().and_then(|v| v.try_into().ok()).ok_or(ClientError::BadData)?;
        if txid != env.txid() {
            return Err(ClientError::BadData);
        }
        Ok(txid)
    }

    /// Downloads and scans every block the wallet has not seen. Returns the
    /// new scanned-to height. Each block's own tree root is cross-checked by
    /// the wallet's anchor when it next spends; a node lying about blocks
    /// produces spends the honest chain rejects.
    pub async fn sync(&self, wallet: &mut SeedWallet) -> Result<u64, ClientError> {
        let tip = self.status().await?.height;
        while wallet.scanned_to() <= tip {
            let height = wallet.scanned_to();
            let block = self.block(height).await?;
            let envs: Vec<Envelope> =
                block.txs.iter().map(|t| Envelope::decode(t)).collect::<Result<_, _>>().map_err(|_| ClientError::BadData)?;
            wallet.scan_block(height, &envs).map_err(|_| ClientError::BadData)?;
        }
        Ok(wallet.scanned_to())
    }
}
