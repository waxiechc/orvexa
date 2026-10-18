//! JSON shapes of the local v2 node API. Every field here is public chain
//! data: heights, hashes, fees, counts, nullifiers and commitments. No
//! address, amount, memo, key or note plaintext exists in any response.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusView {
    pub network: String,
    pub privacy: String,
    pub role: String,
    pub chain_id: String,
    pub height: u64,
    pub tip: String,
    pub burned_fees: u64,
    pub remaining_supply: u64,
    pub mempool: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockView {
    pub height: u64,
    pub id: String,
    pub prev: String,
    pub confirmations: u64,
    pub txids: Vec<String>,
    pub actions: u64,
    pub fees: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TxView {
    pub txid: String,
    /// `None` while the transaction waits in the mempool.
    pub height: Option<u64>,
    pub confirmations: u64,
    pub fee: u64,
    pub expiry_height: u64,
    pub actions: usize,
    pub bytes: usize,
    pub nullifiers: Vec<String>,
    pub commitments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawView {
    pub hex: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitRequest {
    pub tx: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitResponse {
    pub txid: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorView {
    pub code: String,
    pub message: String,
}
