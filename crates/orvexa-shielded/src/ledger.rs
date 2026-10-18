//! In-memory model of the v2 ledger state and its transition rules.
//!
//! State: note commitment tree, the roots of the last `anchor_window` blocks,
//! the set of revealed nullifiers, burned fees. A block either applies in full
//! or leaves the state untouched: every check runs before the first mutation.

use crate::envelope::{self, Envelope, EnvelopeError, SUITE_IRONWOOD_V3, VERSION};
use crate::keys;
use incrementalmerkletree::{frontier::Frontier, Hashable, Level, Position, Retention};
use orchard::{
    bundle::{Authorization, BatchValidator, Flags, TxVersion},
    tree::{MerkleHashOrchard, MerklePath},
    Anchor, Bundle,
};
use sha2::{Digest, Sha256};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::collections::{HashMap, HashSet, VecDeque};

pub const CHAIN_ID_DOMAIN: &[u8] = b"ORVEXA/chain-id/v2";
pub const BLOCK_DOMAIN: &[u8] = b"ORVEXA/shielded-block/v2";
pub const STATE_DOMAIN: &[u8] = b"ORVEXA/shielded-state/v2";

/// Network parameters fixed in the network config before genesis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Params {
    pub salt: [u8; 32],
    /// Entire test supply, created once by the genesis bundle.
    pub supply: u64,
    /// A spend's anchor must be the tree root after one of the last N blocks.
    pub anchor_window: u32,
    /// `expiry_height` may be at most this far above the including block.
    pub max_expiry_delta: u64,
}

impl Params {
    pub fn test(supply: u64) -> Self {
        Params {
            salt: [7; 32],
            supply,
            anchor_window: 100,
            max_expiry_delta: 1_000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub prev: [u8; 32],
    pub txs: Vec<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    Envelope(EnvelopeError),
    WrongChain,
    WrongHeight,
    WrongParent,
    Expired,
    ExpiryTooFar,
    Flags,
    FeeBelowMinimum,
    ValueBalanceNotFee,
    UnknownAnchor,
    DuplicateNullifier,
    NullifierAlreadySpent,
    FeeExceedsRemainingSupply,
    /// A proof, spend-auth or binding signature in the block failed.
    Authorization,
    TreeFull,
    Genesis(&'static str),
}

impl From<EnvelopeError> for LedgerError {
    fn from(e: EnvelopeError) -> Self {
        LedgerError::Envelope(e)
    }
}

impl Block {
    /// height u64 · prev 32 · tx count u32 · (length u32 · envelope)…
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.prev);
        out.extend_from_slice(&(self.txs.len() as u32).to_le_bytes());
        for tx in &self.txs {
            out.extend_from_slice(&(tx.len() as u32).to_le_bytes());
            out.extend_from_slice(tx);
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let take = |b: &mut &[u8], n: usize| -> Option<Vec<u8>> {
            if b.len() < n {
                return None;
            }
            let (head, rest) = b.split_at(n);
            *b = rest;
            Some(head.to_vec())
        };
        let mut b = bytes;
        let height = u64::from_le_bytes(take(&mut b, 8)?.try_into().ok()?);
        let prev = take(&mut b, 32)?.try_into().ok()?;
        let count = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?) as usize;
        let mut txs = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let len = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?) as usize;
            txs.push(take(&mut b, len)?);
        }
        b.is_empty().then_some(Block { height, prev, txs })
    }
}

pub fn empty_anchor() -> Anchor {
    MerkleHashOrchard::empty_root(Level::from(32)).into()
}

/// Chain ID = hash of the network config and the genesis bundle's *effects*
/// (outputs, flags, value balance). Effects do not depend on signatures, so
/// the genesis bundle can then be signed under this chain ID without a cycle.
pub fn chain_id<A: Authorization>(params: &Params, genesis: &Bundle<A, i64>) -> [u8; 32] {
    let effects: [u8; 32] = genesis
        .commitment(TxVersion::V6)
        .expect("v6 commitment")
        .into();
    let mut h = Sha256::new();
    h.update(CHAIN_ID_DOMAIN);
    h.update([VERSION, SUITE_IRONWOOD_V3]);
    h.update(params.salt);
    h.update(params.supply.to_le_bytes());
    h.update(params.anchor_window.to_le_bytes());
    h.update(params.max_expiry_delta.to_le_bytes());
    h.update(genesis.anchor().to_bytes());
    h.update(effects);
    h.finalize().into()
}

