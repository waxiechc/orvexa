//! Durable v2 state in SQLite. A block's effects (block body, nullifiers,
//! commitments, root, tip, burned fees) are written in one SQL transaction
//! before the in-memory ledger changes, so a crash leaves either the old or
//! the new state, never a partial block.
//!
//! On open, the state is recomputed from genesis and the stored block bodies;
//! the nullifier, commitment and root tables are indexes that must match that
//! recomputation, not a source of trust.

use crate::ledger::{Block, Ledger, LedgerError, Params, ProofCheck, Validated};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    Sql(String),
    Ledger(LedgerError),
    /// Stored data disagrees with what the stored blocks imply.
    Corrupt(&'static str),
    AlreadyExists,
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e.to_string())
    }
}
impl From<LedgerError> for StoreError {
    fn from(e: LedgerError) -> Self {
        StoreError::Ledger(e)
    }
}

/// Test-only crash points inside [`Store::apply_block`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Abort after every row is written but before COMMIT.
    BeforeCommit,
    /// Abort after COMMIT, before the in-memory ledger is updated.
    AfterCommit,
}

const SCHEMA: &str = "
CREATE TABLE meta (key TEXT PRIMARY KEY, value BLOB NOT NULL) STRICT;
-- Blocks after genesis; genesis itself is kept in meta.
CREATE TABLE blocks (height INTEGER PRIMARY KEY, id BLOB NOT NULL UNIQUE, prev BLOB NOT NULL, body BLOB NOT NULL) STRICT;
CREATE TABLE nullifiers (nf BLOB PRIMARY KEY, height INTEGER NOT NULL) STRICT;
CREATE TABLE commitments (position INTEGER PRIMARY KEY, cmx BLOB NOT NULL, height INTEGER NOT NULL) STRICT;
CREATE TABLE roots (height INTEGER PRIMARY KEY, root BLOB NOT NULL) STRICT;
";

pub struct Store {
    conn: Connection,
    ledger: Ledger,
    fault: Option<Fault>,
}

impl Store {
    /// Creates a new store holding only the verified genesis.
    pub fn create(path: &Path, params: Params, genesis_tx: &[u8]) -> Result<Self, StoreError> {
        if path.exists() {
            return Err(StoreError::AlreadyExists);
        }
        let ledger = Ledger::from_genesis(params.clone(), genesis_tx)?;
        let mut conn = connect(path)?;
        conn.execute_batch(SCHEMA)?;
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO meta VALUES ('params', ?1)", [encode_params(&params)])?;
        tx.execute("INSERT INTO meta VALUES ('genesis', ?1)", [genesis_tx])?;
        let genesis = ledger_genesis_effects(&ledger, genesis_tx)?;
        write_effects(&tx, &genesis, 0)?;
        write_tip(&tx, &ledger)?;
        tx.commit()?;
        Ok(Store { conn, ledger, fault: None })
    }

    /// Opens an existing store, recomputing the state from its blocks.
    /// `ProofCheck::Verify` re-verifies every proof (full audit of the file);
    /// `ProofCheck::Skip` trusts proofs this node verified before storing them,
    /// while still rebinding every byte through the block-ID chain.
    pub fn open(path: &Path, proofs: ProofCheck) -> Result<Self, StoreError> {
        if !path.exists() {
            return Err(StoreError::Sql("store does not exist".into()));
        }
        let conn = connect(path)?;
        let params = decode_params(&meta(&conn, "params")?)?;
        let genesis_tx = meta(&conn, "genesis")?;
        let mut ledger = Ledger::from_genesis(params, &genesis_tx)?;

        let mut commitments = genesis_commitments(&genesis_tx)?;
        let mut roots = vec![(0u64, *ledger.recent_roots().last().expect("genesis root"))];
        {
            let mut stmt = conn.prepare("SELECT height, id, prev, body FROM blocks WHERE height > 0 ORDER BY height")?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let height: u64 = row.get(0)?;
                let id: Vec<u8> = row.get(1)?;
                let prev: Vec<u8> = row.get(2)?;
                let block = Block::decode(&row.get::<_, Vec<u8>>(3)?).ok_or(StoreError::Corrupt("block body"))?;
                if block.height != height || block.prev[..] != prev[..] {
                    return Err(StoreError::Corrupt("block header columns"));
                }
                let v = ledger.validate_block(&block, proofs).map_err(|e| match e {
                    LedgerError::WrongParent | LedgerError::WrongHeight => StoreError::Corrupt("block chain"),
                    other => StoreError::Ledger(other),
                })?;
                if v.id[..] != id[..] {
                    return Err(StoreError::Corrupt("block id"));
                }
                commitments.extend(v.commitments());
                roots.push((v.height, v.root));
                ledger.commit(v);
            }
        }

