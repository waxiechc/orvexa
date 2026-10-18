//! Minimal wallet over the prototype ledger: trial-decrypts outputs, keeps
//! notes with their positions, builds transfers with real proofs. Also a raw
//! builder so tests can produce well-formed bundles that break ledger rules.

use crate::envelope::{self, Envelope};
use crate::keys;
use crate::ledger::{self, Ledger, Params};
use orchard::{
    builder::{Builder, BundleType},
    bundle::Flags,
    keys::{FullViewingKey, OutgoingViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey, SpendingKey},
    note_encryption::IronwoodDomain,
    tree::MerklePath,
    value::NoteValue,
    Address, Anchor, Note,
};
use zcash_note_encryption::try_note_decryption;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalletError {
    ZeroAmount,
    FeeBelowMinimum,
    Overflow,
    InsufficientFunds,
    MissingWitness,
    /// A watch-only wallet cannot spend.
    ViewOnly,
    /// A public payment must match the consent the user gave.
    ConsentMismatch,
    Builder(String),
}

#[derive(Clone, Debug)]
pub struct OwnedNote {
    pub note: Note,
    pub position: u64,
    pub nullifier: [u8; 32],
    pub spent: bool,
}

pub struct Wallet {
    sk: SpendingKey,
    fvk: FullViewingKey,
    ivk: PreparedIncomingViewingKey,
    pub address: Address,
    pub notes: Vec<OwnedNote>,
}

/// Spend inputs, outputs and context for [`build_envelope`].
pub struct RawTx<'a> {
    pub chain_id: [u8; 32],
    pub fee: u64,
    pub expiry_height: u64,
    pub anchor: Anchor,
    pub flags: Flags,
    pub bundle_type: BundleType,
    pub spends: Vec<(FullViewingKey, Note, MerklePath)>,
    pub outputs: Vec<(Option<OutgoingViewingKey>, Address, u64)>,
    pub keys: &'a [SpendAuthorizingKey],
}

/// Builds, proves and signs one envelope. Value balance follows from
/// spends - outputs; the builder does not apply ORVEXA rules, the ledger does.
pub fn build_envelope(tx: RawTx<'_>) -> Result<Envelope, WalletError> {
    let mut builder = Builder::new(tx.bundle_type, envelope::bundle_version(), tx.flags, tx.anchor)
        .map_err(|e| WalletError::Builder(format!("{e:?}")))?;
    for (fvk, note, path) in tx.spends {
        builder.add_spend(fvk, note, path).map_err(|e| WalletError::Builder(format!("{e:?}")))?;
    }
    for (ovk, to, value) in tx.outputs {
        builder
            .add_output(ovk, to, NoteValue::from_raw(value), [0u8; 512])
            .map_err(|e| WalletError::Builder(format!("{e:?}")))?;
    }
    let (unauthorized, _) = builder
        .build::<i64>(&mut keys::os_rng())
        .map_err(|e| WalletError::Builder(format!("{e:?}")))?
        .ok_or_else(|| WalletError::Builder("empty bundle".into()))?;
    let sighash = envelope::sighash(&tx.chain_id, tx.fee, tx.expiry_height, &unauthorized);
    let proven = unauthorized
        .create_proof(keys::proving_key(), &mut keys::os_rng())
        .map_err(|e| WalletError::Builder(format!("{e:?}")))?;
    let bundle = proven
        .apply_signatures(keys::os_rng(), sighash, tx.keys)
        .map_err(|e| WalletError::Builder(format!("{e:?}")))?;
    Ok(Envelope { chain_id: tx.chain_id, fee: tx.fee, expiry_height: tx.expiry_height, bundle })
}

