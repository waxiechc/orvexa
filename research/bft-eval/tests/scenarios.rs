//! Fault scenarios for v2 on several validators (Malachite, Tendermint BFT).
//!
//! With n = 3f + 1 validators of equal weight the protocol promises:
//! * safety — honest validators never commit different blocks at one height,
//!   whatever the network does and with up to f Byzantine validators;
//! * liveness — new blocks keep coming while at least 2f + 1 honest
//!   validators can talk to each other.
//!
//! Every scenario checks safety at the end (`Sim::check_safety`): one decided
//! block per height, every honest chain linked and certified by more than 2/3
//! of the voting power, no invalid block committed, no honest double vote —
//! including across crashes and WAL replay.

use bft_eval::sim::{Behaviour, Config, Sim};

fn sim(validators: usize, seed: u64) -> Sim {
    Sim::new(Config { validators, seed, ..Config::default() })
}

fn heights(sim: &Sim) -> Vec<u64> {
    (0..sim.validators()).map(|i| sim.height(i)).collect()
}

#[track_caller]
fn assert_safe(sim: &Sim) {
    if let Err(e) = sim.check_safety() {
        panic!("safety violated at t={}ms, heights {:?}: {e}", sim.now, heights(sim));
    }
    assert!(sim.stats.consensus_errors.is_empty(), "consensus errors: {:?}", sim.stats.consensus_errors);
}

#[track_caller]
fn reach(sim: &mut Sim, height: u64, limit_ms: u64) {
    let ok = sim.run_until_height(height, limit_ms);
    assert!(ok, "did not reach height {height} within {limit_ms}ms: heights {:?}, t={}ms, {:?}", heights(sim), sim.now, sim.stats);
}

#[test]
fn four_validators_produce_blocks() {
    let mut s = sim(4, 1);
    reach(&mut s, 50, 60_000);
    assert_safe(&s);
    assert_eq!(s.stats.synced_blocks, 0, "nobody should need catch-up on a healthy network");
    assert_eq!(s.stats.evidence, 0);
    // Every validator takes its turn as proposer.
    let mut proposers: Vec<_> = s.payloads(0).iter().map(|p| String::from_utf8_lossy(p).split(' ').nth(3).unwrap().to_string()).collect();
    proposers.sort();
    proposers.dedup();
    assert_eq!(proposers.len(), 4, "proposers: {proposers:?}");
}

#[test]
fn one_and_seven_validators() {
    let mut solo = sim(1, 2);
    reach(&mut solo, 20, 30_000);
    assert_safe(&solo);

    let mut seven = sim(7, 3);
    reach(&mut seven, 20, 60_000);
    assert_safe(&seven);
}

#[test]
fn leader_crash_moves_to_next_round() {
    for seed in 1..=5 {
        let mut s = sim(4, seed);
        reach(&mut s, 5, 30_000);
        // Stop the validator that proposes the next height, before it proposes.
        let next = s.max_height() + 1;
        let leader = s.proposer(next, 0);
        s.crash(leader);
        let before = s.min_honest_height().max(next - 1);
        s.run_for(10_000);
        let live: Vec<u64> = (0..4).filter(|&i| i != leader).map(|i| s.height(i)).collect();
        assert!(live.iter().all(|&h| h >= before + 5), "seed {seed}: 3 of 4 must keep going, heights {:?}", heights(&s));
        s.restart(leader);
        let target = s.max_height() + 3;
        reach(&mut s, target, 30_000);
        assert_safe(&s);
    }
}

#[test]
fn two_of_four_down_halts_without_forking_then_resumes() {
    let mut s = sim(4, 4);
    reach(&mut s, 5, 30_000);
    s.crash(1);
    s.crash(2);
    s.run_for(2_000);
    let stalled = heights(&s);
    s.run_for(20_000);
    assert_eq!(heights(&s), stalled, "no quorum: nobody may commit");
    assert_safe(&s);
    s.restart(1);
    s.restart(2);
    let target = s.max_height() + 5;
    reach(&mut s, target, 60_000);
    assert_safe(&s);
}

#[test]
fn even_split_halts_both_sides_and_heals() {
    for seed in 1..=5 {
        let mut s = sim(4, seed);
        reach(&mut s, 5, 30_000);
        s.partition(&[&[0, 1], &[2, 3]]);
        s.run_for(2_000);
        let stalled = heights(&s);
        s.run_for(20_000);
        assert_eq!(heights(&s), stalled, "seed {seed}: 2 of 4 is not a quorum on either side");
        assert_safe(&s);
        s.heal();
        let target = s.max_height() + 5;
        reach(&mut s, target, 60_000);
        assert_safe(&s);
    }
}

