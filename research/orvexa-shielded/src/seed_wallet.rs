//! Seed-phrase wallet for v2 (plan stage 5).
//!
//! 24-word BIP39 English mnemonic from OS entropy → 64-byte seed (BIP39 PBKDF2,
//! empty passphrase) → ZIP 32 Orchard spending key (`SpendingKey::from_zip32_seed`,
//! coin type [`COIN_TYPE`], account 0). Everything else is derived from that key.
//!
//! The wallet keeps its own commitment tree: it appends every commitment it
//! scans, marks only its own notes, and gets witnesses and the anchor from
//! that tree, never from a node. Restoring from the phrase and rescanning the
//! chain rebuilds notes, change, spent state and balance.

use crate::envelope::{self, Envelope};
use crate::keys;
use crate::wallet::{build_envelope, RawTx, WalletError};
use bip0039::{English, Mnemonic};
use incrementalmerkletree::{Position, Retention};
use orchard::{
    builder::BundleType,
    keys::{FullViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey, SpendingKey},
    note_encryption::IronwoodDomain,
    tree::{MerkleHashOrchard, MerklePath},
    Address, Anchor, Note,
};
use rand::rand_core::Rng as _;
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::collections::HashMap;
use std::fmt;
use zcash_note_encryption::try_note_decryption;
use zeroize::Zeroizing;

/// SLIP-44 "Testnet (all coins)". Provisional: a dedicated ORVEXA coin type
/// must be chosen before any network that is not a test network.
pub const COIN_TYPE: u32 = 1;
pub const WORDS: usize = 24;
/// Witnesses are kept for this many recent blocks (matches the anchor window).
const CHECKPOINTS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedError {
    /// Not a valid 24-word English BIP39 phrase (word or checksum).
    InvalidPhrase,
    Derivation,
    /// Blocks must be scanned in order, each exactly once.
    OutOfOrder { expected: u64, got: u64 },
}

#[derive(Clone, Debug)]
pub struct SeedNote {
    pub note: Note,
    pub position: u64,
    pub nullifier: [u8; 32],
    pub scope: Scope,
    pub height: u64,
    pub spent_at: Option<u64>,
}

pub struct SeedWallet {
    entropy: Zeroizing<Vec<u8>>,
    account: u32,
    sk: SpendingKey,
    fvk: FullViewingKey,
    external: PreparedIncomingViewingKey,
    internal: PreparedIncomingViewingKey,
    tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16>,
    leaves: u64,
    next_height: u64,
    next_diversifier: u32,
    pub notes: Vec<SeedNote>,
    /// Nullifiers of notes used by a not-yet-confirmed transfer → its expiry height.
    pending: HashMap<[u8; 32], u64>,
}

/// Never prints the phrase, entropy or keys.
impl fmt::Debug for SeedWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeedWallet")
            .field("account", &self.account)
            .field("notes", &self.notes.len())
            .field("scanned_to", &self.next_height)
            .finish_non_exhaustive()
    }
}

impl SeedWallet {
    /// New wallet from 256 bits of operating-system entropy.
    pub fn generate() -> Self {
        let mut entropy = Zeroizing::new(vec![0u8; 32]);
        keys::os_rng().fill_bytes(&mut entropy);
        Self::from_entropy(&entropy, 0).expect("32 bytes is a valid BIP39 entropy length")
    }

    /// Restores from a written-down phrase. Whitespace is normalized; case is not.
    pub fn from_phrase(phrase: &str, account: u32) -> Result<Self, SeedError> {
        let normalized = Zeroizing::new(phrase.split_whitespace().collect::<Vec<_>>().join(" "));
        let mnemonic = Mnemonic::<English>::from_phrase(normalized.as_str()).map_err(|_| SeedError::InvalidPhrase)?;
        if mnemonic.phrase().split(' ').count() != WORDS {
            return Err(SeedError::InvalidPhrase);
        }
        Self::from_entropy(mnemonic.entropy(), account)
    }

