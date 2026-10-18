//! Browser-only signing adapter. The node never receives wallet/password material.
//! Reuses the native v1 container, signatures and canonical encodings unchanged.
use anyhow::{Result, ensure};
use orvexa_core::{Block, Genesis, Transaction, VERSION};
use orvexa_crypto::Hash32;
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

fn js_error(error: anyhow::Error) -> JsValue {
    JsValue::from_str(&error.to_string())
}

#[wasm_bindgen]
pub fn create_wallet(password: String) -> Result<String, JsValue> {
    orvexa_wallet::create_encrypted(&Zeroizing::new(password)).map_err(js_error)
}

#[wasm_bindgen]
pub fn inspect_wallet(encrypted: &str) -> Result<String, JsValue> {
    orvexa_wallet::address_from_json(encrypted)
        .map(|a| a.to_string())
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn authenticate_wallet(encrypted: &str, password: String) -> Result<String, JsValue> {
    orvexa_wallet::unlock_json(encrypted, &Zeroizing::new(password))
        .map(|k| k.address().to_string())
        .map_err(js_error)
}

#[wasm_bindgen]
pub fn validate_address(address: &str) -> bool {
    address.parse::<orvexa_crypto::Address>().is_ok()
}

#[wasm_bindgen]
pub fn verify_network(genesis_json: &str, expected_chain: &str) -> Result<(), JsValue> {
    (|| -> Result<()> {
        let value: serde_json::Value = serde_json::from_str(genesis_json)?;
        let genesis: Genesis =
            serde_json::from_value(value.get("genesis").cloned().unwrap_or(value))?;
        genesis.validate()?;
        ensure!(
            genesis.chain_id() == expected_chain.parse()?,
            "network identity mismatch"
        );
        Ok(())
    })()
    .map_err(js_error)
}

/// Numeric fields cross the JS boundary as decimal strings; never JS Numbers.
#[wasm_bindgen]
pub fn sign_transfer(
    encrypted: &str,
    password: String,
    transfer_json: &str,
) -> Result<String, JsValue> {
    (|| -> Result<String> {
        let fields: serde_json::Value = serde_json::from_str(transfer_json)?;
        let field = |name: &str| -> Result<&str> {
            fields
                .get(name)
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing string field: {name}"))
        };
        let key = orvexa_wallet::unlock_json(encrypted, &Zeroizing::new(password))?;
        let tx = Transaction {
            version: VERSION,
            chain_id: field("chain_id")?.parse::<Hash32>()?,
            sender: key.address(),
            recipient: field("recipient")?.parse()?,
            amount: field("amount")?.parse()?,
            fee: field("fee")?.parse()?,
            nonce: field("nonce")?.parse()?,
        };
        let signed = orvexa_wallet::sign_transaction(&key, tx)?;
        Ok(serde_json::to_string(&serde_json::json!({
            "id": signed.id()?,
            "signed_json": serde_json::to_string(&signed)?,
        }))?)
    })()
    .map_err(js_error)
}

#[wasm_bindgen]
pub fn authorize_block(
    encrypted: &str,
    password: String,
    genesis_json: &str,
    block_json: &str,
    expected_chain: &str,
) -> Result<String, JsValue> {
    (|| -> Result<String> {
        let genesis: Genesis = serde_json::from_str(genesis_json)?;
        ensure!(
            genesis.chain_id() == expected_chain.parse()?,
            "network identity mismatch"
        );
        let block: Block = serde_json::from_str(block_json)?;
        let key = orvexa_wallet::unlock_json(encrypted, &Zeroizing::new(password))?;
        Ok(serde_json::to_string(&orvexa_wallet::sign_block(
            &key, &genesis, block,
        )?)?)
    })()
    .map_err(js_error)
}
