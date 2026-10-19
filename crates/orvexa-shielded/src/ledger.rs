//! In-memory model of the v2 ledger state and its transition rules.
//!
//! State: note commitment tree, the roots of the last `anchor_window` blocks,
//! the set of revealed nullifiers, burned fees. A block either applies in full
//! or leaves the state untouched: every check runs before the first mutation.
//!
//! Every block after genesis carries a commit certificate: precommit
//! signatures of validators holding more than 2/3 of the voting power for
//! exactly this block ID at its height. The bytes signed are the precommit
//! votes of the BFT engine ([`vote_sign_bytes`]), so a certificate the engine
//! produced verifies here without the engine, and a follower trusts no single
//! machine.

use crate::envelope::{self, Envelope, EnvelopeError, SUITE_IRONWOOD_V3, VERSION};
use crate::keys;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
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
pub const VOTE_DOMAIN: &[u8] = b"ORVEXA/shielded-vote/v2\n";
pub const PROPOSAL_DOMAIN: &[u8] = b"ORVEXA/shielded-proposal/v2\n";
/// Most validators a network may have (keeps certificates and checks bounded).
pub const MAX_VALIDATORS: usize = 64;

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
    /// Ed25519 keys of the validators, sorted and distinct, one vote each.
    /// Part of the chain ID: another set is another chain.
    pub validators: Vec<[u8; 32]>,
}

impl Params {
    /// Test parameters with the single validator [`test_validator`]. Never for a shared network.
    pub fn test(supply: u64) -> Self {
        Params {
            salt: [7; 32],
            supply,
            anchor_window: 100,
            max_expiry_delta: 1_000,
            validators: vec![test_validator().verifying_key().to_bytes()],
        }
    }

    /// Sorts and checks the validator set: 1..=MAX_VALIDATORS distinct valid keys.
    pub fn set_validators(&mut self, mut keys: Vec<[u8; 32]>) -> Result<(), &'static str> {
        keys.sort();
        keys.dedup();
        self.validators = keys;
        self.check_validators()
    }

    pub fn check_validators(&self) -> Result<(), &'static str> {
        let v = &self.validators;
        if v.is_empty() || v.len() > MAX_VALIDATORS {
            return Err("a network needs 1 to 64 validators");
        }
        if v.windows(2).any(|w| w[0] >= w[1]) {
            return Err("validator keys must be sorted and distinct");
        }
        if v.iter()
            .any(|k| VerifyingKey::from_bytes(k).map_or(true, |k| k.is_weak()))
        {
            return Err("invalid validator key");
        }
        Ok(())
    }

    /// Votes needed for a decision: more than 2/3 of all validators.
    pub fn quorum(&self) -> usize {
        self.validators.len() * 2 / 3 + 1
    }
}

/// Fixed, publicly known validator key for tests only.
pub fn test_validator() -> SigningKey {
    SigningKey::from_bytes(&[9u8; 32])
}

/// Bytes a validator signs for a vote. `kind` 0 = prevote, 1 = precommit;
/// `value` is `None` for a nil vote. Shared with the BFT engine.
pub fn vote_sign_bytes(
    chain_id: &[u8; 32],
    kind: u8,
    height: u64,
    round: i64,
    value: Option<&[u8; 32]>,
    validator: &[u8; 32],
) -> Vec<u8> {
    let mut b = Vec::with_capacity(VOTE_DOMAIN.len() + 32 + 1 + 8 + 8 + 33 + 32);
    b.extend_from_slice(VOTE_DOMAIN);
    b.extend_from_slice(chain_id);
    b.push(kind);
    b.extend_from_slice(&height.to_be_bytes());
    b.extend_from_slice(&round.to_be_bytes());
    match value {
        None => b.push(0),
        Some(id) => {
            b.push(1);
            b.extend_from_slice(id);
        }
    }
    b.extend_from_slice(validator);
    b
}

/// Precommits of more than 2/3 of the validators for one block in one round.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Commit {
    pub round: u32,
    /// (validator key, Ed25519 signature over the precommit), sorted by key.
    pub signatures: Vec<([u8; 32], [u8; 64])>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub height: u64,
    pub prev: [u8; 32],
    /// Empty for genesis and for a proposed block that is not decided yet.
    pub commit: Commit,
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
    /// The commit certificate is missing, has unknown or repeated signers,
    /// a bad signature, or fewer than the quorum.
    BadCommit,
    TreeFull,
    Genesis(&'static str),
}

impl From<EnvelopeError> for LedgerError {
    fn from(e: EnvelopeError) -> Self {
        LedgerError::Envelope(e)
    }
}

impl Block {
    /// A block decided in round 0 by one validator: enough on a network with
    /// a single validator (tests, a local network).
    pub fn produce(
        height: u64,
        prev: [u8; 32],
        txs: Vec<Vec<u8>>,
        key: &SigningKey,
        chain_id: &[u8; 32],
    ) -> Self {
        Self::seal(height, prev, txs, 0, &[key], chain_id)
    }

