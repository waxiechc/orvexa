//! P2/P3 acceptance tests (docs/PRIVACY_TEST_PLAN.md) with real Halo2 proofs
//! and RedPallas signatures. Attacks are built either as well-formed bundles
//! that break a ledger rule, or as byte edits of a valid envelope.

use incrementalmerkletree::Retention;
use orchard::{
    builder::BundleType,
    bundle::Flags,
    note_encryption::{IronwoodDomain, ENC_CIPHERTEXT_SIZE},
    tree::{MerkleHashOrchard, MerklePath},
    Address,
};
use orvexa_shielded::{
    envelope::{self, Envelope, EnvelopeError, ACTION_BYTES},
    ledger::{empty_anchor, Block, Ledger, LedgerError, Params},
    wallet::{build_envelope, build_genesis, RawTx, Wallet, WalletError},
};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use zcash_note_encryption::try_note_decryption;

const SUPPLY: u64 = 1_000_000;
const HEADER: usize = 4 + 1 + 1 + 32 + 8 + 8 + 1;

struct Net {
    params: Params,
    genesis: Vec<u8>,
    ledger: Ledger,
    blocks: Vec<Block>,
    a: Wallet,
    b: Wallet,
    c: Wallet,
}

fn net_with(params: Params) -> Net {
    let (mut a, b, c) = (Wallet::from_seed(1), Wallet::from_seed(2), Wallet::from_seed(3));
    let split = [600_000, 300_000, 100_000];
    let recipients: Vec<(Address, u64)> = split.iter().map(|v| (a.address, *v)).collect();
    let genesis = build_genesis(&params, &recipients);
    let ledger = Ledger::from_genesis(params.clone(), &genesis).expect("genesis accepted");
    a.scan(0, &[Envelope::decode(&genesis).unwrap()]);
    assert_eq!(a.balance(), SUPPLY);
    Net { params, genesis, ledger, blocks: Vec::new(), a, b, c }
}

fn net() -> Net {
    net_with(Params::test(SUPPLY))
}

impl Net {
    fn block(&self, txs: Vec<Vec<u8>>) -> Block {
        Block { height: self.ledger.height() + 1, prev: self.ledger.tip(), txs }
    }

    /// Applies a block; on success every wallet scans it.
    fn apply(&mut self, txs: Vec<Vec<u8>>) -> Result<(), LedgerError> {
        let block = self.block(txs);
        let first_leaf = self.ledger.leaf_count();
        self.ledger.apply_block(&block)?;
        let envs: Vec<_> = block.txs.iter().map(|t| Envelope::decode(t).unwrap()).collect();
        for w in [&mut self.a, &mut self.b, &mut self.c] {
            w.scan(first_leaf, &envs);
        }
        self.blocks.push(block);
        Ok(())
    }

    /// Asserts the block is rejected with `expected` and the state did not move.
    fn rejects(&mut self, txs: Vec<Vec<u8>>, expected: LedgerError) {
        let before = self.ledger.state_digest();
        assert_eq!(self.apply(txs), Err(expected));
        assert_eq!(self.ledger.state_digest(), before, "rejected block changed state");
    }

    fn rejects_any(&mut self, txs: Vec<Vec<u8>>) -> LedgerError {
        let before = self.ledger.state_digest();
        let err = self.apply(txs).expect_err("block must be rejected");
        assert_eq!(self.ledger.state_digest(), before, "rejected block changed state");
        err
    }

    fn pay(&self, from: &Wallet, to: Address, amount: u64) -> Vec<u8> {
        from.transfer(&self.ledger, to, amount, 10, 20).unwrap().encode()
    }

    fn conserved(&self) {
        let held = self.a.balance() + self.b.balance() + self.c.balance();
        assert_eq!(held + self.ledger.burned(), SUPPLY, "value created or destroyed");
        assert_eq!(self.ledger.remaining_supply(), SUPPLY - self.ledger.burned());
    }

    /// An unspent note of exactly `value` (the builder shuffles output order).
    fn note(&self, w: &Wallet, value: u64) -> (orchard::Note, MerklePath) {
        let owned = w.notes.iter().find(|o| !o.spent && o.note.value().inner() == value).expect("note with value");
        (owned.note, self.ledger.witness(owned.position).unwrap())
    }

