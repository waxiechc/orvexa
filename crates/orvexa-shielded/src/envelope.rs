//! Canonical ORVEXA v2 envelope: one authorized Ironwood v3 bundle plus the
//! public fields the ledger needs. Fixed-width little-endian integers, upstream
//! canonical point/field encodings, implied proof length, no trailing bytes.
//! Exactly one byte string decodes to a given envelope.

use nonempty::NonEmpty;
use orchard::{
    bundle::{Authorization, Authorized, BundleVersion, Flags, TxVersion},
    note::{ExtractedNoteCommitment, Nullifier, TransmittedNoteCiphertext},
    note_encryption::ENC_CIPHERTEXT_SIZE,
    primitives::redpallas::{Signature, SpendAuth, VerificationKey},
    value::ValueCommitment,
    Action, Anchor, Bundle, Proof,
};
use sha2::{Digest, Sha256};
use subtle::CtOption;
use zcash_note_encryption::note_bytes::NoteBytesData;

pub const MAGIC: [u8; 4] = *b"OVX2";
pub const VERSION: u8 = 2;
/// Suite 1: `BundleVersion::ironwood_v3()`, `OrchardCircuitVersion::PostNu6_3`,
/// bundle commitment with `TxVersion::V6`.
pub const SUITE_IRONWOOD_V3: u8 = 1;
/// Upper bound on actions per envelope (resource limit, not a protocol constant).
pub const MAX_ACTIONS: usize = 16;

pub const SIGHASH_DOMAIN: &[u8] = b"ORVEXA/shielded-sighash/v2";
pub const TXID_DOMAIN: &[u8] = b"ORVEXA/shielded-txid/v2";

/// cv_net, nf, rk, cmx, epk, enc_ciphertext, out_ciphertext, spend_auth_sig.
pub const ACTION_BYTES: usize = 32 * 5 + ENC_CIPHERTEXT_SIZE + 80 + 64;
/// magic, version, suite, chain_id, fee, expiry_height, n_actions.
const HEADER_BYTES: usize = 4 + 1 + 1 + 32 + 8 + 8 + 1;
/// flags, value_balance, anchor, binding_sig (proof is in between, implied length).
const TRAILER_FIXED_BYTES: usize = 1 + 8 + 32 + 64;

pub fn bundle_version() -> BundleVersion {
    BundleVersion::ironwood_v3()
}

/// Exact encoded size of an envelope with `n` actions.
pub fn encoded_len(n: usize) -> usize {
    HEADER_BYTES + n * ACTION_BYTES + TRAILER_FIXED_BYTES + Proof::expected_proof_size(n)
}

#[derive(Clone, Debug)]
pub struct Envelope {
    pub chain_id: [u8; 32],
    pub fee: u64,
    pub expiry_height: u64,
    pub bundle: Bundle<Authorized, i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    Truncated,
    TrailingBytes,
    BadMagic,
    UnsupportedVersion,
    UnsupportedSuite,
    ActionCount,
    NonCanonicalPoint(&'static str),
    InvalidAction,
    UnrepresentableFlags,
    NonCanonicalProof,
    Oversized,
}

/// Signing digest. Covers every effect of the bundle (via the upstream v6
/// effects commitment) plus the ORVEXA context: chain, fee, expiry, suite,
/// action count, flags and anchor. Proof and signatures are excluded to avoid
/// a cycle; the tx ID covers them.
pub fn sighash<A: Authorization>(
    chain_id: &[u8; 32],
    fee: u64,
    expiry_height: u64,
    bundle: &Bundle<A, i64>,
) -> [u8; 32] {
    let effects: [u8; 32] = bundle
        .commitment(TxVersion::V6)
        .expect("ironwood_v3 bundles commit under TxVersion::V6")
        .into();
    let flags = bundle
        .flags()
        .to_byte(bundle_version())
        .expect("flags were validated for ironwood_v3");
    let mut h = Sha256::new();
    h.update(SIGHASH_DOMAIN);
    h.update([VERSION, SUITE_IRONWOOD_V3]);
    h.update(chain_id);
    h.update(fee.to_le_bytes());
    h.update(expiry_height.to_le_bytes());
    h.update([bundle.actions().len() as u8, flags]);
    h.update(bundle.anchor().to_bytes());
    h.update(effects);
    h.finalize().into()
}

impl Envelope {
    pub fn sighash(&self) -> [u8; 32] {
        sighash(&self.chain_id, self.fee, self.expiry_height, &self.bundle)
    }

    /// Commits to every byte of the authorized envelope.
    pub fn txid(&self) -> [u8; 32] {
        txid_of(&self.encode())
    }

    pub fn nullifiers(&self) -> impl Iterator<Item = [u8; 32]> + '_ {
        self.bundle
            .actions()
            .iter()
            .map(|a| a.nullifier().to_bytes())
    }

