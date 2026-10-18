//! Prototype of the ORVEXA private v2 ledger rules (docs/PRIVATE_V2_SPEC.md).
//!
//! Scope: canonical v2 envelope, ORVEXA sighash and tx ID, stateless checks,
//! and an in-memory ledger that owns the note commitment tree, the anchor
//! window, the nullifier set and burned-fee accounting, applying blocks
//! atomically. Cryptography is entirely the upstream `orchard` crate
//! (Ironwood v3 bundles, `PostNu6_3` circuit); nothing here re-implements a
//! proof, a signature or a commitment.
//!
//! `store` keeps that state durably in SQLite with atomic block commits.
//!
//! Not in scope: networking, mempool, block authorship/consensus, RPC. This crate never touches the transparent v1 protocol.

pub mod envelope;
pub mod keys;
pub mod ledger;
pub mod store;
pub mod wallet;

pub use envelope::{Envelope, EnvelopeError};
pub use ledger::{Block, Ledger, LedgerError, Params, ProofCheck, Validated};
