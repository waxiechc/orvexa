//! Seed-phrase wallet for v2 (plan stages 5 and 6).
//!
//! 24-word BIP39 English mnemonic from OS entropy → 64-byte seed (BIP39 PBKDF2,
//! empty passphrase) → ZIP 32 Orchard spending key (`SpendingKey::from_zip32_seed`,
//! coin type [`COIN_TYPE`], account N). Everything else is derived from that key.
//!
//! The wallet keeps its own commitment tree: it appends every commitment it
//! scans, marks only its own notes, and gets witnesses and the anchor from
//! that tree, never from a node. Restoring from the phrase and rescanning the
//! chain rebuilds notes, change, spent state, sent payments and balance.
//!
//! The same scanner runs with less authority: a full viewing key (sees
//! everything, cannot spend) or an incoming viewing key (sees only payments
//! received on external addresses).

use crate::disclosure::{PaymentReceipt, PublicConsent, ViewingKey};
use crate::envelope::{self, Envelope};
use crate::keys;
use crate::wallet::{build_envelope, RawTx, WalletError};
use bip0039::{English, Mnemonic};
use incrementalmerkletree::{Position, Retention};
use orchard::{
    builder::BundleType,
    keys::{FullViewingKey, IncomingViewingKey, PreparedIncomingViewingKey, Scope, SpendAuthorizingKey, SpendingKey},
    note_encryption::IronwoodDomain,
    tree::{MerkleHashOrchard, MerklePath},
    Address, Anchor, Note,
};
use rand::rand_core::Rng as _;
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::collections::HashMap;
use std::fmt;
use zcash_note_encryption::{try_note_decryption, try_output_recovery_with_ovk};
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

/// What this wallet instance is allowed to do and see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Spend,
    ViewFull,
    ViewIncoming,
}

#[derive(Clone, Debug)]
pub struct SeedNote {
    pub note: Note,
    pub position: u64,
    /// Unknown to an incoming-only key.
    pub nullifier: Option<[u8; 32]>,
    pub scope: Scope,
    pub height: u64,
    pub txid: [u8; 32],
    pub action: u8,
    pub spent_at: Option<u64>,
}

/// A payment this wallet made, recovered with its outgoing viewing key.
#[derive(Clone, Debug)]
pub struct SentPayment {
    pub note: Note,
    pub height: u64,
    pub txid: [u8; 32],
    pub action: u8,
}

pub struct SeedWallet {
    access: Access,
    entropy: Option<Zeroizing<Vec<u8>>>,
    account: u32,
    sk: Option<SpendingKey>,
    fvk: Option<FullViewingKey>,
    external_ivk: IncomingViewingKey,
    external: PreparedIncomingViewingKey,
    internal: Option<PreparedIncomingViewingKey>,
    tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16>,
    leaves: u64,
    next_height: u64,
    next_diversifier: u32,
    pub notes: Vec<SeedNote>,
    pub sent: Vec<SentPayment>,
    /// Nullifiers of notes used by a not-yet-confirmed transfer → its expiry height.
    pending: HashMap<[u8; 32], u64>,
}