pub fn block_id(chain_id: &[u8; 32], block: &Block) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(BLOCK_DOMAIN);
    h.update(chain_id);
    h.update(block.height.to_le_bytes());
    h.update(block.prev);
    h.update((block.txs.len() as u64).to_le_bytes());
    for tx in &block.txs {
        h.update(envelope::txid_of(tx));
    }
    h.finalize().into()
}

type Tree = ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16>;

/// Whether [`Ledger::validate_block`] runs the batch proof/signature check.
/// `Skip` is only for re-reading blocks this node already verified and stored
/// (their bytes are still bound by the recomputed block-ID chain).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofCheck {
    Verify,
    Skip,
}

/// A block that passed every rule, with all of its effects precomputed.
/// Applying it cannot fail; storing it first makes the transition atomic.
pub struct Validated {
    pub height: u64,
    pub id: [u8; 32],
    pub block: Block,
    pub envs: Vec<Envelope>,
    pub fees: u64,
    /// Tree root after this block.
    pub root: [u8; 32],
}

impl Validated {
    pub fn nullifiers(&self) -> impl Iterator<Item = [u8; 32]> + '_ {
        self.envs.iter().flat_map(|e| e.nullifiers())
    }
    pub fn commitments(&self) -> impl Iterator<Item = [u8; 32]> + '_ {
        self.envs
            .iter()
            .flat_map(|e| e.bundle.actions().iter().map(|a| a.cmx().to_bytes()))
    }
}

pub struct Ledger {
    params: Params,
    chain_id: [u8; 32],
    height: u64,
    tip: [u8; 32],
    tree: Tree,
    frontier: Frontier<MerkleHashOrchard, 32>,
    leaves: u64,
    roots: VecDeque<[u8; 32]>,
    nullifiers: HashSet<[u8; 32]>,
    /// txid → height of the block that included it.
    txids: HashMap<[u8; 32], u64>,
    burned: u64,
}

impl Ledger {
    /// Validates the genesis envelope and creates the ledger at height 0.
    pub fn from_genesis(params: Params, genesis_tx: &[u8]) -> Result<Self, LedgerError> {
        let env = Envelope::decode(genesis_tx)?;
        let b = &env.bundle;
        let supply =
            i64::try_from(params.supply).map_err(|_| LedgerError::Genesis("supply above i64"))?;
        if *b.flags() != Flags::SPENDS_DISABLED {
            return Err(LedgerError::Genesis("genesis must have spends disabled"));
        }
        if env.fee != 0 || env.expiry_height != 0 {
            return Err(LedgerError::Genesis("genesis has no fee and no expiry"));
        }
        if *b.value_balance() != -supply {
            return Err(LedgerError::Genesis(
                "genesis must mint exactly the configured supply",
            ));
        }
        if b.anchor().to_bytes() != empty_anchor().to_bytes() {
            return Err(LedgerError::Genesis(
                "genesis anchor must be the empty tree",
            ));
        }
        if env.chain_id != chain_id(&params, b) {
            return Err(LedgerError::WrongChain);
        }
        let sighash = env.sighash();
        if !batch_valid(&[(&env, sighash)]) {
            return Err(LedgerError::Authorization);
        }

        let genesis = Block {
            height: 0,
            prev: [0; 32],
            txs: vec![genesis_tx.to_vec()],
        };
        let tip = block_id(&env.chain_id, &genesis);
        let mut frontier = Frontier::<MerkleHashOrchard, 32>::empty();
        for action in env.bundle.actions() {
            frontier.append(MerkleHashOrchard::from_cmx(action.cmx()));
        }
        let root = frontier.root().to_bytes();
        let mut ledger = Ledger {
            frontier: Frontier::empty(),
            chain_id: env.chain_id,
            tree: ShardTree::new(MemoryShardStore::empty(), params.anchor_window as usize + 1),
            params,
            height: 0,
            tip,
            leaves: 0,
            roots: VecDeque::new(),
            nullifiers: HashSet::new(),
            txids: HashMap::new(),
            burned: 0,
        };
        // Genesis spends nothing; its dummy-spend nullifiers are still recorded
        // so that no published nullifier can ever repeat.
        ledger.commit(Validated {
            height: 0,
            id: tip,
            block: genesis,
            envs: vec![env],
            fees: 0,
            root,
        });
        Ok(ledger)
    }