    pub fn encode(&self) -> Vec<u8> {
        let b = &self.bundle;
        let n = b.actions().len();
        let mut out = Vec::with_capacity(encoded_len(n));
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&[VERSION, SUITE_IRONWOOD_V3]);
        out.extend_from_slice(&self.chain_id);
        out.extend_from_slice(&self.fee.to_le_bytes());
        out.extend_from_slice(&self.expiry_height.to_le_bytes());
        out.push(n as u8);
        for action in b.actions() {
            out.extend_from_slice(&action.cv_net().to_bytes());
            out.extend_from_slice(&action.nullifier().to_bytes());
            out.extend_from_slice(&<[u8; 32]>::from(action.rk()));
            out.extend_from_slice(&action.cmx().to_bytes());
            let note = action.encrypted_note();
            out.extend_from_slice(&note.epk_bytes);
            out.extend_from_slice(note.enc_ciphertext.as_ref());
            out.extend_from_slice(&note.out_ciphertext);
            out.extend_from_slice(&<[u8; 64]>::from(action.authorization()));
        }
        out.push(
            b.flags()
                .to_byte(bundle_version())
                .expect("validated flags"),
        );
        out.extend_from_slice(&b.value_balance().to_le_bytes());
        out.extend_from_slice(&b.anchor().to_bytes());
        out.extend_from_slice(b.authorization().proof().as_ref());
        out.extend_from_slice(&<[u8; 64]>::from(b.authorization().binding_signature()));
        debug_assert_eq!(out.len(), encoded_len(n));
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        // Cheapest bound first: nothing larger than the biggest legal envelope.
        if bytes.len() > encoded_len(MAX_ACTIONS) {
            return Err(EnvelopeError::Oversized);
        }
        let mut r = Reader(bytes);
        if r.take::<4>()? != MAGIC {
            return Err(EnvelopeError::BadMagic);
        }
        if r.u8()? != VERSION {
            return Err(EnvelopeError::UnsupportedVersion);
        }
        if r.u8()? != SUITE_IRONWOOD_V3 {
            return Err(EnvelopeError::UnsupportedSuite);
        }
        let chain_id = r.take::<32>()?;
        let fee = u64::from_le_bytes(r.take::<8>()?);
        let expiry_height = u64::from_le_bytes(r.take::<8>()?);
        let n = r.u8()? as usize;
        if n == 0 || n > MAX_ACTIONS {
            return Err(EnvelopeError::ActionCount);
        }
        // Exact length is known now; reject before any point decoding.
        if bytes.len() < encoded_len(n) {
            return Err(EnvelopeError::Truncated);
        }
        if bytes.len() > encoded_len(n) {
            return Err(EnvelopeError::TrailingBytes);
        }

        let mut actions = Vec::with_capacity(n);
        for _ in 0..n {
            let cv_net = point(ValueCommitment::from_bytes(&r.take::<32>()?), "cv_net")?;
            let nf = point(Nullifier::from_bytes(&r.take::<32>()?), "nullifier")?;
            let rk = VerificationKey::<SpendAuth>::try_from(r.take::<32>()?)
                .map_err(|_| EnvelopeError::NonCanonicalPoint("rk"))?;
            let cmx = point(ExtractedNoteCommitment::from_bytes(&r.take::<32>()?), "cmx")?;
            let epk_bytes = r.take::<32>()?;
            let enc_ciphertext = NoteBytesData(r.take::<ENC_CIPHERTEXT_SIZE>()?);
            let out_ciphertext = r.take::<80>()?;
            let sig = Signature::<SpendAuth>::from(r.take::<64>()?);
            let note = TransmittedNoteCiphertext {
                epk_bytes,
                enc_ciphertext,
                out_ciphertext,
            };
            let action = Action::from_parts(nf, rk, cmx, note, cv_net, sig)
                .map_err(|_| EnvelopeError::InvalidAction)?;
            actions.push(action);
        }
        let flags = Flags::from_byte(r.u8()?, bundle_version())
            .ok_or(EnvelopeError::UnrepresentableFlags)?;
        let value_balance = i64::from_le_bytes(r.take::<8>()?);
        let anchor = point(Anchor::from_bytes(r.take::<32>()?), "anchor")?;
        let proof = Proof::new(r.slice(Proof::expected_proof_size(n))?.to_vec());
        let binding = Signature::from(r.take::<64>()?);
        debug_assert!(r.0.is_empty());

        let actions = NonEmpty::from_vec(actions).expect("n >= 1");
        let bundle = Bundle::try_from_parts(
            actions,
            flags,
            value_balance,
            anchor,
            Authorized::from_parts(proof, binding),
            bundle_version(),
        )
        .map_err(|_| EnvelopeError::NonCanonicalProof)?;
        Ok(Envelope {
            chain_id,
            fee,
            expiry_height,
            bundle,
        })
    }
}

pub fn txid_of(encoded: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(TXID_DOMAIN);
    h.update(encoded);
    h.finalize().into()
}

fn point<T>(value: CtOption<T>, field: &'static str) -> Result<T, EnvelopeError> {
    Option::<T>::from(value).ok_or(EnvelopeError::NonCanonicalPoint(field))
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn slice(&mut self, len: usize) -> Result<&'a [u8], EnvelopeError> {
        if self.0.len() < len {
            return Err(EnvelopeError::Truncated);
        }
        let (head, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(head)
    }
    fn take<const N: usize>(&mut self) -> Result<[u8; N], EnvelopeError> {
        Ok(self.slice(N)?.try_into().expect("length checked"))
    }
    fn u8(&mut self) -> Result<u8, EnvelopeError> {
        Ok(self.take::<1>()?[0])
    }
}
