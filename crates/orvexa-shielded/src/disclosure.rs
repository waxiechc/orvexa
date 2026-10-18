//! Privacy controls (plan stage 6): one-payment receipts, view-only keys and
//! the explicit consent a public payment needs.
//!
//! A receipt opens exactly one output note: recipient address, value and
//! rseed. A verifier recomputes the note commitment with `rho` taken from the
//! on-chain action and compares it with that action's `cmx`; the commitment is
//! binding, so neither the amount nor the recipient can be misstated. A
//! receipt does not reveal or prove who sent the payment, any other output,
//! change, balances or keys. Once shared it cannot be taken back.

use crate::envelope::Envelope;
use crate::ledger::Ledger;
use orchard::{
    keys::{FullViewingKey, IncomingViewingKey},
    note::{ExtractedNoteCommitment, RandomSeed},
    value::NoteValue,
    Address, Note, NoteVersion,
};
use sha2::{Digest, Sha256};

pub const RECEIPT_PREFIX: &str = "orvexa-receipt-v2:";
pub const FULL_KEY_PREFIX: &str = "orvexa-fvk-v2:";
pub const INCOMING_KEY_PREFIX: &str = "orvexa-ivk-v2:";
const RECEIPT_MAGIC: [u8; 4] = *b"OVXR";
const RECEIPT_LEN: usize = 4 + 1 + 32 + 32 + 1 + 43 + 8 + 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisclosureError {
    Encoding,
    Checksum,
    WrongChain,
    TxNotInChain,
    TxMismatch,
    NoSuchAction,
    /// The opening does not match the on-chain commitment.
    CommitmentMismatch,
}

/// Opening of one output note in one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentReceipt {
    pub chain_id: [u8; 32],
    pub txid: [u8; 32],
    pub action: u8,
    pub recipient: [u8; 43],
    pub value: u64,
    pub rseed: [u8; 32],
}

/// What a verifier learns from a valid receipt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedPayment {
    pub recipient: Address,
    pub value: u64,
    pub height: u64,
    pub confirmations: u64,
}

