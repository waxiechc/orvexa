//! P3-03, P3-09…P3-11 on disk: reopen, real process crashes inside a block
//! write, and detection of a tampered store.

use orvexa_shielded::{
    ledger::{Block, Ledger, LedgerError, Params, ProofCheck},
    store::{Fault, Store, StoreError},
    wallet::{build_genesis, Wallet},
    Envelope,
};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::process::Command;

const SUPPLY: u64 = 1_000_000;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orvexa-shielded-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Chain {
    params: Params,
    genesis: Vec<u8>,
    a: Wallet,
    b: Wallet,
    blocks: Vec<Block>,
}

impl Chain {
    fn new() -> Self {
        let params = Params::test(SUPPLY);
        let mut a = Wallet::from_seed(1);
        let genesis = build_genesis(&params, &[(a.address, 700_000), (a.address, 300_000)]);
        a.scan(0, &[Envelope::decode(&genesis).unwrap()]);
        Chain {
            params,
            genesis,
            a,
            b: Wallet::from_seed(2),
            blocks: Vec::new(),
        }
    }

    /// Next block paying `amount` from A to B, built against `ledger`.
    fn next(&self, ledger: &Ledger, amount: u64) -> Block {
        let tx = self
            .a
            .transfer(ledger, self.b.address, amount, 10, 20)
            .unwrap()
            .encode();
        Block {
            height: ledger.height() + 1,
            prev: ledger.tip(),
            txs: vec![tx],
        }
    }

    fn accept(&mut self, ledger_leaves_before: u64, block: Block) {
        let envs: Vec<_> = block
            .txs
            .iter()
            .map(|t| Envelope::decode(t).unwrap())
            .collect();
        self.a.scan(ledger_leaves_before, &envs);
        self.b.scan(ledger_leaves_before, &envs);
        self.blocks.push(block);
    }

    fn expected(&self, blocks: usize) -> [u8; 32] {
        Ledger::replay(self.params.clone(), &self.genesis, &self.blocks[..blocks])
            .unwrap()
            .state_digest()
    }
}

/// Store with genesis and one accepted block.
fn store_with_one_block(dir: &Path) -> (Chain, Store) {
    let mut chain = Chain::new();
    let mut store =
        Store::create(&dir.join("v2.sqlite"), chain.params.clone(), &chain.genesis).unwrap();
    let block = chain.next(store.ledger(), 200_000);
    let before = store.ledger().leaf_count();
    store.apply_block(&block).unwrap();
    chain.accept(before, block);
    (chain, store)
}

#[test]
fn reopen_recomputes_identical_state_fast_and_full() {
    let dir = scratch("reopen");
    let (mut chain, mut store) = store_with_one_block(&dir);
    let block = chain.next(store.ledger(), 50_000);
    let before = store.ledger().leaf_count();
    store.apply_block(&block).unwrap();
    chain.accept(before, block.clone());
    let live = store.ledger().state_digest();
    drop(store);

    for mode in [ProofCheck::Skip, ProofCheck::Verify] {
        let reopened = Store::open(&dir.join("v2.sqlite"), mode).unwrap();
        assert_eq!(reopened.ledger().state_digest(), live, "{mode:?}");
    }
    assert_eq!(chain.expected(2), live, "store equals a replay from zero");

    // Double spend is still rejected after restart.
    let mut reopened = Store::open(&dir.join("v2.sqlite"), ProofCheck::Skip).unwrap();
    let replay = Block {
        height: 3,
        prev: reopened.ledger().tip(),
        txs: block.txs,
    };
    assert_eq!(
        reopened.apply_block(&replay),
        Err(StoreError::Ledger(LedgerError::NullifierAlreadySpent))
    );
}

#[test]
fn rejected_block_writes_nothing() {
    let dir = scratch("rejected");
    let (chain, mut store) = store_with_one_block(&dir);
    let mut bad = chain.next(store.ledger(), 1_000);
    let tx = &mut bad.txs[0];
    let last = tx.len() - 1;
    tx[last] ^= 1; // binding signature
    let before = store.ledger().state_digest();
    assert_eq!(
        store.apply_block(&bad),
        Err(StoreError::Ledger(LedgerError::Authorization))
    );
    drop(store);
    assert_eq!(
        Store::open(&dir.join("v2.sqlite"), ProofCheck::Verify)
            .unwrap()
            .ledger()
            .state_digest(),
        before
    );
}