    /// Rebuilds the state from genesis and the accepted blocks (restart/resync).
    pub fn replay(
        params: Params,
        genesis_tx: &[u8],
        blocks: &[Block],
    ) -> Result<Self, LedgerError> {
        let mut ledger = Ledger::from_genesis(params, genesis_tx)?;
        for block in blocks {
            ledger.apply_block(block)?;
        }
        Ok(ledger)
    }

    pub fn apply_block(&mut self, block: &Block) -> Result<[u8; 32], LedgerError> {
        let validated = self.validate_block(block, ProofCheck::Verify)?;
        let id = validated.id;
        self.commit(validated);
        Ok(id)
    }

    /// Runs every rule for `block` against the current state without changing it.
    pub fn validate_block(
        &self,
        block: &Block,
        proofs: ProofCheck,
    ) -> Result<Validated, LedgerError> {
        if block.height != self.height + 1 {
            return Err(LedgerError::WrongHeight);
        }
        if block.prev != self.tip {
            return Err(LedgerError::WrongParent);
        }
        let mut seen = HashSet::new();
        let mut fees: u64 = 0;
        let mut envs = Vec::with_capacity(block.txs.len());
        for tx in &block.txs {
            let env = Envelope::decode(tx)?;
            self.check_ordinary(&env, block.height)?;
            for nf in env.nullifiers() {
                if self.nullifiers.contains(&nf) {
                    return Err(LedgerError::NullifierAlreadySpent);
                }
                if !seen.insert(nf) {
                    return Err(LedgerError::DuplicateNullifier);
                }
            }
            fees = fees
                .checked_add(env.fee)
                .ok_or(LedgerError::FeeExceedsRemainingSupply)?;
            envs.push(env);
        }
        if fees > self.remaining_supply() {
            return Err(LedgerError::FeeExceedsRemainingSupply);
        }
        let new_leaves: u64 = envs.iter().map(|e| e.bundle.actions().len() as u64).sum();
        if self.leaves + new_leaves > 1u64 << 32 || u32::try_from(block.height).is_err() {
            return Err(LedgerError::TreeFull);
        }
        // Most expensive check last, once per block.
        if proofs == ProofCheck::Verify {
            let items: Vec<_> = envs.iter().map(|e| (e, e.sighash())).collect();
            if !items.is_empty() && !batch_valid(&items) {
                return Err(LedgerError::Authorization);
            }
        }
        let mut frontier = self.frontier.clone();
        for env in &envs {
            for action in env.bundle.actions() {
                frontier.append(MerkleHashOrchard::from_cmx(action.cmx()));
            }
        }
        Ok(Validated {
            height: block.height,
            id: block_id(&self.chain_id, block),
            block: block.clone(),
            envs,
            fees,
            root: frontier.root().to_bytes(),
        })
    }

    fn check_ordinary(&self, env: &Envelope, height: u64) -> Result<(), LedgerError> {
        let b = &env.bundle;
        if env.chain_id != self.chain_id {
            return Err(LedgerError::WrongChain);
        }
        if env.expiry_height < height {
            return Err(LedgerError::Expired);
        }
        if env.expiry_height - height > self.params.max_expiry_delta {
            return Err(LedgerError::ExpiryTooFar);
        }
        // Ordinary transfers: spends, outputs and cross-address all enabled.
        // Spends-disabled (mint) or restricted bundles never pass here.
        if *b.flags() != envelope::bundle_version().default_flags() {
            return Err(LedgerError::Flags);
        }
        if env.fee < 1 {
            return Err(LedgerError::FeeBelowMinimum);
        }
        // value_balance > 0 leaves the pool; it must be exactly the burned fee.
        // Negative (value entering the pool from nowhere) or any other amount is rejected.
        if i64::try_from(env.fee).ok() != Some(*b.value_balance()) {
            return Err(LedgerError::ValueBalanceNotFee);
        }
        if !self.roots.contains(&b.anchor().to_bytes()) {
            return Err(LedgerError::UnknownAnchor);
        }
        Ok(())
    }