    /// Spends the note of `value` from A to `to`, paying the standard fee.
    fn spend_note(&self, value: u64, to: Address) -> Vec<u8> {
        let key = [self.a.spend_key()];
        let (note, path) = self.note(&self.a, value);
        let mut raw = self.raw(&self.a, &key);
        raw.spends = vec![(self.a.fvk().clone(), note, path)];
        raw.outputs = vec![(None, to, value - 10)];
        build_envelope(raw).unwrap().encode()
    }

    fn raw<'k>(&self, w: &Wallet, keys: &'k [orchard::keys::SpendAuthorizingKey]) -> RawTx<'k> {
        RawTx {
            chain_id: self.ledger.chain_id(),
            fee: 10,
            expiry_height: self.ledger.height() + 20,
            anchor: self.ledger.anchor(),
            flags: envelope::bundle_version().default_flags(),
            bundle_type: BundleType::DEFAULT,
            spends: Vec::new(),
            outputs: Vec::new(),
            keys: {
                let _ = w;
                keys
            },
        }
    }
}

fn flip(mut tx: Vec<u8>, at: usize) -> Vec<u8> {
    tx[at] ^= 0x01;
    tx
}

fn trailer(n: usize) -> usize {
    HEADER + n * ACTION_BYTES
}

// ---------------------------------------------------------------- P2 ----

#[test]
fn p2_01_valid_transfer_scan_and_conservation() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 250_000);
    assert_eq!(Envelope::decode(&tx).unwrap().encode(), tx, "canonical round trip");
    n.apply(vec![tx]).unwrap();
    assert_eq!(n.b.balance(), 250_000);
    assert_eq!(n.a.balance(), SUPPLY - 250_000 - 10);
    assert_eq!(n.c.balance(), 0, "a stranger decrypts nothing");
    assert_eq!(n.ledger.burned(), 10);
    n.conserved();
}

#[test]
fn p2_02_tampered_or_truncated_proof() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    let actions = Envelope::decode(&tx).unwrap().bundle.actions().len();
    let proof_at = trailer(actions) + 1 + 8 + 32;
    n.rejects(vec![flip(tx.clone(), proof_at + 100)], LedgerError::Authorization);
    n.rejects(vec![tx[..tx.len() - 1].to_vec()], LedgerError::Envelope(EnvelopeError::Truncated));
    n.apply(vec![tx]).unwrap();
}

#[test]
fn p2_03_trailing_and_oversized_bytes() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    let mut extra = tx.clone();
    extra.push(0);
    n.rejects(vec![extra], LedgerError::Envelope(EnvelopeError::TrailingBytes));
    let huge = vec![0u8; envelope::encoded_len(envelope::MAX_ACTIONS) + 1];
    n.rejects(vec![huge], LedgerError::Envelope(EnvelopeError::Oversized));
}

#[test]
fn p2_04_mutated_action_fields_and_anchor() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    let actions = Envelope::decode(&tx).unwrap().bundle.actions().len();
    // cv_net, nullifier, rk, cmx of action 0, then the anchor.
    for at in [HEADER, HEADER + 32, HEADER + 64, HEADER + 96, trailer(actions) + 9] {
        n.rejects_any(vec![flip(tx.clone(), at)]);
    }
    n.apply(vec![tx]).unwrap();
}

#[test]
fn p2_05_spend_auth_and_binding_signatures() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    let spend_sig = HEADER + 32 * 5 + ENC_CIPHERTEXT_SIZE + 80;
    n.rejects(vec![flip(tx.clone(), spend_sig)], LedgerError::Authorization);
    let binding_sig = tx.len() - 64;
    n.rejects(vec![flip(tx.clone(), binding_sig + 1)], LedgerError::Authorization);
}

#[test]
fn p2_06_context_fields_are_signed() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    // version, suite, chain_id, fee, expiry
    n.rejects(vec![flip(tx.clone(), 4)], LedgerError::Envelope(EnvelopeError::UnsupportedVersion));
    n.rejects(vec![flip(tx.clone(), 5)], LedgerError::Envelope(EnvelopeError::UnsupportedSuite));
    n.rejects(vec![flip(tx.clone(), 6)], LedgerError::WrongChain);
    n.rejects(vec![flip(tx.clone(), 38)], LedgerError::ValueBalanceNotFee);
    n.rejects(vec![flip(tx.clone(), 46)], LedgerError::Authorization);
    // Encrypted note, out ciphertext and ephemeral key of action 0.
    n.rejects(vec![flip(tx.clone(), HEADER + 32 * 5 + 7)], LedgerError::Authorization);
    n.rejects(vec![flip(tx.clone(), HEADER + 32 * 5 + ENC_CIPHERTEXT_SIZE + 3)], LedgerError::Authorization);
    n.rejects_any(vec![flip(tx.clone(), HEADER + 128)]);
    n.apply(vec![tx]).unwrap();
}

