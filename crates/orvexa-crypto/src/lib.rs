//! Reviewed primitives behind a small, explicitly transparent protocol boundary.
//! The legacy address prefix and hash domain stay fixed for v1 compatibility.
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fmt, str::FromStr};
use thiserror::Error;
use zeroize::Zeroizing;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("invalid or noncanonical hex encoding")]
    Encoding,
    #[error("invalid test address or checksum")]
    Address,
    #[error("invalid Ed25519 signature or public key")]
    Signature,
    #[error("operating-system randomness unavailable")]
    Random,
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut bytes = [0; N];
    getrandom::getrandom(&mut bytes).map_err(|_| CryptoError::Random)?;
    Ok(bytes)
}

pub fn decode<const N: usize>(value: &str) -> Result<[u8; N], CryptoError> {
    if value.len() != N * 2
        || value
            .bytes()
            .any(|c| !c.is_ascii_hexdigit() || c.is_ascii_uppercase())
    {
        return Err(CryptoError::Encoding);
    }
    let mut out = [0; N];
    hex::decode_to_slice(value, &mut out).map_err(|_| CryptoError::Encoding)?;
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Hash32(pub [u8; 32]);

impl Hash32 {
    pub const ZERO: Self = Self([0; 32]);
}

impl fmt::Display for Hash32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}
impl FromStr for Hash32 {
    type Err = CryptoError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(decode(s)?))
    }
}
impl TryFrom<String> for Hash32 {
    type Error = CryptoError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl From<Hash32> for String {
    fn from(value: Hash32) -> Self {
        value.to_string()
    }
}

pub fn hash(domain: &[u8], data: &[u8]) -> Hash32 {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(data);
    Hash32(hasher.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Address([u8; 32]);

impl Address {
    pub fn from_public_key(bytes: [u8; 32]) -> Result<Self, CryptoError> {
        let key = VerifyingKey::from_bytes(&bytes).map_err(|_| CryptoError::Address)?;
        if key.is_weak() {
            return Err(CryptoError::Address);
        }
        Ok(Self(bytes))
    }
    pub fn public_key(&self) -> &[u8; 32] {
        &self.0
    }
}
impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let checksum = hash(b"VOID/address/v1", &self.0);
        write!(
            f,
            "void_test_{}{}",
            hex::encode(self.0),
            hex::encode(&checksum.0[..4])
        )
    }
}
impl FromStr for Address {
    type Err = CryptoError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let value = s.strip_prefix("void_test_").ok_or(CryptoError::Address)?;
        if value.len() != 72 || !value.is_ascii() {
            return Err(CryptoError::Address);
        }
        let address = Self::from_public_key(decode(&value[..64])?)?;
        if address.to_string() != s {
            return Err(CryptoError::Address);
        }
        Ok(address)
    }
}
impl TryFrom<String> for Address {
    type Error = CryptoError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}
impl From<Address> for String {
    fn from(value: Address) -> Self {
        value.to_string()
    }
}

/// Secret key is neither serializable nor Debug-printable. SigningKey zeroizes on drop.
pub struct SecretKey(SigningKey);
impl SecretKey {
    pub fn generate() -> Result<Self, CryptoError> {
        let bytes = Zeroizing::new(random_bytes()?);
        Ok(Self(SigningKey::from_bytes(&bytes)))
    }
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self(SigningKey::from_bytes(bytes))
    }
    pub fn expose_for_encryption(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.0.to_bytes())
    }
    pub fn address(&self) -> Address {
        Address(self.0.verifying_key().to_bytes())
    }
    pub fn sign(&self, bytes: &[u8]) -> String {
        hex::encode(self.0.sign(bytes).to_bytes())
    }
}

pub fn verify(address: Address, message: &[u8], signature: &str) -> Result<(), CryptoError> {
    let signature = ed25519_dalek::Signature::from_bytes(&decode(signature)?);
    VerifyingKey::from_bytes(address.public_key())
        .map_err(|_| CryptoError::Signature)?
        .verify_strict(message, &signature)
        .map_err(|_| CryptoError::Signature)
}

pub mod privacy {
    /// Future private ledger integration; v1 has no implementation of this trait.
    /// Implementations must specify canonical encodings and independently reviewed proofs.
    pub trait PrivacyBackend {
        type Transaction;
        type State;
        type Error;
        fn verify(
            &self,
            transaction: &Self::Transaction,
            state: &Self::State,
        ) -> Result<(), Self::Error>;
    }
    #[derive(Debug, PartialEq, Eq)]
    pub enum Capability {
        Transparent,
    }
    pub fn capability() -> Capability {
        Capability::Transparent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn address_and_strict_signature() {
        let key = SecretKey::from_bytes(&[7; 32]);
        let address = key.address();
        assert_eq!(address.to_string().parse::<Address>().unwrap(), address);
        let signature = key.sign(b"ORVEXA test");
        verify(address, b"ORVEXA test", &signature).unwrap();
        assert!(verify(address, b"tampered", &signature).is_err());
        assert!(verify(address, b"ORVEXA test", &"00".repeat(64)).is_err());
        assert!(
            address
                .to_string()
                .to_uppercase()
                .parse::<Address>()
                .is_err()
        );
        assert!(Address::from_public_key([0; 32]).is_err());
        let mut bad = address.to_string();
        bad.pop();
        bad.push('g');
        assert!(bad.parse::<Address>().is_err());
        assert!("void_test_😀".parse::<Address>().is_err());
    }
    #[test]
    fn hash_is_domain_separated() {
        assert_ne!(hash(b"tx", b"abc"), hash(b"block", b"abc"));
        assert_eq!(
            hash(b"", b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(privacy::capability(), privacy::Capability::Transparent);
    }
}