#[test]
fn majority_side_continues_minority_catches_up() {
    for seed in 1..=5 {
        let mut s = sim(4, seed);
        reach(&mut s, 3, 30_000);
        s.partition(&[&[0, 1, 2], &[3]]);
        let isolated = s.height(3);
        s.run_for(15_000);
        assert!(s.height(3) <= isolated + 1, "seed {seed}: a lone validator cannot commit");
        assert!(s.height(0) >= isolated + 20, "seed {seed}: 3 of 4 keep going, heights {:?}", heights(&s));
        s.heal();
        let target = s.max_height() + 2;
        reach(&mut s, target, 30_000);
        assert!(s.stats.synced_blocks > 0, "the minority validator caught up by certificates");
        assert_safe(&s);
    }
}

#[test]
fn lagging_validator_catches_up_by_certificates() {
    let mut s = sim(4, 7);
    reach(&mut s, 3, 30_000);
    s.crash(2);
    s.run_for(20_000);
    assert!(s.height(0) >= 40, "heights {:?}", heights(&s));
    s.restart(2);
    let target = s.max_height() + 2;
    reach(&mut s, target, 30_000);
    assert!(s.stats.synced_blocks >= 30, "{:?}", s.stats);
    assert_safe(&s);
}

#[test]
fn everyone_crashes_at_once_and_recovers_from_the_wal() {
    // Different offsets hit every step of a round: propose, prevote, precommit, commit.
    for offset in (0..600).step_by(23) {
        let mut s = sim(4, 100 + offset);
        reach(&mut s, 3, 30_000);
        s.run_for(offset);
        for i in 0..4 {
            s.crash(i);
        }
        s.run_for(1_000);
        for i in 0..4 {
            s.restart(i);
        }
        let target = s.max_height() + 3;
        reach(&mut s, target, 60_000);
        assert_safe(&s);
    }
}

#[test]
fn crash_restart_storm() {
    for seed in 1..=8 {
        let mut s = sim(4, seed);
        let mut down: Option<usize> = None;
        for step in 0..60u64 {
            s.run_for(97 + (seed * 31 + step * 17) % 400);
            match down.take() {
                Some(i) => s.restart(i),
                None => {
                    let i = ((seed + step) % 4) as usize;
                    s.crash(i);
                    down = Some(i);
                }
            }
        }
        if let Some(i) = down {
            s.restart(i);
        }
        let target = s.max_height() + 3;
        reach(&mut s, target, 60_000);
        assert_safe(&s);
        assert!(s.min_honest_height() >= 10, "seed {seed}: {:?}", heights(&s));
    }
}

#[test]
fn lossy_slow_network() {
    for seed in 1..=10 {
        let mut s = Sim::new(Config { validators: 4, seed, latency_ms: (5, 250), drop_permille: 200, ..Config::default() });
        reach(&mut s, 15, 180_000);
        assert_safe(&s);
    }
}

#[test]
fn equivocating_validator_cannot_fork_the_chain() {
    for seed in 1..=6 {
        for byzantine in 0..4 {
            let mut s = Sim::new(Config { validators: 4, seed, behaviours: vec![(byzantine, Behaviour::Equivocate)], ..Config::default() });
            reach(&mut s, 12, 120_000);
            assert_safe(&s);
        }
    }
}

#[test]
fn equivocation_is_reported_as_evidence() {
    let mut s = Sim::new(Config { validators: 4, seed: 11, behaviours: vec![(0, Behaviour::Equivocate)], latency_ms: (1, 5), ..Config::default() });
    reach(&mut s, 12, 120_000);
    assert_safe(&s);
    assert!(s.stats.evidence > 0, "honest validators should have recorded the double votes: {:?}", s.stats);
}

#[test]
fn two_equivocators_among_seven() {
    for seed in 1..=4 {
        let mut s = Sim::new(Config {
            validators: 7,
            seed,
            behaviours: vec![(1, Behaviour::Equivocate), (4, Behaviour::Equivocate)],
            ..Config::default()
        });
        reach(&mut s, 10, 120_000);
        assert_safe(&s);
    }
}

#[test]
fn invalid_blocks_are_never_committed() {
    for seed in 1..=4 {
        let mut s = Sim::new(Config { validators: 4, seed, behaviours: vec![(2, Behaviour::InvalidBlocks)], ..Config::default() });
        reach(&mut s, 20, 120_000);
        assert_safe(&s);
        for i in [0, 1, 3] {
            assert!(s.payloads(i).iter().all(|p| !p.starts_with(b"invalid")));
        }
    }
}

#[test]
fn forged_catch_up_answers_are_rejected() {
    let mut s = Sim::new(Config { validators: 4, seed: 5, behaviours: vec![(0, Behaviour::ForgeSync)], ..Config::default() });
    reach(&mut s, 3, 30_000);
    s.crash(3);
    s.run_for(10_000);
    s.restart(3);
    let target = s.max_height() + 2;
    reach(&mut s, target, 60_000);
    assert_safe(&s);
    assert!(s.stats.synced_blocks > 0);
    assert!(s.stats.rejected_sync > 0, "the forger should have been asked and refused: {:?}", s.stats);
    assert!(s.payloads(3).iter().all(|p| !p.starts_with(b"forged")));
}