#[test]
fn p2_07_mint_flags_and_reserved_bits_rejected() {
    let mut n = net();
    // Output-only bundle in an ordinary block: tries to mint 5 000 from nothing.
    let mut raw = n.raw(&n.a, &[]);
    raw.flags = Flags::SPENDS_DISABLED;
    raw.bundle_type = BundleType::Coinbase;
    raw.anchor = empty_anchor();
    raw.fee = 0;
    raw.outputs = vec![(None, n.c.address, 5_000)];
    let mint = build_envelope(raw).unwrap().encode();
    n.rejects(vec![mint], LedgerError::Flags);

    let tx = n.pay(&n.a, n.b.address, 1_000);
    let actions = Envelope::decode(&tx).unwrap().bundle.actions().len();
    let mut reserved = tx.clone();
    reserved[trailer(actions)] |= 0x80;
    n.rejects(vec![reserved], LedgerError::Envelope(EnvelopeError::UnrepresentableFlags));
}

#[test]
fn p2_08_action_count_bounds() {
    let mut n = net();
    let tx = n.pay(&n.a, n.b.address, 1_000);
    for count in [0u8, 17, 255] {
        let mut bad = tx.clone();
        bad[HEADER - 1] = count;
        n.rejects(vec![bad], LedgerError::Envelope(EnvelopeError::ActionCount));
    }
    n.rejects(vec![Vec::new()], LedgerError::Envelope(EnvelopeError::Truncated));
}

#[test]
fn p2_10_12_value_balance_must_equal_fee() {
    let mut n = net();
    let key = [n.a.spend_key()];
    let (note, path) = n.note(&n.a, 100_000);

    // Outputs larger than the spend: value balance negative.
    let mut raw = n.raw(&n.a, &key);
    raw.spends = vec![(n.a.fvk().clone(), note, path.clone())];
    raw.outputs = vec![(None, n.c.address, 150_000)];
    n.rejects(vec![build_envelope(raw).unwrap().encode()], LedgerError::ValueBalanceNotFee);

    // Declared fee 10, but 20 actually leaves the pool.
    let mut raw = n.raw(&n.a, &key);
    raw.spends = vec![(n.a.fvk().clone(), note, path.clone())];
    raw.outputs = vec![(None, n.c.address, 100_000 - 20)];
    n.rejects(vec![build_envelope(raw).unwrap().encode()], LedgerError::ValueBalanceNotFee);

    // u64::MAX fee cannot be a value balance.
    let mut huge = n.pay(&n.a, n.b.address, 1_000);
    huge[38..46].copy_from_slice(&u64::MAX.to_le_bytes());
    n.rejects(vec![huge], LedgerError::ValueBalanceNotFee);
}

#[test]
fn p2_11_zero_fee_rejected() {
    let mut n = net();
    let key = [n.a.spend_key()];
    let (note, path) = n.note(&n.a, 100_000);
    let mut raw = n.raw(&n.a, &key);
    raw.fee = 0;
    raw.spends = vec![(n.a.fvk().clone(), note, path)];
    raw.outputs = vec![(None, n.b.address, 100_000)];
    n.rejects(vec![build_envelope(raw).unwrap().encode()], LedgerError::FeeBelowMinimum);
}

#[test]
fn p2_15_txid_commits_to_every_byte() {
    let n = net();
    let env = n.a.transfer(&n.ledger, n.b.address, 1_000, 10, 20).unwrap();
    let bytes = env.encode();
    let id = env.txid();
    for at in [0, 6, 40, HEADER + 200, bytes.len() / 2, bytes.len() - 1] {
        assert_ne!(envelope::txid_of(&flip(bytes.clone(), at)), id);
    }
    // Sighash excludes proof and signatures; the tx ID does not.
    let resigned = n.a.transfer(&n.ledger, n.b.address, 1_000, 10, 20).unwrap();
    assert_ne!(resigned.txid(), id);
}

// ---------------------------------------------------------------- P3 ----