/// Runs only as a child of `crash_*`: applies the prepared block with a fault.
#[test]
#[ignore = "child process for crash tests"]
fn crash_child() {
    let Ok(dir) = std::env::var("ORVEXA_CRASH_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let mut store = Store::open(&dir.join("v2.sqlite"), ProofCheck::Skip).unwrap();
    let block = Block::decode(&std::fs::read(dir.join("next.block")).unwrap()).unwrap();
    store.inject_fault(
        match std::env::var("ORVEXA_CRASH_POINT").unwrap().as_str() {
            "before" => Fault::BeforeCommit,
            _ => Fault::AfterCommit,
        },
    );
    let _ = store.apply_block(&block);
    unreachable!("the fault aborts the process");
}

fn crash_in_child(dir: &Path, point: &str) {
    let status = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_child",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("ORVEXA_CRASH_DIR", dir)
        .env("ORVEXA_CRASH_POINT", point)
        .status()
        .unwrap();
    assert!(!status.success(), "child must die inside apply_block");
}

#[test]
fn crash_before_commit_keeps_old_state() {
    let dir = scratch("crash-before");
    let (mut chain, store) = store_with_one_block(&dir);
    let before = store.ledger().state_digest();
    let next = chain.next(store.ledger(), 30_000);
    std::fs::write(dir.join("next.block"), next.encode()).unwrap();
    let leaves = store.ledger().leaf_count();
    drop(store);

    crash_in_child(&dir, "before");

    let mut reopened = Store::open(&dir.join("v2.sqlite"), ProofCheck::Verify).unwrap();
    assert_eq!(reopened.ledger().height(), 1);
    assert_eq!(reopened.ledger().state_digest(), before, "no partial block");
    // The same block applies cleanly afterwards.
    reopened.apply_block(&next).unwrap();
    chain.accept(leaves, next);
    assert_eq!(reopened.ledger().state_digest(), chain.expected(2));
}

#[test]
fn crash_after_commit_keeps_new_state() {
    let dir = scratch("crash-after");
    let (mut chain, store) = store_with_one_block(&dir);
    let next = chain.next(store.ledger(), 30_000);
    std::fs::write(dir.join("next.block"), next.encode()).unwrap();
    let leaves = store.ledger().leaf_count();
    drop(store);

    crash_in_child(&dir, "after");

    chain.accept(leaves, next);
    let reopened = Store::open(&dir.join("v2.sqlite"), ProofCheck::Verify).unwrap();
    assert_eq!(reopened.ledger().height(), 2);
    assert_eq!(
        reopened.ledger().state_digest(),
        chain.expected(2),
        "committed block survives"
    );
}

#[test]
fn tampered_store_is_detected() {
    let dir = scratch("tamper");
    let (_, store) = store_with_one_block(&dir);
    drop(store);
    let original = std::fs::read(dir.join("v2.sqlite")).unwrap();
    let cases: [(&str, &str); 5] = [
        ("drop a nullifier", "DELETE FROM nullifiers WHERE rowid = (SELECT min(rowid) FROM nullifiers)"),
        ("forge a nullifier", "UPDATE nullifiers SET nf = zeroblob(32) WHERE rowid = (SELECT min(rowid) FROM nullifiers)"),
        ("drop a commitment", "DELETE FROM commitments WHERE position = (SELECT max(position) FROM commitments)"),
        ("forge a root", "UPDATE roots SET root = zeroblob(32) WHERE height = 1"),
        ("rewrite burned fees", "UPDATE meta SET value = zeroblob(8) WHERE key = 'burned'"),
    ];
    for (what, sql) in cases {
        std::fs::write(dir.join("v2.sqlite"), &original).unwrap();
        Connection::open(dir.join("v2.sqlite"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        for mode in [ProofCheck::Skip, ProofCheck::Verify] {
            assert!(
                Store::open(&dir.join("v2.sqlite"), mode).is_err(),
                "{what} was not detected ({mode:?})"
            );
        }
    }
    // One flipped byte inside a stored block body (a proof byte).
    std::fs::write(dir.join("v2.sqlite"), &original).unwrap();
    {
        let conn = Connection::open(dir.join("v2.sqlite")).unwrap();
        let mut body: Vec<u8> = conn
            .query_row("SELECT body FROM blocks WHERE height = 1", [], |r| r.get(0))
            .unwrap();
        let at = body.len() - 3_000;
        body[at] ^= 1;
        conn.execute("UPDATE blocks SET body = ?1 WHERE height = 1", [body])
            .unwrap();
    }
    assert_eq!(
        Store::open(&dir.join("v2.sqlite"), ProofCheck::Skip).err(),
        Some(StoreError::Corrupt("block id"))
    );
    assert_eq!(
        Store::open(&dir.join("v2.sqlite"), ProofCheck::Verify).err(),
        Some(StoreError::Ledger(LedgerError::Authorization))
    );

    std::fs::write(dir.join("v2.sqlite"), &original).unwrap();
    assert!(Store::open(&dir.join("v2.sqlite"), ProofCheck::Verify).is_ok());
}

#[test]
fn create_never_overwrites() {
    let dir = scratch("exists");
    let (chain, store) = store_with_one_block(&dir);
    drop(store);
    assert_eq!(
        Store::create(&dir.join("v2.sqlite"), chain.params.clone(), &chain.genesis).err(),
        Some(StoreError::AlreadyExists)
    );
}