impl PaymentReceipt {
    pub fn for_note(chain_id: [u8; 32], txid: [u8; 32], action: u8, note: &Note) -> Self {
        PaymentReceipt {
            chain_id,
            txid,
            action,
            recipient: note.recipient().to_raw_address_bytes(),
            value: note.value().inner(),
            rseed: *note.rseed().as_bytes(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(RECEIPT_LEN);
        out.extend_from_slice(&RECEIPT_MAGIC);
        out.push(1);
        out.extend_from_slice(&self.chain_id);
        out.extend_from_slice(&self.txid);
        out.push(self.action);
        out.extend_from_slice(&self.recipient);
        out.extend_from_slice(&self.value.to_le_bytes());
        out.extend_from_slice(&self.rseed);
        out
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self, DisclosureError> {
        if b.len() != RECEIPT_LEN || b[..4] != RECEIPT_MAGIC || b[4] != 1 {
            return Err(DisclosureError::Encoding);
        }
        Ok(PaymentReceipt {
            chain_id: b[5..37].try_into().unwrap(),
            txid: b[37..69].try_into().unwrap(),
            action: b[69],
            recipient: b[70..113].try_into().unwrap(),
            value: u64::from_le_bytes(b[113..121].try_into().unwrap()),
            rseed: b[121..153].try_into().unwrap(),
        })
    }

    /// Text form to copy or publish.
    pub fn encode(&self) -> String {
        with_checksum(RECEIPT_PREFIX, &self.to_bytes())
    }

    pub fn decode(text: &str) -> Result<Self, DisclosureError> {
        Self::from_bytes(&without_checksum(RECEIPT_PREFIX, text)?)
    }

    /// Checks the receipt against the chain. `env` is the transaction as
    /// served by any node; it is bound by its txid, which must be in `ledger`.
    pub fn verify(
        &self,
        ledger: &Ledger,
        env: &Envelope,
    ) -> Result<VerifiedPayment, DisclosureError> {
        if self.chain_id != ledger.chain_id() || env.chain_id != self.chain_id {
            return Err(DisclosureError::WrongChain);
        }
        if env.txid() != self.txid {
            return Err(DisclosureError::TxMismatch);
        }
        let height = ledger
            .tx_height(&self.txid)
            .ok_or(DisclosureError::TxNotInChain)?;
        let action = env
            .bundle
            .actions()
            .iter()
            .nth(self.action as usize)
            .ok_or(DisclosureError::NoSuchAction)?;
        let recipient = Option::from(Address::from_raw_address_bytes(&self.recipient))
            .ok_or(DisclosureError::Encoding)?;
        let rho = action.rho();
        let rseed = Option::from(RandomSeed::from_bytes(self.rseed, &rho))
            .ok_or(DisclosureError::CommitmentMismatch)?;
        let note: Note = Option::from(Note::from_parts(
            recipient,
            NoteValue::from_raw(self.value),
            rho,
            rseed,
            NoteVersion::V3,
        ))
        .ok_or(DisclosureError::CommitmentMismatch)?;
        if ExtractedNoteCommitment::from(note.commitment()).to_bytes() != action.cmx().to_bytes() {
            return Err(DisclosureError::CommitmentMismatch);
        }
        Ok(VerifiedPayment {
            recipient,
            value: self.value,
            height,
            confirmations: ledger.height() - height + 1,
        })
    }
}

/// A key that lets someone else watch a wallet without spending from it.
#[derive(Clone, Debug)]
pub enum ViewingKey {
    /// Incoming and outgoing payments, change, spent state and balance.
    Full(FullViewingKey),
    /// Incoming payments to the external addresses only: no change, no
    /// spends, no outgoing history, therefore no balance.
    Incoming(IncomingViewingKey),
}

impl ViewingKey {
    pub fn encode(&self) -> String {
        match self {
            ViewingKey::Full(k) => with_checksum(FULL_KEY_PREFIX, &k.to_bytes()),
            ViewingKey::Incoming(k) => with_checksum(INCOMING_KEY_PREFIX, &k.to_bytes()),
        }
    }

    pub fn decode(text: &str) -> Result<Self, DisclosureError> {
        if text.starts_with(FULL_KEY_PREFIX) {
            let bytes: [u8; 96] = without_checksum(FULL_KEY_PREFIX, text)?
                .try_into()
                .map_err(|_| DisclosureError::Encoding)?;
            FullViewingKey::from_bytes(&bytes)
                .map(ViewingKey::Full)
                .ok_or(DisclosureError::Encoding)
        } else {
            let bytes: [u8; 64] = without_checksum(INCOMING_KEY_PREFIX, text)?
                .try_into()
                .map_err(|_| DisclosureError::Encoding)?;
            Option::from(IncomingViewingKey::from_bytes(&bytes))
                .map(ViewingKey::Incoming)
                .ok_or(DisclosureError::Encoding)
        }
    }

    /// Plain-language scope shown before the key is exported.
    pub fn scope_ru(&self) -> &'static [&'static str] {
        match self {
            ViewingKey::Full(_) => &[
                "все входящие платежи на любые адреса этого кошелька",
                "все исходящие платежи: получатели и суммы",
                "сдачу, потраченные средства и точный баланс",
                "всю прошлую и будущую историю; отозвать доступ нельзя",
            ],
            ViewingKey::Incoming(_) => &[
                "входящие платежи на адреса этого кошелька: суммы и время",
                "не показывает исходящие платежи, сдачу и баланс",
                "всю прошлую и будущую историю входящих; отозвать доступ нельзя",
            ],
        }
    }
}

/// Explicit consent for a public payment. Built only from what the user saw
/// on the confirmation screen; the wallet refuses a transfer that differs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicConsent {
    pub to: Address,
    pub amount: u64,
}

impl PublicConsent {
    /// What becomes public, for the confirmation screen.
    pub const REVEALS_RU: [&'static str; 3] = [
        "адрес получателя",
        "сумма перевода",
        "идентификатор транзакции",
    ];
    /// What stays private even in a public payment.
    pub const KEEPS_PRIVATE_RU: [&'static str; 3] =
        ["отправитель", "сдача и остаток", "остальные платежи"];
}

fn with_checksum(prefix: &str, bytes: &[u8]) -> String {
    let check = checksum(prefix, bytes);
    format!("{prefix}{}{}", hex::encode(bytes), hex::encode(check))
}

fn without_checksum(prefix: &str, text: &str) -> Result<Vec<u8>, DisclosureError> {
    let body = text
        .trim()
        .strip_prefix(prefix)
        .ok_or(DisclosureError::Encoding)?;
    let raw = hex::decode(body).map_err(|_| DisclosureError::Encoding)?;
    if raw.len() < 4 {
        return Err(DisclosureError::Encoding);
    }
    let (bytes, check) = raw.split_at(raw.len() - 4);
    if checksum(prefix, bytes) != check {
        return Err(DisclosureError::Checksum);
    }
    Ok(bytes.to_vec())
}

fn checksum(prefix: &str, bytes: &[u8]) -> [u8; 4] {
    let mut h = Sha256::new();
    h.update(prefix.as_bytes());
    h.update(bytes);
    h.finalize()[..4].try_into().unwrap()
}