#[test]
fn p3_01_double_spend_in_block_and_across_blocks() {
    let mut n = net();
    let (to_b, to_c) = (n.spend_note(100_000, n.b.address), n.spend_note(100_000, n.c.address));
    assert_ne!(envelope::txid_of(&to_b), envelope::txid_of(&to_c));
    n.rejects(vec![to_b.clone(), to_c.clone()], LedgerError::DuplicateNullifier);
    n.apply(vec![to_b]).unwrap();
    n.rejects(vec![to_c], LedgerError::NullifierAlreadySpent);
    n.conserved();
}

#[test]
fn p3_02_same_note_twice_inside_one_bundle() {
    let mut n = net();
    let key = [n.a.spend_key()];
    let (note, path) = n.note(&n.a, 100_000);
    let mut raw = n.raw(&n.a, &key);
    raw.spends = vec![(n.a.fvk().clone(), note, path.clone()), (n.a.fvk().clone(), note, path)];
    raw.outputs = vec![(None, n.c.address, 200_000 - 10)];
    // The library proves and signs this; only the ledger stops it.
    let tx = build_envelope(raw).unwrap().encode();
    n.rejects(vec![tx], LedgerError::DuplicateNullifier);
}

#[test]
fn p3_03_restart_replay_keeps_nullifiers() {
    let mut n = net();
    let first = n.pay(&n.a, n.b.address, 250_000);
    n.apply(vec![first]).unwrap();
    let second = n.pay(&n.b, n.c.address, 50_000);
    n.apply(vec![second.clone()]).unwrap();

    let replayed = Ledger::replay(n.params.clone(), &n.genesis, &n.blocks).unwrap();
    assert_eq!(replayed.state_digest(), n.ledger.state_digest());
    n.ledger = replayed;
    let again = Block { height: n.ledger.height() + 1, prev: n.ledger.tip(), txs: vec![second] };
    assert_eq!(n.ledger.apply_block(&again), Err(LedgerError::NullifierAlreadySpent));
}

#[test]
fn p3_04_proof_against_attacker_tree_is_rejected() {
    let mut n = net();
    // The attacker proves a coinbase note into a private tree the ledger never saw.
    let mut raw = n.raw(&n.c, &[]);
    raw.flags = Flags::SPENDS_DISABLED;
    raw.bundle_type = BundleType::Coinbase;
    raw.anchor = empty_anchor();
    raw.fee = 0;
    raw.outputs = vec![(None, n.c.address, 500_000)];
    let private = build_envelope(raw).unwrap();
    let action = private.bundle.actions().first();
    let ivk = orchard::keys::PreparedIncomingViewingKey::new(&n.c.fvk().to_ivk(orchard::keys::Scope::External));
    let (note, _, _) = try_note_decryption(&IronwoodDomain::for_action(action), &ivk, action).unwrap();
    let mut tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16> =
        ShardTree::new(MemoryShardStore::empty(), 10);
    tree.append(MerkleHashOrchard::from_cmx(action.cmx()), Retention::Marked).unwrap();
    tree.checkpoint(0).unwrap();
    let root = tree.root_at_checkpoint_id(&0).unwrap().unwrap();
    let path: MerklePath = tree.witness_at_checkpoint_id(0.into(), &0).unwrap().unwrap().into();

    let key = [n.c.spend_key()];
    let mut raw = n.raw(&n.c, &key);
    raw.anchor = root.into();
    raw.spends = vec![(n.c.fvk().clone(), note, path)];
    raw.outputs = vec![(None, n.c.address, 500_000 - 10)];
    let tx = build_envelope(raw).unwrap().encode();
    n.rejects(vec![tx], LedgerError::UnknownAnchor);
    n.conserved();
}

#[test]
fn p3_05_stale_anchor_and_cross_chain() {
    let mut n = net_with(Params { anchor_window: 2, ..Params::test(SUPPLY) });
    let stale = n.spend_note(100_000, n.b.address);
    // Empty blocks keep the same root, so the old anchor stays valid; new
    // commitments in two later blocks push it out of the window.
    n.apply(vec![]).unwrap();
    n.apply(vec![n.spend_note(300_000, n.c.address)]).unwrap();
    n.apply(vec![n.spend_note(600_000, n.c.address)]).unwrap();
    n.rejects(vec![stale], LedgerError::UnknownAnchor);

    let other = net_with(Params { salt: [8; 32], ..Params::test(SUPPLY) });
    let foreign = other.pay(&other.a, other.b.address, 1_000);
    n.rejects(vec![foreign], LedgerError::WrongChain);
}