/// Never prints the phrase, entropy or keys.
impl fmt::Debug for SeedWallet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SeedWallet")
            .field("access", &self.access)
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
        let mut wallet = Self::with_fvk(Access::Spend, FullViewingKey::from(&sk), account);
        wallet.sk = Some(sk);
        wallet.entropy = Some(Zeroizing::new(entropy.to_vec()));
        Ok(wallet)
    }

    /// A watch-only wallet from an exported viewing key. It can never spend.
    pub fn view_only(key: ViewingKey) -> Self {
        match key {
            ViewingKey::Full(fvk) => Self::with_fvk(Access::ViewFull, fvk, 0),
            ViewingKey::Incoming(ivk) => Self::empty(Access::ViewIncoming, ivk, 0),
        }
    }

    fn with_fvk(access: Access, fvk: FullViewingKey, account: u32) -> Self {
        let mut w = Self::empty(access, fvk.to_ivk(Scope::External), account);
        w.internal = Some(PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::Internal)));
        w.fvk = Some(fvk);
        w
    }

    fn empty(access: Access, external_ivk: IncomingViewingKey, account: u32) -> Self {
        SeedWallet {
            access,
            entropy: None,
            account,
            sk: None,
            fvk: None,
            external: PreparedIncomingViewingKey::new(&external_ivk),
            external_ivk,
            internal: None,
            tree: ShardTree::new(MemoryShardStore::empty(), CHECKPOINTS),
            leaves: 0,
            next_height: 0,
            next_diversifier: 1,
            notes: Vec::new(),
            sent: Vec::new(),
            pending: HashMap::new(),
        }
    }

    pub fn access(&self) -> Access {
        self.access
    }

    /// The 24 words, for the backup screen only. `None` for watch-only wallets.
    pub fn phrase(&self) -> Option<Zeroizing<String>> {
        let entropy = self.entropy.as_ref()?;
        let mnemonic = Mnemonic::<English>::from_entropy(entropy.to_vec()).expect("valid entropy");
        Some(Zeroizing::new(mnemonic.phrase().to_owned()))
    }

    /// Checks words the user typed back at the given 0-based positions.
    pub fn verify_words(&self, picks: &[(usize, &str)]) -> bool {
        let Some(phrase) = self.phrase() else { return false };
        let words: Vec<&str> = phrase.split(' ').collect();
        !picks.is_empty() && picks.iter().all(|(i, w)| words.get(*i) == Some(&w.trim()))
    }

    pub(crate) fn entropy(&self) -> Option<&[u8]> {
        self.entropy.as_deref().map(Vec::as_slice)
    }
    pub fn account(&self) -> u32 {
        self.account
    }
    pub fn fvk(&self) -> Option<&FullViewingKey> {
        self.fvk.as_ref()
    }

    /// Exports a key for someone else to watch this wallet. Show
    /// [`ViewingKey::scope_ru`] before handing it out.
    pub fn viewing_key(&self, full: bool) -> Option<ViewingKey> {
        match (full, &self.fvk) {
            (true, Some(fvk)) => Some(ViewingKey::Full(fvk.clone())),
            (true, None) => None,
            (false, _) => Some(ViewingKey::Incoming(self.external_ivk.clone())),
        }
    }

    /// Default receiving address (diversifier index 0).
    pub fn address(&self) -> Address {
        self.external_ivk.address_at(0u32)
    }

    /// A fresh unlinkable address; all of them are found by the same scan.
    pub fn new_address(&mut self) -> Address {
        let address = self.external_ivk.address_at(self.next_diversifier);
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
        let ovk = self.fvk.as_ref().map(|f| f.to_ovk(Scope::External));
        for env in txs {
            let txid = env.txid();
            for (index, action) in env.bundle.actions().iter().enumerate() {
                let nf = action.nullifier().to_bytes();
                for owned in self.notes.iter_mut().filter(|n| n.nullifier == Some(nf)) {
                    owned.spent_at = Some(height);
                }
                self.pending.remove(&nf);

                let domain = IronwoodDomain::for_action(action);
                let found = try_note_decryption(&domain, &self.external, action)
                    .map(|(note, _, _)| (note, Scope::External))
                    .or_else(|| {
                        let internal = self.internal.as_ref()?;
                        try_note_decryption(&domain, internal, action).map(|(note, _, _)| (note, Scope::Internal))
                    });
                let leaf = MerkleHashOrchard::from_cmx(action.cmx());
                match found {
                    Some((note, scope)) => {
                        self.tree.append(leaf, Retention::Marked).expect("tree capacity");
                        self.notes.push(SeedNote {
                            nullifier: self.fvk.as_ref().map(|f| note.nullifier(f).to_bytes()),
                            note,
                            position: self.leaves,
                            scope,
                            height,
                            txid,
                            action: index as u8,
                            spent_at: None,
                        });
                    }
                    None => self.tree.append(leaf, Retention::Ephemeral).expect("tree capacity"),
                }
                // Payments this wallet sent to others (its own outputs are notes above).
                if let (Some(ovk), Some(fvk)) = (&ovk, &self.fvk) {
                    let out = &action.encrypted_note().out_ciphertext;
                    if let Some((sent, _, _)) = try_output_recovery_with_ovk(&domain, ovk, action, action.cv_net(), out) {
                        if fvk.scope_for_address(&sent.recipient()).is_none() {
                            self.sent.push(SentPayment { note: sent, height, txid, action: index as u8 });
                        }
                    }
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
        self.notes.iter().filter(|n| {
            n.spent_at.is_none() && n.nullifier.is_some_and(|nf| !self.pending.contains_key(&nf)) && n.note.value().inner() > 0
        })
    }

    /// Confirmed balance not reserved by a pending transfer. `None` for an
    /// incoming-only key: it cannot see spends, so it cannot know a balance.
    pub fn balance(&self) -> Option<u64> {
        (self.access != Access::ViewIncoming).then(|| self.spendable().map(|n| n.note.value().inner()).sum())
    }

    /// Total received on external addresses: what any key of this wallet sees.
    pub fn received(&self) -> u64 {
        self.notes.iter().filter(|n| n.scope == Scope::External).map(|n| n.note.value().inner()).sum()
    }

    /// Value of notes locked by transfers sent but not yet confirmed.
    pub fn pending(&self) -> u64 {
        self.notes
            .iter()
            .filter(|n| n.spent_at.is_none() && n.nullifier.is_some_and(|nf| self.pending.contains_key(&nf)))
            .map(|n| n.note.value().inner())
            .sum()
    }

    /// Spent nullifiers this wallet knows of, sorted.
    pub fn spent_nullifiers(&self) -> Vec<[u8; 32]> {
        let mut v: Vec<_> = self.notes.iter().filter(|n| n.spent_at.is_some()).filter_map(|n| n.nullifier).collect();
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

    /// Private transfer. Change goes to the internal (change) address. The
    /// chosen notes stay reserved until the transfer is seen or expires.
    /// There is no fallback: on any error nothing is sent or disclosed.
    pub fn transfer(
        &mut self,
        chain_id: [u8; 32],
        to: Address,
        amount: u64,
        fee: u64,
        expiry_delta: u64,
    ) -> Result<Envelope, WalletError> {
        let (Some(sk), Some(fvk)) = (self.sk.as_ref(), self.fvk.clone()) else {
            return Err(WalletError::ViewOnly);
        };
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
            picked.push((fvk.clone(), owned.note, path));
            used.push(owned.nullifier.expect("spend access has nullifiers"));
        }
        if total < need {
            return Err(WalletError::InsufficientFunds);
        }
        let mut outputs = vec![(Some(fvk.to_ovk(Scope::External)), to, amount)];
        if total > need {
            let change = fvk.address_at(0u32, Scope::Internal);
            outputs.push((Some(fvk.to_ovk(Scope::Internal)), change, total - need));
        }
        let key: SpendAuthorizingKey = sk.into();
        let env = build_envelope(RawTx {
            chain_id,
            fee,
            expiry_height: self.next_height + expiry_delta,
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

    /// Public payment: the same private transfer plus a receipt meant to be
    /// published. Requires consent for exactly this recipient and amount.
    pub fn public_transfer(
        &mut self,
        chain_id: [u8; 32],
        consent: &PublicConsent,
        to: Address,
        amount: u64,
        fee: u64,
        expiry_delta: u64,
    ) -> Result<(Envelope, PaymentReceipt), WalletError> {
        if consent.to != to || consent.amount != amount {
            return Err(WalletError::ConsentMismatch);
        }
        let env = self.transfer(chain_id, to, amount, fee, expiry_delta)?;
        let receipt = self.receipt_for_output(chain_id, &env, to, amount).ok_or(WalletError::ConsentMismatch)?;
        Ok((env, receipt))
    }

    /// Receipt for one payment this wallet sent (found by OVK) or received.
    pub fn payment_receipt(&self, chain_id: [u8; 32], txid: [u8; 32], action: u8) -> Option<PaymentReceipt> {
        let sent = self.sent.iter().find(|p| p.txid == txid && p.action == action).map(|p| p.note);
        let received = self.notes.iter().find(|n| n.txid == txid && n.action == action).map(|n| n.note);
        sent.or(received).map(|note| PaymentReceipt::for_note(chain_id, txid, action, &note))
    }

    fn receipt_for_output(&self, chain_id: [u8; 32], env: &Envelope, to: Address, amount: u64) -> Option<PaymentReceipt> {
        let ovk = self.fvk.as_ref()?.to_ovk(Scope::External);
        let txid = env.txid();
        env.bundle.actions().iter().enumerate().find_map(|(i, action)| {
            let domain = IronwoodDomain::for_action(action);
            let out = &action.encrypted_note().out_ciphertext;
            let (note, _, _) = try_output_recovery_with_ovk(&domain, &ovk, action, action.cv_net(), out)?;
            (note.recipient() == to && note.value().inner() == amount).then(|| PaymentReceipt::for_note(chain_id, txid, i as u8, &note))
        })
    }
}
