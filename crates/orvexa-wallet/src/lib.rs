//! Password-encrypted local keys. No secret data is sent to a node.
use anyhow::{Context, Result, bail, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use orvexa_core::{
    Block, Genesis, SignedTransaction, Transaction, VERSION, transaction_commitment,
};
use orvexa_crypto::{Address, SecretKey, decode, random_bytes};
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
use zeroize::Zeroizing;

const MEMORY_KIB: u32 = 65536;
const ITERATIONS: u32 = 3;
const LANES: u32 = 1;
const MAX_FILE: u64 = 8192;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    version: u32,
    address: Address,
    kdf: String,
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
    salt: String,
    nonce: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedWallet {
    metadata: Metadata,
    ciphertext: String,
}

fn derive(password: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(32))
        .map_err(|e| anyhow::anyhow!("KDF parameters: {e}"))?;
    let mut key = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt, key.as_mut())
        .map_err(|e| anyhow::anyhow!("key derivation failed: {e}"))?;
    Ok(key)
}

fn read_wallet(path: &Path) -> Result<EncryptedWallet> {
    let file = File::open(path).context("open encrypted wallet")?;
    ensure!(
        file.metadata()?.len() <= MAX_FILE,
        "wallet file exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_FILE,
        "wallet file exceeds size limit"
    );
    parse_wallet(&bytes)
}

fn parse_wallet(bytes: &[u8]) -> Result<EncryptedWallet> {
    ensure!(
        bytes.len() as u64 <= MAX_FILE,
        "wallet file exceeds size limit"
    );
    let wallet: EncryptedWallet = serde_json::from_slice(bytes).context("invalid wallet file")?;
    let m = &wallet.metadata;
    ensure!(
        m.version == 1
            && m.kdf == "argon2id-v19"
            && m.memory_kib == MEMORY_KIB
            && m.iterations == ITERATIONS
            && m.lanes == LANES,
        "unsupported wallet format or KDF parameters"
    );
    Ok(wallet)
}

/// The identical v1 encrypted container, usable by native and browser wallets.
/// Returns encrypted bytes only; no unencrypted key leaves this library.
pub fn create_encrypted(password: &str) -> Result<String> {
    ensure!(
        password.chars().count() >= 12,
        "use a password of at least 12 characters"
    );
    let secret = SecretKey::generate()?;
    let salt: [u8; 16] = random_bytes()?;
    let nonce: [u8; 24] = random_bytes()?;
    let metadata = Metadata {
        version: 1,
        address: secret.address(),
        kdf: "argon2id-v19".into(),
        memory_kib: MEMORY_KIB,
        iterations: ITERATIONS,
        lanes: LANES,
        salt: hex::encode(salt),
        nonce: hex::encode(nonce),
    };
    let aad = serde_json::to_vec(&metadata)?;
    let encryption_key = derive(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(encryption_key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
    let plaintext = secret.expose_for_encryption();
    let ciphertext = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext.as_ref(),
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("wallet encryption failed"))?;
    let wallet = EncryptedWallet {
        metadata,
        ciphertext: hex::encode(ciphertext),
    };
    Ok(serde_json::to_string_pretty(&wallet)?)
}

pub fn create(path: &Path, password: &str) -> Result<Address> {
    let encrypted = create_encrypted(password)?;
    let address = address_from_json(&encrypted)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("create wallet (existing files are never overwritten)")?;
    file.write_all(encrypted.as_bytes())?;
    file.sync_all()?;
    Ok(address)
}

/// Public metadata only; tampering is detected when the wallet is unlocked.
pub fn address(path: &Path) -> Result<Address> {
    Ok(read_wallet(path)?.metadata.address)
}

pub fn address_from_json(encrypted: &str) -> Result<Address> {
    Ok(parse_wallet(encrypted.as_bytes())?.metadata.address)
}

pub fn unlock(path: &Path, password: &str) -> Result<SecretKey> {
    let wallet = read_wallet(path)?;
    decrypt_wallet(wallet, password)
}

pub fn unlock_json(encrypted: &str, password: &str) -> Result<SecretKey> {
    decrypt_wallet(parse_wallet(encrypted.as_bytes())?, password)
}

fn decrypt_wallet(wallet: EncryptedWallet, password: &str) -> Result<SecretKey> {
    let m = &wallet.metadata;
    let salt: [u8; 16] = decode(&m.salt)?;
    let nonce: [u8; 24] = decode(&m.nonce)?;
    let ciphertext: [u8; 48] = decode(&wallet.ciphertext)?;
    let key = derive(password, &salt)?;
    let cipher = XChaCha20Poly1305::new_from_slice(key.as_ref())
        .map_err(|_| anyhow::anyhow!("invalid encryption key"))?;
    let aad = serde_json::to_vec(m)?;
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow::anyhow!("wrong password or tampered wallet"))?,
    );
    let bytes = Zeroizing::new(
        <[u8; 32]>::try_from(plaintext.as_slice()).context("invalid decrypted key")?,
    );
    let secret = SecretKey::from_bytes(&bytes);
    if secret.address() != m.address {
        bail!("wallet public address does not match key");
    }
    Ok(secret)
}

pub fn sign_transaction(key: &SecretKey, transaction: Transaction) -> Result<SignedTransaction> {
    let chain = transaction.chain_id;
    let signed = transaction.sign(key)?;
    signed.verify(chain)?;
    Ok(signed)
}

pub fn sign_block(key: &SecretKey, genesis: &Genesis, block: Block) -> Result<Block> {
    genesis.validate()?;
    ensure!(
        key.address() == genesis.authority,
        "wallet is not the test authority"
    );
    ensure!(
        block.header.chain_id == genesis.chain_id(),
        "block belongs to another network"
    );
    ensure!(block.header.version == VERSION, "unsupported block version");
    ensure!(
        block.header.transactions_hash == transaction_commitment(&block.transactions)?,
        "block body does not match header"
    );
    for tx in &block.transactions {
        tx.verify(genesis.chain_id())?;
    }
    Ok(block.sign(key))
}
