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
/// 2: several validators with commit certificates (1 had one producer).
pub const VERSION: u32 = 2;
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
    /// Validators that decide blocks (more than 2/3 must agree), sorted by key.
    pub validators: Vec<ValidatorEntry>,
    /// Genesis envelope, hex.
    pub genesis: String,
    /// Public v2 node API base URLs (`https://…`); empty for a local network.
    pub nodes: Vec<String>,
}

/// A validator: its Ed25519 public key (part of the chain ID) and the
/// address of its node, where the other validators send consensus messages
/// (empty when it has none, e.g. a one-computer network).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValidatorEntry {
    pub key: String,
    pub url: String,
}

impl NetworkProfile {
    /// `urls` maps validator keys to node addresses; missing ones stay empty.
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
            validators: params
                .validators
                .iter()
                .map(|k| ValidatorEntry {
                    key: hex::encode(k),
                    url: String::new(),
                })
                .collect(),
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
            validators: self
                .validators
                .iter()
                .map(|v| hex32(&v.key).ok_or_else(|| "validator key".to_string()))
                .collect::<Result<_, _>>()?,
        })
    }

    /// Sets the node address of the validator with `key`.
    pub fn set_validator_url(&mut self, key: &[u8; 32], url: &str) -> bool {
        let key = hex::encode(key);
        match self.validators.iter_mut().find(|v| v.key == key) {
            Some(v) => {
                v.url = url.trim_end_matches('/').to_owned();
                true
            }
            None => false,
        }
    }

    /// Node addresses of the validators other than `key`.
    pub fn peers_of(&self, key: &[u8; 32]) -> Vec<String> {
        let key = hex::encode(key);
        self.validators
            .iter()
            .filter(|v| v.key != key && !v.url.is_empty())
            .map(|v| v.url.clone())
            .collect()
    }

    /// Public nodes a follower can sync from: `nodes`, then validator nodes.
    pub fn public_nodes(&self) -> Vec<String> {
        let mut out = self.nodes.clone();
        for v in &self.validators {
            if !v.url.is_empty() && !out.contains(&v.url) {
                out.push(v.url.clone());
            }
        }
        out
    }

    pub fn is_validator(&self, key: &[u8; 32]) -> bool {
        let key = hex::encode(key);
        self.validators.iter().any(|v| v.key == key)
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
        let url_ok = |url: &str| {
            (url.starts_with("https://") || url.starts_with("http://"))
                && url.len() <= 200
                && !url.contains(char::is_whitespace)
                && !url.ends_with('/')
        };
        if self
            .validators
            .iter()
            .any(|v| !v.url.is_empty() && !url_ok(&v.url))
        {
            return Err(
                "validator node addresses must be http(s):// URLs without a trailing slash".into(),
            );
        }
        let params = self.params()?;
        let mut sorted = params.validators.clone();
        sorted.sort();
        if sorted != params.validators {
            return Err("validators must be listed sorted by key".into());
        }
        params.check_validators().map_err(str::to_string)?;
        for (i, url) in self.nodes.iter().enumerate() {
            if !url_ok(url) || self.nodes[..i].contains(url) {
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

/// Reads a 32-byte validator key file.
pub fn read_validator_key(path: &Path) -> Result<SigningKey, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read validator key: {e}"))?;
    let raw: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "validator key file must be 32 bytes".to_string())?;
    Ok(SigningKey::from_bytes(&raw))
}

/// Creates a new validator key file (0600). Never overwrites.
pub fn create_validator_key(path: &Path) -> Result<SigningKey, String> {
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
        .map_err(|e| format!("create validator key: {e}"))?;
    file.write_all(raw.as_ref()).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    Ok(SigningKey::from_bytes(&raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::test_validator;
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
            good.params().unwrap().validators,
            vec![test_validator().verifying_key().to_bytes()]
        );

        let mut p = good.clone();
        p.chain_id = hex::encode([0u8; 32]);
        assert!(p.validate().is_err(), "chain ID must be recomputed");
        let mut p = good.clone();
        p.validators[0].key =
            hex::encode(SigningKey::from_bytes(&[4; 32]).verifying_key().to_bytes());
        assert!(
            p.validate().is_err(),
            "another validator set means another chain"
        );
        let mut p = good.clone();
        p.validators[0].url = "ftp://x".into();
        assert!(p.validate().is_err());
        let mut p = good.clone();
        assert!(p.set_validator_url(
            &test_validator().verifying_key().to_bytes(),
            "https://v1.example/"
        ));
        assert!(
            p.validate().is_ok(),
            "validator addresses can change without changing the chain"
        );
        assert_eq!(
            p.public_nodes(),
            vec![
                "https://node.example".to_string(),
                "https://v1.example".to_string()
            ]
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