/// Genesis: one spends-disabled bundle minting exactly `params.supply` to the
/// given recipients, signed under the chain ID derived from its effects.
pub fn build_genesis(params: &Params, recipients: &[(Address, u64)]) -> Vec<u8> {
    let mut builder =
        Builder::new(BundleType::Coinbase, envelope::bundle_version(), Flags::SPENDS_DISABLED, ledger::empty_anchor())
            .expect("coinbase flags are valid for ironwood_v3");
    for (to, value) in recipients {
        builder.add_output(None, *to, NoteValue::from_raw(*value), [0u8; 512]).expect("outputs enabled");
    }
    let (unauthorized, _) = builder.build::<i64>(&mut keys::os_rng()).unwrap().unwrap();
    let chain_id = ledger::chain_id(params, &unauthorized);
    let sighash = envelope::sighash(&chain_id, 0, 0, &unauthorized);
    let proven = unauthorized.create_proof(keys::proving_key(), &mut keys::os_rng()).unwrap();
    let bundle = proven.apply_signatures(keys::os_rng(), sighash, &[]).unwrap();
    Envelope { chain_id, fee: 0, expiry_height: 0, bundle }.encode()
}

impl Wallet {
    pub fn from_seed(seed: u8) -> Self {
        let sk = SpendingKey::from_bytes([seed; 32]).unwrap();
        let fvk = FullViewingKey::from(&sk);
        let ivk = PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External));
        let address = fvk.address_at(0u32, Scope::External);
        Wallet { sk, fvk, ivk, address, notes: Vec::new() }
    }

    pub fn fvk(&self) -> &FullViewingKey {
        &self.fvk
    }
    pub fn spend_key(&self) -> SpendAuthorizingKey {
        (&self.sk).into()
    }
    pub fn ovk(&self) -> OutgoingViewingKey {
        self.fvk.to_ovk(Scope::External)
    }

    /// Scans transactions in ledger order; `first_leaf` is the ledger's leaf
    /// count before them. Marks own notes spent when their nullifier appears.
    pub fn scan(&mut self, first_leaf: u64, txs: &[Envelope]) {
        let mut position = first_leaf;
        for env in txs {
            for action in env.bundle.actions() {
                let nf = action.nullifier().to_bytes();
                for owned in &mut self.notes {
                    if owned.nullifier == nf {
                        owned.spent = true;
                    }
                }
                if let Some((note, _, _)) =
                    try_note_decryption(&IronwoodDomain::for_action(action), &self.ivk, action)
                {
                    let nullifier = note.nullifier(&self.fvk).to_bytes();
                    self.notes.push(OwnedNote { note, position, nullifier, spent: false });
                }
                position += 1;
            }
        }
    }

    /// Confirmed balance from own, unspent, non-zero notes.
    pub fn balance(&self) -> u64 {
        self.notes.iter().filter(|n| !n.spent).map(|n| n.note.value().inner()).sum()
    }

    /// Ordinary transfer with change back to this wallet, anchored at the tip.
    pub fn transfer(
        &self,
        ledger: &Ledger,
        to: Address,
        amount: u64,
        fee: u64,
        expiry_delta: u64,
    ) -> Result<Envelope, WalletError> {
        if amount == 0 {
            return Err(WalletError::ZeroAmount);
        }
        if fee == 0 {
            return Err(WalletError::FeeBelowMinimum);
        }
        let need = amount.checked_add(fee).ok_or(WalletError::Overflow)?;
        if need > ledger.remaining_supply() {
            return Err(WalletError::InsufficientFunds);
        }
        let mut picked = Vec::new();
        let mut total: u64 = 0;
        for owned in self.notes.iter().filter(|n| !n.spent && n.note.value().inner() > 0) {
            if total >= need {
                break;
            }
            let path = ledger.witness(owned.position).ok_or(WalletError::MissingWitness)?;
            total = total.checked_add(owned.note.value().inner()).ok_or(WalletError::Overflow)?;
            picked.push((self.fvk.clone(), owned.note, path));
        }
        if total < need {
            return Err(WalletError::InsufficientFunds);
        }
        let mut outputs = vec![(Some(self.ovk()), to, amount)];
        if total > need {
            outputs.push((Some(self.ovk()), self.address, total - need));
        }
        build_envelope(RawTx {
            chain_id: ledger.chain_id(),
            fee,
            expiry_height: ledger.height() + expiry_delta,
            anchor: ledger.anchor(),
            flags: envelope::bundle_version().default_flags(),
            bundle_type: BundleType::DEFAULT,
            spends: picked,
            outputs,
            keys: &[self.spend_key()],
        })
    }
}