#[test]
fn byzantine_plus_partition_plus_crashes() {
    for seed in 1..=4 {
        let mut s = Sim::new(Config {
            validators: 7,
            seed,
            latency_ms: (5, 120),
            drop_permille: 50,
            behaviours: vec![(6, Behaviour::Equivocate)],
            ..Config::default()
        });
        reach(&mut s, 5, 60_000);
        s.partition(&[&[0, 1, 2], &[3, 4, 5, 6]]);
        s.crash(0);
        s.run_for(8_000);
        s.restart(0);
        s.heal();
        s.crash(4);
        s.run_for(8_000);
        s.restart(4);
        let target = s.max_height() + 3;
        reach(&mut s, target, 120_000);
        assert_safe(&s);
    }
}

/// Self-test of the checker: validators misconfigured to accept a 1/3 quorum
/// fork under an even split, and the observer must notice.
#[test]
fn checker_detects_a_fork_when_the_quorum_is_too_small() {
    use malachitebft_core_types::ThresholdParam;
    let mut s = Sim::new(Config { validators: 4, seed: 9, quorum: ThresholdParam::new(1, 3), ..Config::default() });
    reach(&mut s, 3, 30_000);
    s.partition(&[&[0, 1], &[2, 3]]);
    s.run_for(10_000);
    let err = s.check_safety().expect_err("both halves committed on their own");
    assert!(err.contains("conflicting decisions") || err.contains("without a valid certificate"), "{err}");
}

/// Self-test of the double-vote check: validators that forget their WAL on
/// restart re-vote in rounds they already voted in, and the observer must
/// notice. This is also why the WAL has to be on disk before a vote leaves.
#[test]
fn checker_detects_double_votes_without_the_wal() {
    let mut caught = false;
    for offset in (0..600).step_by(23) {
        let mut s = Sim::new(Config { validators: 4, seed: 100 + offset, replay_wal: false, ..Config::default() });
        reach(&mut s, 3, 30_000);
        s.run_for(offset);
        for i in 0..4 {
            s.crash(i);
        }
        s.run_for(1_000);
        for i in 0..4 {
            s.restart(i);
        }
        s.run_for(5_000);
        if let Err(e) = s.check_safety() {
            assert!(e.contains("signed two"), "{e}");
            caught = true;
            break;
        }
    }
    assert!(caught, "forgetting the WAL should lead to a double vote at some crash point");
}

/// Mixed faults for one seed: a Byzantine validator (or none), loss, jitter,
/// partitions, crashes and restarts, then a check that the chain resumes.
fn chaos(seed: u64) {
    let byzantine = (seed % 4) as usize;
    let behaviour = [Behaviour::Equivocate, Behaviour::InvalidBlocks, Behaviour::ForgeSync, Behaviour::Honest][(seed / 4 % 4) as usize];
    let mut s = Sim::new(Config {
        validators: 4,
        seed,
        latency_ms: (1, 10 + seed % 200),
        drop_permille: (seed % 5) as u32 * 50,
        behaviours: vec![(byzantine, behaviour)],
        ..Config::default()
    });
    let honest: Vec<usize> = (0..4).filter(|&i| i != byzantine).collect();
    for step in 0..30u64 {
        s.run_for(150 + (seed * 13 + step * 29) % 700);
        match (seed + step) % 5 {
            0 => s.partition(&[&[honest[0]], &[honest[1], honest[2], byzantine]]),
            1 => s.heal(),
            2 => s.crash(honest[(step % 3) as usize]),
            3 => {
                for &i in &honest {
                    if !s.is_running(i) {
                        s.restart(i);
                    }
                }
            }
            _ => {}
        }
    }
    s.heal();
    for &i in &honest {
        if !s.is_running(i) {
            s.restart(i);
        }
    }
    let target = s.max_height() + 2;
    reach(&mut s, target, 300_000);
    if let Err(e) = s.check_safety() {
        panic!("seed {seed}: {e}");
    }
    assert!(s.stats.consensus_errors.is_empty(), "seed {seed}: {:?}", s.stats.consensus_errors);
}

/// Seed 381 once made a restarted validator stop with `ReplayDivergence`: a
/// propose timeout of an already committed height reached the WAL of the next
/// height and was replayed as that height's timeout. The WAL is now per height.
#[test]
fn chaos_regressions_and_first_seeds() {
    for seed in [381].into_iter().chain(1..=60) {
        chaos(seed);
    }
}

/// Long sweep, not run by default: `BFT_SEEDS=2000 cargo test --release -- --ignored`.
#[test]
#[ignore]
fn stress_many_seeds() {
    let seeds: u64 = std::env::var("BFT_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(200);
    for seed in 1..=seeds {
        chaos(seed);
    }
}