    /// A block with precommits of `keys` in `round`.
    pub fn seal(
        height: u64,
        prev: [u8; 32],
        txs: Vec<Vec<u8>>,
        round: u32,
        keys: &[&SigningKey],
        chain_id: &[u8; 32],
    ) -> Self {
        let mut block = Block {
            height,
            prev,
            commit: Commit::default(),
            txs,
        };
        let id = block_id(chain_id, &block);
        let mut signatures: Vec<_> = keys
            .iter()
            .map(|k| {
                let key = k.verifying_key().to_bytes();
                let msg = vote_sign_bytes(chain_id, 1, height, round as i64, Some(&id), &key);
                (key, k.sign(&msg).to_bytes())
            })
            .collect();
        signatures.sort_by_key(|(k, _)| *k);
        block.commit = Commit { round, signatures };
        block
    }

    /// height u64 · prev 32 · round u32 · signer count u16 · (key 32 · signature 64)… ·
    /// tx count u32 · (length u32 · envelope)…
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.height.to_le_bytes());
        out.extend_from_slice(&self.prev);
        out.extend_from_slice(&self.commit.round.to_le_bytes());
        out.extend_from_slice(&(self.commit.signatures.len() as u16).to_le_bytes());
        for (key, sig) in &self.commit.signatures {
            out.extend_from_slice(key);
            out.extend_from_slice(sig);
        }
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
        let round = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?);
        let signers = u16::from_le_bytes(take(&mut b, 2)?.try_into().ok()?) as usize;
        if signers > MAX_VALIDATORS {
            return None;
        }
        let mut signatures = Vec::with_capacity(signers);
        for _ in 0..signers {
            let key = take(&mut b, 32)?.try_into().ok()?;
            let sig = take(&mut b, 64)?.try_into().ok()?;
            signatures.push((key, sig));
        }
        let count = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?) as usize;
        let mut txs = Vec::with_capacity(count.min(1024));
        for _ in 0..count {
            let len = u32::from_le_bytes(take(&mut b, 4)?.try_into().ok()?) as usize;
            txs.push(take(&mut b, len)?);
        }
        b.is_empty().then_some(Block {
            height,
            prev,
            commit: Commit { round, signatures },
            txs,
        })
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
    h.update((params.validators.len() as u32).to_le_bytes());
    for key in &params.validators {
        h.update(key);
    }
    h.update(genesis.anchor().to_bytes());
    h.update(effects);
    h.finalize().into()
}

/// Checks a commit certificate for block `id` at `height`: every signer is a
/// validator, appears once, signed the precommit (strict Ed25519), and the
/// signers are more than 2/3 of the set.
pub fn verify_commit(
    params: &Params,
    chain_id: &[u8; 32],
    height: u64,
    id: &[u8; 32],
    commit: &Commit,
) -> Result<(), LedgerError> {
    let mut seen = HashSet::new();
    for (key, sig) in &commit.signatures {
        if !params.validators.contains(key) || !seen.insert(*key) {
            return Err(LedgerError::BadCommit);
        }
        let msg = vote_sign_bytes(chain_id, 1, height, commit.round as i64, Some(id), key);
        VerifyingKey::from_bytes(key)
            .and_then(|k| k.verify_strict(&msg, &Signature::from_bytes(sig)))
            .map_err(|_| LedgerError::BadCommit)?;
    }
    if seen.len() < params.quorum() {
        return Err(LedgerError::BadCommit);
    }
    Ok(())
}

/// Block ID: commits to height, parent and every txid, not to the certificate.
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
        params.check_validators().map_err(LedgerError::Genesis)?;
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
            commit: Commit::default(),
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
        let id = block_id(&self.chain_id, block);
        verify_commit(
            &self.params,
            &self.chain_id,
            block.height,
            &id,
            &block.commit,
        )?;
        let (envs, fees) = self.validate_body(block.height, &block.txs, proofs)?;
        let mut frontier = self.frontier.clone();
        for env in &envs {
            for action in env.bundle.actions() {
                frontier.append(MerkleHashOrchard::from_cmx(action.cmx()));
            }
        }
        Ok(Validated {
            height: block.height,
            id,
            block: block.clone(),
            envs,
            fees,
            root: frontier.root().to_bytes(),
        })
    }

    /// Checks transactions as if they formed the next block, without a
    /// commit certificate (mempool admission, block assembly, and a
    /// validator's check of a proposed block).
    pub fn check_candidates(&self, txs: &[Vec<u8>], proofs: ProofCheck) -> Result<(), LedgerError> {
        self.validate_body(self.height + 1, txs, proofs).map(|_| ())
    }

    fn validate_body(
        &self,
        height: u64,
        txs: &[Vec<u8>],
        proofs: ProofCheck,
    ) -> Result<(Vec<Envelope>, u64), LedgerError> {
        let mut seen = HashSet::new();
        let mut fees: u64 = 0;
        let mut envs = Vec::with_capacity(txs.len());
        for tx in txs {
            let env = Envelope::decode(tx)?;
            self.check_ordinary(&env, height)?;
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
        if self.leaves + new_leaves > 1u64 << 32 || u32::try_from(height).is_err() {
            return Err(LedgerError::TreeFull);
        }
        // Most expensive check last, once per block.
        if proofs == ProofCheck::Verify {
            let items: Vec<_> = envs.iter().map(|e| (e, e.sighash())).collect();
            if !items.is_empty() && !batch_valid(&items) {
                return Err(LedgerError::Authorization);
            }
        }
        Ok((envs, fees))
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
