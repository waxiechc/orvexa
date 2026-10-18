//! Signed update manifest.
//!
//! The manifest is a small JSON file published next to the site
//! (`docs/updates/<channel>.json`) with a detached Ed25519 signature
//! (`<channel>.json.sig`, hex). The signature covers the exact manifest bytes
//! under the domain [`SIGNATURE_DOMAIN`]. The app trusts only keys compiled into
//! it, so a compromised host or mirror can at most withhold an update — it
//! cannot make the app install anything else. `expires` bounds how long a
//! withheld (frozen) manifest stays acceptable.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

pub const FORMAT: &str = "orvexa-update";
pub const VERSION: u32 = 1;
pub const SIGNATURE_DOMAIN: &[u8] = b"ORVEXA/update-manifest/v1\n";
/// Downloads are only accepted from this repository's release assets.
pub const ALLOWED_PREFIX: &str = "https://github.com/waxiechc/orvexa/releases/download/";
pub const MAX_ARTIFACT_BYTES: u64 = 300 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub url: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub channel: String,
    /// Release version, e.g. `0.1.0-desktop-preview.5`.
    pub release: String,
    /// Unix seconds after which the manifest is no longer trusted.
    pub expires: u64,
    pub notes_ru: String,
    pub notes_en: String,
    /// Platform key (`macos-universal`, `windows-x64`) → file.
    pub files: BTreeMap<String, Artifact>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    TooLarge,
    BadSignature,
    Format,
    Expired,
    UnsafeUrl,
}

/// Platform key of this build.
pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos-universal"
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        "windows-x64"
    } else {
        "unsupported"
    }
}

fn message(bytes: &[u8]) -> Vec<u8> {
    [SIGNATURE_DOMAIN, bytes].concat()
}

/// Signs manifest bytes; returns the hex signature file content.
pub fn sign(bytes: &[u8], key: &SigningKey) -> String {
    hex::encode(key.sign(&message(bytes)).to_bytes()) + "\n"
}

/// Verifies `signature_hex` over `bytes` with any of `trusted`, then parses
/// and checks the manifest at time `now` (unix seconds).
pub fn verify(
    bytes: &[u8],
    signature_hex: &str,
    trusted: &[VerifyingKey],
    now: u64,
) -> Result<Manifest, ManifestError> {
    verify_with_prefix(bytes, signature_hex, trusted, now, ALLOWED_PREFIX)
}

/// [`verify`] with another download prefix (tests serve files locally).
pub fn verify_with_prefix(
    bytes: &[u8],
    signature_hex: &str,
    trusted: &[VerifyingKey],
    now: u64,
    prefix: &str,
) -> Result<Manifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(ManifestError::TooLarge);
    }
    let raw: [u8; 64] = hex::decode(signature_hex.trim())
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(ManifestError::BadSignature)?;
    let signature = Signature::from_bytes(&raw);
    if !trusted
        .iter()
        .any(|k| k.verify_strict(&message(bytes), &signature).is_ok())
    {
        return Err(ManifestError::BadSignature);
    }
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|_| ManifestError::Format)?;
    if manifest.format != FORMAT
        || manifest.version != VERSION
        || parse_version(&manifest.release).is_none()
    {
        return Err(ManifestError::Format);
    }
    if manifest.expires <= now {
        return Err(ManifestError::Expired);
    }
    for file in manifest.files.values() {
        let path = &file.url[prefix.len().min(file.url.len())..];
        let safe = file.url.starts_with(prefix)
            && !path.contains("..")
            && !file.url.contains(char::is_whitespace)
            && file.bytes > 0
            && file.bytes <= MAX_ARTIFACT_BYTES
            && file.sha256.len() == 64
            && hex::decode(&file.sha256).is_ok();
        if !safe {
            return Err(ManifestError::UnsafeUrl);
        }
    }
    Ok(manifest)
}

/// `MAJOR.MINOR.PATCH[-pre.release]` → (core, pre-release identifiers).
fn parse_version(v: &str) -> Option<([u64; 3], Vec<String>)> {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, p.split('.').map(str::to_owned).collect()),
        None => (v, Vec::new()),
    };
    let parts: Vec<u64> = core
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let core: [u64; 3] = parts.try_into().ok()?;
    if pre
        .iter()
        .any(|p: &String| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    {
        return None;
    }
    Some((core, pre))
}