        // Index tables must equal what the blocks imply.
        let stored_nfs: Vec<Vec<u8>> = column(&conn, "SELECT nf FROM nullifiers ORDER BY nf")?;
        let mut expected_nfs: Vec<[u8; 32]> = ledger_nullifiers(&ledger);
        expected_nfs.sort();
        if stored_nfs.len() != expected_nfs.len() || stored_nfs.iter().zip(&expected_nfs).any(|(a, b)| a[..] != b[..]) {
            return Err(StoreError::Corrupt("nullifier table"));
        }
        let stored_cmx: Vec<Vec<u8>> = column(&conn, "SELECT cmx FROM commitments ORDER BY position")?;
        let positions: Vec<i64> = column(&conn, "SELECT position FROM commitments ORDER BY position")?;
        if stored_cmx.len() != commitments.len()
            || positions.iter().enumerate().any(|(i, p)| *p != i as i64)
            || stored_cmx.iter().zip(&commitments).any(|(a, b)| a[..] != b[..])
        {
            return Err(StoreError::Corrupt("commitment table"));
        }
        let stored_roots: Vec<Vec<u8>> = column(&conn, "SELECT root FROM roots ORDER BY height")?;
        if stored_roots.len() != roots.len() || stored_roots.iter().zip(&roots).any(|(a, (_, b))| a[..] != b[..]) {
            return Err(StoreError::Corrupt("root table"));
        }
        if meta(&conn, "tip")? != ledger.tip()
            || meta(&conn, "height")? != ledger.height().to_le_bytes()
            || meta(&conn, "burned")? != ledger.burned().to_le_bytes()
        {
            return Err(StoreError::Corrupt("tip metadata"));
        }
        Ok(Store { conn, ledger, fault: None })
    }

    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Stored body of the block at `height` (≥ 1).
    pub fn block(&self, height: u64) -> Result<Option<Block>, StoreError> {
        let body: Option<Vec<u8>> = self
            .conn
            .query_row("SELECT body FROM blocks WHERE height = ?1", [height as i64], |r| r.get(0))
            .optional()?;
        body.map(|b| Block::decode(&b).ok_or(StoreError::Corrupt("block body"))).transpose()
    }

    /// The genesis envelope this store was created with.
    pub fn genesis_tx(&self) -> Result<Vec<u8>, StoreError> {
        meta(&self.conn, "genesis")
    }

    #[doc(hidden)]
    pub fn inject_fault(&mut self, fault: Fault) {
        self.fault = Some(fault);
    }

    /// Validates, durably stores, then applies one block.
    pub fn apply_block(&mut self, block: &Block) -> Result<[u8; 32], StoreError> {
        let v = self.ledger.validate_block(block, ProofCheck::Verify)?;
        let first = self.ledger.leaf_count();
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO blocks (height, id, prev, body) VALUES (?1, ?2, ?3, ?4)",
            params![v.height as i64, &v.id[..], &v.block.prev[..], block.encode()],
        )?;
        write_effects(&tx, &v, first)?;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'tip'", [&v.id[..]])?;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'height'", [&v.height.to_le_bytes()[..]])?;
        let burned = self.ledger.burned() + v.fees;
        tx.execute("UPDATE meta SET value = ?1 WHERE key = 'burned'", [&burned.to_le_bytes()[..]])?;
        if self.fault == Some(Fault::BeforeCommit) {
            std::process::abort();
        }
        tx.commit()?;
        if self.fault == Some(Fault::AfterCommit) {
            std::process::abort();
        }
        let id = v.id;
        self.ledger.commit(v);
        Ok(id)
    }
}