#[test]
fn p3_06_expiry_window() {
    let mut n = net();
    let now = n.a.transfer(&n.ledger, n.b.address, 1_000, 10, 0).unwrap().encode();
    n.rejects(vec![now], LedgerError::Expired);
    let far = n.a.transfer(&n.ledger, n.b.address, 1_000, 10, 5_000).unwrap().encode();
    n.rejects(vec![far], LedgerError::ExpiryTooFar);
}

#[test]
fn p3_07_genesis_rules() {
    let params = Params::test(SUPPLY);
    let a = Wallet::from_seed(1);
    // Mints less than the configured supply.
    let short = build_genesis(&params, &[(a.address, SUPPLY - 1)]);
    assert!(matches!(Ledger::from_genesis(params.clone(), &short), Err(LedgerError::Genesis(_))));
    // Valid genesis presented to a network with a different config.
    let good = build_genesis(&params, &[(a.address, SUPPLY)]);
    let other = Params { salt: [9; 32], ..params.clone() };
    assert_eq!(Ledger::from_genesis(other, &good).err(), Some(LedgerError::WrongChain));
    // Same bytes, one changed proof byte.
    let n = Envelope::decode(&good).unwrap().bundle.actions().len();
    let bad = flip(good.clone(), trailer(n) + 41 + 10);
    assert_eq!(Ledger::from_genesis(params.clone(), &bad).err(), Some(LedgerError::Authorization));
    assert!(Ledger::from_genesis(params, &good).is_ok());
}

#[test]
fn p3_12_block_with_one_bad_tx_applies_nothing() {
    let mut n = net();
    let good = n.pay(&n.a, n.b.address, 1_000);
    let other_note = {
        let key = [n.a.spend_key()];
        let (note, path) = n.note(&n.a, 300_000);
        let mut raw = n.raw(&n.a, &key);
        raw.spends = vec![(n.a.fvk().clone(), note, path)];
        raw.outputs = vec![(None, n.c.address, 300_000 - 10)];
        build_envelope(raw).unwrap().encode()
    };
    let bad = flip(other_note, HEADER + 32 * 5 + ENC_CIPHERTEXT_SIZE + 80);
    n.rejects(vec![good.clone(), bad], LedgerError::Authorization);
    n.apply(vec![good]).unwrap();
    n.conserved();
}

#[test]
fn p3_round_trip_a_b_c_a_conserves_value() {
    let mut n = net();
    for (from, to, amount) in [("a", "b", 400_000), ("b", "c", 150_000), ("c", "a", 70_000), ("a", "c", 1)] {
        let wallet = |name| match name { "a" => &n.a, "b" => &n.b, _ => &n.c };
        let tx = n.pay(wallet(from), wallet(to).address, amount);
        n.apply(vec![tx]).unwrap();
        n.conserved();
    }
    assert_eq!(n.b.balance(), 400_000 - 150_000 - 10);
    assert_eq!(n.c.balance(), 150_000 - 70_000 - 10 + 1);
    assert_eq!(n.ledger.burned(), 40);
}

#[test]
fn wallet_refuses_bad_amounts() {
    let n = net();
    assert_eq!(n.a.transfer(&n.ledger, n.b.address, 0, 10, 20).err(), Some(WalletError::ZeroAmount));
    assert_eq!(n.a.transfer(&n.ledger, n.b.address, 5, 0, 20).err(), Some(WalletError::FeeBelowMinimum));
    assert_eq!(n.a.transfer(&n.ledger, n.b.address, u64::MAX, 10, 20).err(), Some(WalletError::Overflow));
    assert_eq!(n.b.transfer(&n.ledger, n.a.address, 5, 10, 20).err(), Some(WalletError::InsufficientFunds));
}

#[test]
fn vectors_sizes_and_domains() {
    assert_eq!(ACTION_BYTES, 884);
    assert_eq!(envelope::encoded_len(2), HEADER + 2 * 884 + 105 + 7_264);
    assert_eq!(envelope::encoded_len(4), HEADER + 4 * 884 + 105 + 11_808);
    assert_eq!(envelope::encoded_len(8), HEADER + 8 * 884 + 105 + 20_896);
    assert_eq!([envelope::encoded_len(2), envelope::encoded_len(4), envelope::encoded_len(8)], [9_192, 15_504, 28_128]);
    assert_eq!(envelope::SIGHASH_DOMAIN, b"ORVEXA/shielded-sighash/v2");
    assert_eq!(envelope::TXID_DOMAIN, b"ORVEXA/shielded-txid/v2");
}
