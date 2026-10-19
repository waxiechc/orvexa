//! Evaluation stand for running the v2 shielded chain on several validators
//! with Malachite (Tendermint BFT) instead of one block producer.
//!
//! * [`ctx`] — the ORVEXA context: block ids as consensus values, Ed25519
//!   votes and proposals signed over domain-separated bytes bound to a chain id.
//! * [`sim`] — a deterministic simulator with virtual time that drives the
//!   Malachite core state machine for every validator and injects faults:
//!   latency, message loss, partitions, crashes with WAL replay, lagging
//!   validators that catch up by verified commit certificates, and Byzantine
//!   validators (equivocation, invalid blocks, forged sync answers).
//!
//! The scenarios live in `tests/scenarios.rs`.

pub mod ctx;
pub mod sim;
