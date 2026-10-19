//! Network directory: the list of public v2 networks the app offers to join
//! with one button, so nobody has to pass network files around.
//!
//! It is published next to the update manifest (GitHub Pages) as
//! `directory.json` plus `directory.json.sig`, signed with the release key the
//! app already trusts for updates. A changed file, an old copy past its
//! expiry or a network whose genesis does not check out is refused, so the
//! web host cannot point people at another chain. The app fetches it only
//! when the person presses the button.

use crate::profile::NetworkProfile;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

pub const FORMAT: &str = "orvexa-v2-directory";
pub const VERSION: u32 = 1;
pub const SIGNATURE_DOMAIN: &[u8] = b"ORVEXA/v2-network-directory/v1\n";
pub const DEFAULT_URL: &str = "https://waxiechc.github.io/orvexa/networks/directory.json";
pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_NETWORKS: usize = 16;
/// Clock skew tolerated for `issued_at`.
const SKEW: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Directory {
    pub format: String,
    pub version: u32,
    /// Unix seconds.
    pub issued_at: u64,
    pub expires_at: u64,
    /// Recommended network first.
    pub networks: Vec<NetworkProfile>,
}

impl Directory {
    pub fn new(networks: Vec<NetworkProfile>, issued_at: u64, valid_for: Duration) -> Self {
        Directory {
            format: FORMAT.into(),
            version: VERSION,
            issued_at,
            expires_at: issued_at + valid_for.as_secs(),
            networks,
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("plain struct") + "\n"
    }
}

fn message(bytes: &[u8]) -> Vec<u8> {
    [SIGNATURE_DOMAIN, bytes].concat()
}

/// Signs the exact file bytes; returns the content of the `.sig` file.
pub fn sign(bytes: &[u8], key: &SigningKey) -> String {
    hex::encode(key.sign(&message(bytes)).to_bytes()) + "\n"
}

/// Checks the signature with any of `trusted`, then the contents at time
/// `now` (unix seconds): format, expiry, and every network in full.
pub fn verify(
    bytes: &[u8],
    signature_hex: &str,
    trusted: &[[u8; 32]],
    now: u64,
) -> Result<Directory, String> {
    if bytes.len() > MAX_BYTES {
        return Err("The network list is too large.".into());
    }
    let signature: [u8; 64] = hex::decode(signature_hex.trim())
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or("The network list has no valid signature.")?;
    let signature = Signature::from_bytes(&signature);
    let signed = trusted.iter().any(|k| {
        VerifyingKey::from_bytes(k)
            .is_ok_and(|k| k.verify_strict(&message(bytes), &signature).is_ok())
    });
    if !signed {
        return Err("The network list is not signed by ORVEXA.".into());
    }
    let directory: Directory =
        serde_json::from_slice(bytes).map_err(|_| "The network list is damaged.".to_string())?;
    if directory.format != FORMAT || directory.version != VERSION {
        return Err("This network list needs a newer ORVEXA.".into());
    }
    if now >= directory.expires_at || directory.issued_at > now + SKEW {
        return Err("The published network list is out of date.".into());
    }
    if directory.networks.len() > MAX_NETWORKS {
        return Err("The network list is too large.".into());
    }
    let mut chains = HashSet::new();
    for profile in &directory.networks {
        profile
            .validate()
            .map_err(|e| format!("A listed network does not check out: {e}"))?;
        if profile.public_nodes().is_empty() {
            return Err("A listed network has no public node.".into());
        }
        if !chains.insert(profile.chain_id.clone()) {
            return Err("The network list repeats a network.".into());
        }
    }
    Ok(directory)
}

/// Downloads `url` and `url.sig` and verifies them.
pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    trusted: &[[u8; 32]],
    now: u64,
) -> Result<Directory, String> {
    let get = |url: String| async move {
        let response = http
            .get(&url)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| {
                "The network list cannot be reached. Check the internet connection.".to_string()
            })?;
        if !response.status().is_success() {
            return Err("No public network is published yet.".to_string());
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "The network list download was interrupted.".to_string())?;
        if bytes.len() > MAX_BYTES {
            return Err("The network list is too large.".to_string());
        }
        Ok(bytes)
    };
    let body = get(url.to_owned()).await?;
    let signature = get(format!("{url}.sig")).await?;
    let signature = std::str::from_utf8(&signature)
        .map_err(|_| "The network list has no valid signature.".to_string())?;
    verify(&body, signature, trusted, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Params;
    use crate::seed_wallet::SeedWallet;
    use crate::wallet::build_genesis;

    fn profile(salt: u8) -> NetworkProfile {
        let mut params = Params::test(1_000);
        params.salt = [salt; 32];
        let genesis = build_genesis(&params, &[(SeedWallet::generate().address(), 1_000)]);
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
    fn only_a_fresh_signed_list_of_valid_networks_is_accepted() {
        let key = SigningKey::from_bytes(&[1; 32]);
        let trusted = [key.verifying_key().to_bytes()];
        let now = 1_800_000_000;
        let dir = Directory::new(
            vec![profile(1), profile(2)],
            now,
            Duration::from_secs(90 * 86_400),
        );
        let bytes = dir.to_json().into_bytes();
        let sig = sign(&bytes, &key);
        assert_eq!(verify(&bytes, &sig, &trusted, now + 10).unwrap(), dir);

        let stranger = SigningKey::from_bytes(&[2; 32]);
        assert!(
            verify(&bytes, &sign(&bytes, &stranger), &trusted, now).is_err(),
            "another key"
        );
        let mut edited = bytes.clone();
        let at = edited.iter().position(|&b| b == b'1').unwrap();
        edited[at] = b'2';
        assert!(
            verify(&edited, &sig, &trusted, now).is_err(),
            "edited after signing"
        );
        assert!(
            verify(&bytes, &sig, &trusted, now + 91 * 86_400).is_err(),
            "expired"
        );
        assert!(verify(&bytes, "zz", &trusted, now).is_err());

        // Signed, but a listed network's genesis does not match its chain ID.
        let mut bad = profile(3);
        bad.chain_id = hex::encode([0u8; 32]);
        let bytes = Directory::new(vec![bad], now, Duration::from_secs(86_400))
            .to_json()
            .into_bytes();
        assert!(verify(&bytes, &sign(&bytes, &key), &trusted, now).is_err());
        // Signed, but the same network twice.
        let p = profile(4);
        let bytes = Directory::new(vec![p.clone(), p], now, Duration::from_secs(86_400))
            .to_json()
            .into_bytes();
        assert!(verify(&bytes, &sign(&bytes, &key), &trusted, now).is_err());
    }
}