fn connect(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open(path)?;
    // Same durability stance as the v1 node: one writer, fsync on commit.
    conn.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;")?;
    Ok(conn)
}

fn write_effects(tx: &rusqlite::Transaction<'_>, v: &Validated, first_position: u64) -> Result<(), StoreError> {
    for nf in v.nullifiers() {
        // PRIMARY KEY makes a repeated nullifier a hard SQL error as well.
        tx.execute("INSERT INTO nullifiers (nf, height) VALUES (?1, ?2)", params![&nf[..], v.height as i64])?;
    }
    for (i, cmx) in v.commitments().enumerate() {
        tx.execute(
            "INSERT INTO commitments (position, cmx, height) VALUES (?1, ?2, ?3)",
            params![(first_position + i as u64) as i64, &cmx[..], v.height as i64],
        )?;
    }
    tx.execute("INSERT INTO roots (height, root) VALUES (?1, ?2)", params![v.height as i64, &v.root[..]])?;
    Ok(())
}

fn write_tip(tx: &rusqlite::Transaction<'_>, ledger: &Ledger) -> Result<(), StoreError> {
    tx.execute("INSERT INTO meta VALUES ('tip', ?1)", [&ledger.tip()[..]])?;
    tx.execute("INSERT INTO meta VALUES ('height', ?1)", [&ledger.height().to_le_bytes()[..]])?;
    tx.execute("INSERT INTO meta VALUES ('burned', ?1)", [&ledger.burned().to_le_bytes()[..]])?;
    Ok(())
}

fn ledger_genesis_effects(ledger: &Ledger, genesis_tx: &[u8]) -> Result<Validated, StoreError> {
    let env = crate::Envelope::decode(genesis_tx).map_err(LedgerError::from)?;
    Ok(Validated {
        height: 0,
        id: ledger.tip(),
        block: Block { height: 0, prev: [0; 32], txs: vec![genesis_tx.to_vec()] },
        envs: vec![env],
        fees: 0,
        root: *ledger.recent_roots().last().expect("genesis root"),
    })
}

fn genesis_commitments(genesis_tx: &[u8]) -> Result<Vec<[u8; 32]>, StoreError> {
    let env = crate::Envelope::decode(genesis_tx).map_err(LedgerError::from)?;
    Ok(env.bundle.actions().iter().map(|a| a.cmx().to_bytes()).collect())
}

fn ledger_nullifiers(ledger: &Ledger) -> Vec<[u8; 32]> {
    ledger.nullifier_set().copied().collect()
}

fn meta(conn: &Connection, key: &str) -> Result<Vec<u8>, StoreError> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0))
        .optional()?
        .ok_or(StoreError::Corrupt("missing metadata"))
}

fn column<T: rusqlite::types::FromSql>(conn: &Connection, sql: &str) -> Result<Vec<T>, StoreError> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn encode_params(p: &Params) -> Vec<u8> {
    let mut out = Vec::with_capacity(52);
    out.extend_from_slice(&p.salt);
    out.extend_from_slice(&p.supply.to_le_bytes());
    out.extend_from_slice(&p.anchor_window.to_le_bytes());
    out.extend_from_slice(&p.max_expiry_delta.to_le_bytes());
    out
}

fn decode_params(b: &[u8]) -> Result<Params, StoreError> {
    if b.len() != 52 {
        return Err(StoreError::Corrupt("params"));
    }
    Ok(Params {
        salt: b[..32].try_into().unwrap(),
        supply: u64::from_le_bytes(b[32..40].try_into().unwrap()),
        anchor_window: u32::from_le_bytes(b[40..44].try_into().unwrap()),
        max_expiry_delta: u64::from_le_bytes(b[44..52].try_into().unwrap()),
    })
}
