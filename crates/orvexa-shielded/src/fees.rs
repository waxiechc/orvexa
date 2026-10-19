//! Recommended fee: a wallet and node policy, not a ledger rule (the ledger
//! only requires at least 1 atom). A transfer pays per action, because
//! actions are what every validator has to verify and every wallet has to
//! scan; the rate goes up while many transfers are waiting for a block.

use serde::{Deserialize, Serialize};

/// Atoms per action. A plain transfer has 2 actions: 10 atoms.
pub const PER_ACTION: u64 = 5;
/// Orchard bundles are padded to at least this many actions.
pub const MIN_ACTIONS: usize = 2;

/// Multiplier for a queue of `mempool` waiting transfers.
pub fn congestion(mempool: usize) -> u64 {
    match mempool {
        0..=49 => 1,
        50..=199 => 2,
        _ => 4,
    }
}

/// Fee for a transfer with `actions` actions at congestion `multiplier`.
pub fn recommended(actions: usize, multiplier: u64) -> u64 {
    PER_ACTION
        .saturating_mul(actions.max(MIN_ACTIONS) as u64)
        .saturating_mul(multiplier.clamp(1, 4))
}

/// What a node reports at `GET /v2/fee`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeeConditions {
    pub per_action: u64,
    pub min_actions: usize,
    pub mempool: usize,
    pub congestion: u64,
}

impl FeeConditions {
    pub fn for_mempool(mempool: usize) -> Self {
        FeeConditions {
            per_action: PER_ACTION,
            min_actions: MIN_ACTIONS,
            mempool,
            congestion: congestion(mempool),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_grows_with_actions_and_queue() {
        assert_eq!(recommended(1, 1), 10, "padded to two actions");
        assert_eq!(recommended(2, 1), 10);
        assert_eq!(recommended(5, 1), 25);
        assert_eq!(recommended(2, congestion(60)), 20);
        assert_eq!(recommended(2, congestion(500)), 40);
        assert_eq!(recommended(2, 99), 40, "a node cannot ask for more than 4×");
    }
}