    /// Applies a validated block. Infallible: every check already ran.
    pub fn commit(&mut self, v: Validated) {
        let mut added = 0;
        for env in &v.envs {
            self.txids.insert(env.txid(), v.height);
            for action in env.bundle.actions() {
                let leaf = MerkleHashOrchard::from_cmx(action.cmx());
                // Prototype: every leaf is marked so test wallets can obtain witnesses.
                // A node only needs the frontier; wallets track their own notes.
                self.tree
                    .append(leaf, Retention::Marked)
                    .expect("capacity checked");
                self.frontier.append(leaf);
                self.nullifiers.insert(action.nullifier().to_bytes());
                added += 1;
            }
        }
        let checkpoint = v.height as u32;
        self.tree
            .checkpoint(checkpoint)
            .expect("heights strictly increase");
        debug_assert_eq!(
            self.tree
                .root_at_checkpoint_id(&checkpoint)
                .unwrap()
                .unwrap()
                .to_bytes(),
            v.root,
            "frontier and tree agree"
        );
        self.roots.push_back(v.root);
        while self.roots.len() > self.params.anchor_window as usize {
            self.roots.pop_front();
        }
        self.leaves += added;
        self.burned += v.fees;
        self.height = v.height;
        self.tip = v.id;
    }

    /// Roots currently accepted as anchors, oldest first.
    pub fn recent_roots(&self) -> impl Iterator<Item = &[u8; 32]> {
        self.roots.iter()
    }

    pub fn chain_id(&self) -> [u8; 32] {
        self.chain_id
    }
    pub fn height(&self) -> u64 {
        self.height
    }
    pub fn tip(&self) -> [u8; 32] {
        self.tip
    }
    pub fn leaf_count(&self) -> u64 {
        self.leaves
    }
    pub fn burned(&self) -> u64 {
        self.burned
    }
    pub fn remaining_supply(&self) -> u64 {
        self.params.supply - self.burned
    }
    pub fn params(&self) -> &Params {
        &self.params
    }
    /// Height of the block that included `txid`, if any.
    pub fn tx_height(&self, txid: &[u8; 32]) -> Option<u64> {
        self.txids.get(txid).copied()
    }
    pub fn nullifier_set(&self) -> impl Iterator<Item = &[u8; 32]> {
        self.nullifiers.iter()
    }
    pub fn is_spent(&self, nullifier: &[u8; 32]) -> bool {
        self.nullifiers.contains(nullifier)
    }

    /// Root after the latest block: the anchor a new spend should use.
    pub fn anchor(&self) -> Anchor {
        Option::from(Anchor::from_bytes(
            *self.roots.back().expect("genesis root"),
        ))
        .expect("valid root")
    }

    /// Authentication path for the leaf at `position` against [`Self::anchor`].
    pub fn witness(&self, position: u64) -> Option<MerklePath> {
        let checkpoint = u32::try_from(self.height).ok()?;
        self.tree
            .witness_at_checkpoint_id(Position::from(position), &checkpoint)
            .ok()
            .flatten()
            .map(MerklePath::from)
    }

    /// Digest of everything a block can change; equal digests mean equal state.
    pub fn state_digest(&self) -> [u8; 32] {
        let mut nfs: Vec<_> = self.nullifiers.iter().collect();
        nfs.sort();
        let mut h = Sha256::new();
        h.update(STATE_DOMAIN);
        h.update(self.chain_id);
        h.update(self.height.to_le_bytes());
        h.update(self.tip);
        h.update(self.roots.back().expect("genesis root"));
        h.update(self.leaves.to_le_bytes());
        h.update(self.burned.to_le_bytes());
        for nf in nfs {
            h.update(nf);
        }
        h.finalize().into()
    }
}

/// One batch over every proof, spend-auth and binding signature.
fn batch_valid(items: &[(&Envelope, [u8; 32])]) -> bool {
    let mut batch = BatchValidator::new(keys::verifying_key());
    for (env, sighash) in items {
        if batch.add_bundle(&env.bundle, *sighash).is_err() {
            return false;
        }
    }
    batch.validate(keys::os_rng())
}
