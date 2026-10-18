//! Network profile: the one public file that defines a v2 test network and
//! where to reach it. An operator creates it next to the genesis; the desktop
//! app imports it (or ships with it) and joins with one button.
//!
//! Everything in it is public. Trust comes from checking it, not from where
//! it was downloaded: the genesis proof and signatures are verified and the
//! chain ID is recomputed from the parameters and the genesis effects.

use crate::ledger::{Ledger, Params};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const FORMAT: &str = "orvexa-v2-network";
pub const VERSION: u32 = 1;
const MAX_NODES: usize = 16;
const MAX_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkProfile {
    pub format: String,
    pub version: u32,
    pub name: String,
    pub chain_id: String,
    pub salt: String,
    pub supply: u64,
    pub anchor_window: u32,
    pub max_expiry_delta: u64,
    /// Ed25519 public key of the block producer.
    pub producer: String,
    /// Genesis envelope, hex.
    pub genesis: String,
    /// Public v2 node API base URLs (`https://…`); empty for a local network.
    pub nodes: Vec<String>,
}

impl NetworkProfile {
    pub fn new(
        name: &str,
        params: &Params,
        genesis: &[u8],
        chain_id: [u8; 32],
        nodes: Vec<String>,
    ) -> Self {
        NetworkProfile {
            format: FORMAT.into(),
            version: VERSION,
            name: name.into(),
            chain_id: hex::encode(chain_id),
            salt: hex::encode(params.salt),
            supply: params.supply,
            anchor_window: params.anchor_window,
            max_expiry_delta: params.max_expiry_delta,
            producer: hex::encode(params.producer),
            genesis: hex::encode(genesis),
            nodes,
        }
    }

    pub fn params(&self) -> Result<Params, String> {
        Ok(Params {
            salt: hex32(&self.salt).ok_or("salt")?,
            supply: self.supply,
            anchor_window: self.anchor_window,
            max_expiry_delta: self.max_expiry_delta,
            producer: hex32(&self.producer).ok_or("producer key")?,
        })
    }

    pub fn genesis(&self) -> Result<Vec<u8>, String> {
        hex::decode(&self.genesis).map_err(|_| "genesis is not hex".to_string())
    }

    /// Parses and fully checks a profile.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.len() > MAX_BYTES {
            return Err("network profile is too large".into());
        }
        let profile: NetworkProfile = serde_json::from_str(text)
            .map_err(|_| "not an ORVEXA v2 network profile".to_string())?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT || self.version != VERSION {
            return Err("unsupported network profile version".into());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 60 {
            return Err("network name must be 1–60 characters".into());
        }
        if self.nodes.len() > MAX_NODES {
            return Err("too many node addresses".into());
        }
        for (i, url) in self.nodes.iter().enumerate() {
            let ok = (url.starts_with("https://") || url.starts_with("http://"))
                && url.len() <= 200
                && !url.contains(char::is_whitespace)
                && !url.ends_with('/');
            if !ok || self.nodes[..i].contains(url) {
                return Err(
                    "node addresses must be distinct http(s):// URLs without a trailing slash"
                        .into(),
                );
            }
        }
        let ledger = Ledger::from_genesis(self.params()?, &self.genesis()?)
            .map_err(|e| format!("genesis rejected: {e:?}"))?;
        if hex::encode(ledger.chain_id()) != self.chain_id {
            return Err("chain ID does not match the genesis and parameters".into());
        }
        Ok(())
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("plain struct") + "\n"
    }
}

fn hex32(s: &str) -> Option<[u8; 32]> {
    hex::decode(s).ok()?.try_into().ok()
}

/// Reads a 32-byte producer key file.
pub fn read_producer_key(path: &Path) -> Result<SigningKey, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read producer key: {e}"))?;
    let raw: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "producer key file must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&raw))
}

/// Creates a new producer key file (0600). Never overwrites.
pub fn create_producer_key(path: &Path) -> Result<SigningKey, String> {
    use rand::rand_core::Rng as _;
    use std::io::Write;
    let mut raw = zeroize::Zeroizing::new([0u8; 32]);
    crate::keys::os_rng().fill_bytes(raw.as_mut());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| format!("create producer key: {e}"))?;
    file.write_all(raw.as_ref()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(SigningKey::from_bytes(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::test_producer;
    use crate::seed_wallet::SeedWallet;
    use crate::wallet::build_genesis;

    fn profile() -> NetworkProfile {
        let params = Params::test(1_000_000);
        let genesis = build_genesis(&params, &[(SeedWallet::generate().address(), 1_000_000)]);
        let chain = crate::Envelope::decode(&genesis).unwrap().chain_id;
        NetworkProfile::new(
            "Тест",
            &params,
            &genesis,
            chain,
            vec!["https://node.example".into()],
        )
    }

    #[test]
    fn checks_everything_it_can() {
        let good = profile();
        assert_eq!(NetworkProfile::parse(&good.to_json()).unwrap(), good);
        assert_eq!(
            good.params().unwrap().producer,
            test_producer().verifying_key().to_bytes()
        );

        let mut p = good.clone();
        p.chain_id = hex::encode([0u8; 32]);
        assert!(p.validate().is_err(), "chain ID must be recomputed");
        let mut p = good.clone();
        p.producer = hex::encode(SigningKey::from_bytes(&[4; 32]).verifying_key().to_bytes());
        assert!(
            p.validate().is_err(),
            "another producer means another chain"
        );
        let mut p = good.clone();
        p.supply += 1;
        assert!(p.validate().is_err());
        let mut p = good.clone();
        p.nodes = vec!["ftp://x".into()];
        assert!(p.validate().is_err());
        let mut p = good.clone();
        p.nodes = vec!["https://a".into(), "https://a".into()];
        assert!(p.validate().is_err());
        let mut p = good;
        p.nodes = vec!["https://other.example".into()];
        assert!(
            p.validate().is_ok(),
            "node addresses can change without changing the chain"
        );
        assert!(NetworkProfile::parse("{\"format\":\"x\"}").is_err());
    }
}