/// Semantic-version precedence (pre-releases sort before the release; numeric
/// identifiers compare numerically, so preview.10 > preview.9).
pub fn compare(a: &str, b: &str) -> Option<Ordering> {
    let (ca, pa) = parse_version(a)?;
    let (cb, pb) = parse_version(b)?;
    Some(
        ca.cmp(&cb)
            .then_with(|| match (pa.is_empty(), pb.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    for (x, y) in pa.iter().zip(&pb) {
                        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
                            (Ok(nx), Ok(ny)) => nx.cmp(&ny),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            _ => x.cmp(y),
                        };
                        if o != Ordering::Equal {
                            return o;
                        }
                    }
                    pa.len().cmp(&pb.len())
                }
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(release: &str, url: &str, expires: u64) -> Vec<u8> {
        let mut files = BTreeMap::new();
        files.insert(
            "macos-universal".into(),
            Artifact {
                url: url.into(),
                sha256: "ab".repeat(32),
                bytes: 10,
            },
        );
        serde_json::to_vec_pretty(&Manifest {
            format: FORMAT.into(),
            version: VERSION,
            channel: "preview".into(),
            release: release.into(),
            expires,
            notes_ru: "Исправления".into(),
            notes_en: "Fixes".into(),
            files,
        })
        .unwrap()
    }

    #[test]
    fn versions_order_like_semver() {
        use Ordering::*;
        assert_eq!(
            compare("0.1.0-desktop-preview.5", "0.1.0-desktop-preview.4"),
            Some(Greater)
        );
        assert_eq!(
            compare("0.1.0-desktop-preview.10", "0.1.0-desktop-preview.9"),
            Some(Greater)
        );
        assert_eq!(compare("0.1.0", "0.1.0-desktop-preview.99"), Some(Greater));
        assert_eq!(compare("0.2.0-desktop-preview.1", "0.1.9"), Some(Greater));
        assert_eq!(
            compare("0.1.0-desktop-preview.4", "0.1.0-desktop-preview.4"),
            Some(Equal)
        );
        assert_eq!(compare("dev", "0.1.0"), None);
    }

    #[test]
    fn only_signed_fresh_repo_manifests_pass() {
        let key = SigningKey::from_bytes(&[5; 32]);
        let trusted = [key.verifying_key()];
        let good = manifest(
            "0.1.0-desktop-preview.5",
            &format!("{ALLOWED_PREFIX}v0.1.0-desktop-preview.5/ORVEXA.zip"),
            2_000,
        );
        let sig = sign(&good, &key);
        assert!(verify(&good, &sig, &trusted, 1_000).is_ok());

        let mut tampered = good.clone();
        let i = tampered.len() / 2;
        tampered[i] ^= 1;
        assert_eq!(
            verify(&tampered, &sig, &trusted, 1_000),
            Err(ManifestError::BadSignature)
        );
        let other = SigningKey::from_bytes(&[6; 32]);
        assert_eq!(
            verify(&good, &sign(&good, &other), &trusted, 1_000),
            Err(ManifestError::BadSignature)
        );
        assert_eq!(
            verify(&good, &sig, &trusted, 2_000),
            Err(ManifestError::Expired)
        );
        let evil = manifest(
            "0.1.0-desktop-preview.5",
            "https://evil.example/ORVEXA.zip",
            2_000,
        );
        assert_eq!(
            verify(&evil, &sign(&evil, &key), &trusted, 1_000),
            Err(ManifestError::UnsafeUrl)
        );
        let climb = manifest(
            "0.1.0-desktop-preview.5",
            &format!("{ALLOWED_PREFIX}../../other/x.zip"),
            2_000,
        );
        assert_eq!(
            verify(&climb, &sign(&climb, &key), &trusted, 1_000),
            Err(ManifestError::UnsafeUrl)
        );
        assert_eq!(
            verify(&good, "zz", &trusted, 1_000),
            Err(ManifestError::BadSignature)
        );
    }
}
