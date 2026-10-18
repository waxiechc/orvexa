//! Password-encrypted container for the seed wallet. Same construction as
//! the v1 wallet file (Argon2id 64 MiB / 3 / 1 → XChaCha20-Poly1305, public
//! metadata as AAD), new format name and version. The plaintext is only the
//! 32-byte BIP39 entropy; notes are always rebuilt by rescanning.

use crate::seed_wallet::{SeedWallet, COIN_TYPE, WORDS};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::rand_core::Rng as _;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use zeroize::Zeroizing;

pub const FORMAT: &str = "orvexa-seed-wallet";
pub const VERSION: u32 = 2;
const KDF: &str = "argon2id-v19";
const MEMORY_KIB: u32 = 65536;
const ITERATIONS: u32 = 3;
const LANES: u32 = 1;
const MAX_FILE: u64 = 8192;
pub const MIN_PASSWORD_CHARS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedFileError {
    WeakPassword,
    AlreadyExists,
    TooLarge,
    /// Unparseable, unknown format/version or unsupported KDF parameters.
    Format,
    /// Wrong password or modified file; deliberately indistinguishable.
    Decrypt,
    Io(String),
}

impl From<std::io::Error> for SeedFileError {
    fn from(e: std::io::Error) -> Self {
        SeedFileError::Io(e.kind().to_string())
    }
}

/// Public part of the file: authenticated (AAD), not secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub format: String,
    pub version: u32,
    pub chain_id: String,
    pub coin_type: u32,
    pub account: u32,
    /// Height to start scanning from. 0 until birthday frontiers exist.
    pub birthday: u64,
    pub words: u32,
    pub kdf: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
    pub salt: String,
    pub nonce: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Container {
    metadata: Metadata,
    ciphertext: String,
}

fn derive(password: &str, salt: &[u8; 16]) -> Zeroizing<[u8; 32]> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(32)).expect("fixed parameters");
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .expect("fixed parameters and 16-byte salt");
    key
}

/// Encrypts the wallet's seed. Never overwrites `path`.
pub fn create(path: &Path, wallet: &SeedWallet, chain_id: [u8; 32], password: &str) -> Result<Metadata, SeedFileError> {
    if password.chars().count() < MIN_PASSWORD_CHARS {
        return Err(SeedFileError::WeakPassword);
    }
    if path.exists() {
        return Err(SeedFileError::AlreadyExists);
    }
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 24];
    crate::keys::os_rng().fill_bytes(&mut salt);
    crate::keys::os_rng().fill_bytes(&mut nonce);
    let metadata = Metadata {
        format: FORMAT.into(),
        version: VERSION,
        chain_id: hex::encode(chain_id),
        coin_type: COIN_TYPE,
        account: wallet.account(),
        birthday: 0,
        words: WORDS as u32,
        kdf: KDF.into(),
        memory_kib: MEMORY_KIB,
        iterations: ITERATIONS,
        lanes: LANES,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
    };
    let aad = serde_json::to_vec(&metadata).expect("plain struct");
    let key = derive(password, &salt);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).expect("32-byte key");
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: wallet.entropy(), aad: &aad })
        .map_err(|_| SeedFileError::Decrypt)?;
    let body = serde_json::to_vec_pretty(&Container { metadata: metadata.clone(), ciphertext: hex::encode(ciphertext) })
        .expect("plain struct");
    write_new(path, &body)?;
    Ok(metadata)
}

/// Write to a fresh temporary file, fsync, then hard-link it into place:
/// the link fails if `path` exists, so an existing backup is never replaced,
/// and a crash leaves either no file or a complete one.
fn write_new(path: &Path, body: &[u8]) -> Result<(), SeedFileError> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut tag = [0u8; 8];
    crate::keys::os_rng().fill_bytes(&mut tag);
    let temp = dir.join(format!(".orvexa-seed-{}.tmp", hex::encode(tag)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temp)?;
        file.write_all(body)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temp, path) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(SeedFileError::AlreadyExists),
            other => other.map_err(SeedFileError::from),
        }
    })();
    let _ = fs::remove_file(&temp);
    result?;
    #[cfg(unix)]
    File::open(dir)?.sync_all()?;
    Ok(())
}

pub fn read_metadata(path: &Path) -> Result<Metadata, SeedFileError> {
    Ok(read(path)?.metadata)
}

/// Decrypts the seed and returns a wallet ready to rescan from `birthday`.
pub fn unlock(path: &Path, password: &str) -> Result<(SeedWallet, Metadata), SeedFileError> {
    let container = read(path)?;
    let m = &container.metadata;
    let salt: [u8; 16] = hex_array(&m.salt)?;
    let nonce: [u8; 24] = hex_array(&m.nonce)?;
    let ciphertext = hex::decode(&container.ciphertext).map_err(|_| SeedFileError::Format)?;
    let aad = serde_json::to_vec(m).expect("plain struct");
    let key = derive(password, &salt);
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref()).expect("32-byte key");
    let entropy = Zeroizing::new(
        cipher
            .decrypt(XNonce::from_slice(&nonce), Payload { msg: &ciphertext, aad: &aad })
            .map_err(|_| SeedFileError::Decrypt)?,
    );
    let wallet = SeedWallet::from_entropy(&entropy, m.account).map_err(|_| SeedFileError::Format)?;
    Ok((wallet, container.metadata))
}

fn read(path: &Path) -> Result<Container, SeedFileError> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_FILE {
        return Err(SeedFileError::TooLarge);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(SeedFileError::TooLarge);
    }
    let container: Container = serde_json::from_slice(&bytes).map_err(|_| SeedFileError::Format)?;
    let m = &container.metadata;
    let supported = m.format == FORMAT
        && m.version == VERSION
        && m.coin_type == COIN_TYPE
        && m.words == WORDS as u32
        && m.kdf == KDF
        && m.memory_kib == MEMORY_KIB
        && m.iterations == ITERATIONS
        && m.lanes == LANES;
    if !supported {
        return Err(SeedFileError::Format);
    }
    Ok(container)
}

fn hex_array<const N: usize>(s: &str) -> Result<[u8; N], SeedFileError> {
    hex::decode(s).ok().and_then(|v| v.try_into().ok()).ok_or(SeedFileError::Format)
}
