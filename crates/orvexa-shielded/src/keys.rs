//! Circuit keys and randomness. The verifying key is built locally for the one
//! allowed circuit version; it is never taken from transaction input.

use orchard::circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use std::sync::OnceLock;

/// The only circuit version suite 1 accepts.
pub const CIRCUIT: OrchardCircuitVersion = OrchardCircuitVersion::PostNu6_3;

pub fn verifying_key() -> &'static VerifyingKey {
    static VK: OnceLock<VerifyingKey> = OnceLock::new();
    VK.get_or_init(|| VerifyingKey::build(CIRCUIT))
}

pub fn proving_key() -> &'static ProvingKey {
    static PK: OnceLock<ProvingKey> = OnceLock::new();
    PK.get_or_init(|| ProvingKey::build(CIRCUIT))
}

/// Operating-system CSPRNG.
pub fn os_rng() -> UnwrapErr<SysRng> {
    UnwrapErr(SysRng)
}