    pub(crate) fn from_entropy(entropy: &[u8], account: u32) -> Result<Self, SeedError> {
        let mnemonic = Mnemonic::<English>::from_entropy(entropy.to_vec()).map_err(|_| SeedError::InvalidPhrase)?;
        if mnemonic.phrase().split(' ').count() != WORDS {
            return Err(SeedError::InvalidPhrase);
        }
        let seed = Zeroizing::new(mnemonic.to_seed(""));
        let account_id = zip32::AccountId::try_from(account).map_err(|_| SeedError::Derivation)?;
        let sk = SpendingKey::from_zip32_seed(seed.as_ref(), COIN_TYPE, account_id).map_err(|_| SeedError::Derivation)?;
        let fvk = FullViewingKey::from(&sk);
        Ok(SeedWallet {
            entropy: Zeroizing::new(entropy.to_vec()),
            account,
            external: PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External)),
            internal: PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::Internal)),
            sk,
            fvk,
            tree: ShardTree::new(MemoryShardStore::empty(), CHECKPOINTS),
            leaves: 0,
            next_height: 0,
            next_diversifier: 1,
            notes: Vec::new(),
            pending: HashMap::new(),
        })
    }

    /// The 24 words, for the backup screen only.
    pub fn phrase(&self) -> Zeroizing<String> {
        let mnemonic = Mnemonic::<English>::from_entropy(self.entropy.to_vec()).expect("valid entropy");
        Zeroizing::new(mnemonic.phrase().to_owned())
    }

    /// Checks words the user typed back at the given 0-based positions.
    pub fn verify_words(&self, picks: &[(usize, &str)]) -> bool {
        let phrase = self.phrase();
        let words: Vec<&str> = phrase.split(' ').collect();
        !picks.is_empty() && picks.iter().all(|(i, w)| words.get(*i) == Some(&w.trim()))
    }

    pub(crate) fn entropy(&self) -> &[u8] {
        &self.entropy
    }
    pub fn account(&self) -> u32 {
        self.account
    }
    pub fn fvk(&self) -> &FullViewingKey {
        &self.fvk
    }

    /// Default receiving address (diversifier index 0).
    pub fn address(&self) -> Address {
        self.fvk.address_at(0u32, Scope::External)
    }

    /// A fresh unlinkable address; all of them are found by the same scan.
    pub fn new_address(&mut self) -> Address {
        let address = self.fvk.address_at(self.next_diversifier, Scope::External);
        self.next_diversifier += 1;
        address
    }

    /// Next block height this wallet expects to scan.
    pub fn scanned_to(&self) -> u64 {
        self.next_height
    }

    /// Scans one block (genesis is height 0) in chain order.
    pub fn scan_block(&mut self, height: u64, txs: &[Envelope]) -> Result<(), SeedError> {
        if height != self.next_height {
            return Err(SeedError::OutOfOrder { expected: self.next_height, got: height });
        }
        for env in txs {
            for action in env.bundle.actions() {
                let nf = action.nullifier().to_bytes();
                for owned in self.notes.iter_mut().filter(|n| n.nullifier == nf) {
                    owned.spent_at = Some(height);
                }
                self.pending.remove(&nf);

                let domain = IronwoodDomain::for_action(action);
                let found = try_note_decryption(&domain, &self.external, action)
                    .map(|(note, _, _)| (note, Scope::External))
                    .or_else(|| try_note_decryption(&domain, &self.internal, action).map(|(note, _, _)| (note, Scope::Internal)));
                let leaf = MerkleHashOrchard::from_cmx(action.cmx());
                match found {
                    Some((note, scope)) => {
                        self.tree.append(leaf, Retention::Marked).expect("tree capacity");
                        self.notes.push(SeedNote {
                            nullifier: note.nullifier(&self.fvk).to_bytes(),
                            note,
                            position: self.leaves,
                            scope,
                            height,
                            spent_at: None,
                        });
                    }
                    None => self.tree.append(leaf, Retention::Ephemeral).expect("tree capacity"),
                }
                self.leaves += 1;
            }
        }
        self.tree.checkpoint(height as u32).expect("heights increase");
        // A transfer that can no longer be mined frees its notes again.
        self.pending.retain(|_, expiry| *expiry > height);
        self.next_height = height + 1;
        Ok(())
    }

    fn spendable(&self) -> impl Iterator<Item = &SeedNote> {
        self.notes
            .iter()
            .filter(|n| n.spent_at.is_none() && !self.pending.contains_key(&n.nullifier) && n.note.value().inner() > 0)
    }

    /// Confirmed and not reserved by a pending transfer.
    pub fn balance(&self) -> u64 {
        self.spendable().map(|n| n.note.value().inner()).sum()
    }

    /// Value of notes locked by transfers sent but not yet confirmed.
    pub fn pending(&self) -> u64 {
        self.notes
            .iter()
            .filter(|n| n.spent_at.is_none() && self.pending.contains_key(&n.nullifier))
            .map(|n| n.note.value().inner())
            .sum()
    }

    /// Spent nullifiers this wallet knows of, sorted.
    pub fn spent_nullifiers(&self) -> Vec<[u8; 32]> {
        let mut v: Vec<_> = self.notes.iter().filter(|n| n.spent_at.is_some()).map(|n| n.nullifier).collect();
        v.sort();
        v
    }

    /// Root of the wallet's own tree after the last scanned block.
    pub fn anchor(&self) -> Option<Anchor> {
        let checkpoint = u32::try_from(self.next_height.checked_sub(1)?).ok()?;
        self.tree.root_at_checkpoint_id(&checkpoint).ok().flatten().map(Anchor::from)
    }

    fn witness(&self, position: u64) -> Option<MerklePath> {
        let checkpoint = u32::try_from(self.next_height.checked_sub(1)?).ok()?;
        self.tree
            .witness_at_checkpoint_id(Position::from(position), &checkpoint)
            .ok()
            .flatten()
            .map(MerklePath::from)
    }

    /// Builds a transfer; change goes to the internal (change) address. The
    /// chosen notes stay reserved until the transfer is seen or expires.
    pub fn transfer(
        &mut self,
        chain_id: [u8; 32],
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
        let anchor = self.anchor().ok_or(WalletError::MissingWitness)?;
        let mut picked = Vec::new();
        let mut used = Vec::new();
        let mut total: u64 = 0;
        for owned in self.spendable() {
            if total >= need {
                break;
            }
            let path = self.witness(owned.position).ok_or(WalletError::MissingWitness)?;
            total = total.checked_add(owned.note.value().inner()).ok_or(WalletError::Overflow)?;
            picked.push((self.fvk.clone(), owned.note, path));
            used.push(owned.nullifier);
        }
        if total < need {
            return Err(WalletError::InsufficientFunds);
        }
        let ovk = self.fvk.to_ovk(Scope::External);
        let mut outputs = vec![(Some(ovk.clone()), to, amount)];
        if total > need {
            let change = self.fvk.address_at(0u32, Scope::Internal);
            outputs.push((Some(self.fvk.to_ovk(Scope::Internal)), change, total - need));
        }
        let height = self.next_height; // the block this transfer can first enter
        let key: SpendAuthorizingKey = (&self.sk).into();
        let env = build_envelope(RawTx {
            chain_id,
            fee,
            expiry_height: height + expiry_delta,
            anchor,
            flags: envelope::bundle_version().default_flags(),
            bundle_type: BundleType::DEFAULT,
            spends: picked,
            outputs,
            keys: &[key],
        })?;
        for nf in used {
            self.pending.insert(nf, env.expiry_height);
        }
        Ok(env)
    }
}
