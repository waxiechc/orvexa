//! Several validators instead of one block producer: Tendermint BFT through
//! Malachite (`arc-malachitebft-core-consensus` 0.8.1). Evaluated with fault
//! scenarios in `research/bft-eval`. Malachite is alpha and not externally
//! audited: acceptable for the test network, not for real value.

pub mod codec;
pub mod ctx;
pub mod engine;
