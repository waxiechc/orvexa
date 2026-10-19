//! Deterministic simulator of a v2 validator network on Malachite.
//!
//! Every validator runs the unmodified Malachite core state machine; the
//! simulator plays the host around it: it answers effects (signing,
//! verification, timers, WAL, publishing), gossips block bodies, delivers
//! messages through a virtual network and commits decided blocks. Time is
//! virtual and every random choice comes from one seed, so a failing run
//! repeats exactly.
//!
//! Faults: per-message latency and loss, network partitions, crashes (all
//! volatile state is lost; the chain, the WAL and stored block bodies stay),
//! restarts with WAL replay, and Byzantine validators. A validator that fell
//! behind catches up by asking peers for `(block, commit certificate)` pairs
//! and accepts one only after checking the certificate against the validator
//! set — the same check a non-validating follower would run.
//!
//! The observer records every decision and every vote an honest validator
//! signs, so the tests can assert safety: one block per height across all
//! honest validators, no honest double vote, valid certificates only.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::time::Duration;

use futures::executor::block_on;
use malachitebft_core_consensus::{
    process, ConsensusMsg, Effect, Error as ConsensusError, Input, LivenessMsg, LocallyProposedValue,
    Params, ProposedValue, Resumable, Resume, SignedConsensusMsg, State, ValuePayload,
    VoteExtensionError,
};
use malachitebft_core_types::{
    CommitCertificate, LinearTimeouts, NilOrVal, Round, ThresholdParam, ThresholdParams, Timeout, TimeoutKind,
    Validity, ValueOrigin, VoteExtensionPolicy, VoteType,
};
use malachitebft_signing::VerifierExt;

use crate::ctx::{Address, Block, BlockId, Crypto, Height, OrvexaCtx, PrivateKey, Proposal, Validator, ValidatorSet, Vote};

pub type Cert = CommitCertificate<OrvexaCtx>;

/// How a validator behaves. Everything but `Honest` is Byzantine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behaviour {
    Honest,
    /// Proposes two different blocks to the two halves of the network and
    /// sends conflicting votes to each half.
    Equivocate,
    /// Proposes blocks that break the application rules.
    InvalidBlocks,
    /// Answers catch-up requests with a forged block under a real certificate.
    ForgeSync,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub validators: usize,
    pub seed: u64,
    /// One-way delay, inclusive range in milliseconds.
    pub latency_ms: (u64, u64),
    /// Probability of losing a message, per mille.
    pub drop_permille: u32,
    pub behaviours: Vec<(usize, Behaviour)>,
    /// Pause between committing a block and starting the next height.
    pub block_interval_ms: u64,
    /// Quorum the validators run with. Only the self-test of the safety
    /// checker changes it; the observer always judges by more than 2/3.
    pub quorum: ThresholdParam,
    /// Replay the WAL on restart. Only the self-test of the double-vote
    /// check turns it off.
    pub replay_wal: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self { validators: 4, seed: 1, latency_ms: (5, 30), drop_permille: 0, behaviours: Vec::new(), block_interval_ms: 100, quorum: ThresholdParam::TWO_F_PLUS_ONE, replay_wal: true }
    }
}

/// What a proposer gossips next to its signed proposal: the block body
/// (the "parts" of `ProposalAndParts`).
#[derive(Clone, Debug)]
pub struct BodyMsg {
    pub height: u64,
    pub round: Round,
    pub pol_round: Round,
    pub proposer: Address,
    pub block: Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Header {
    height: u64,
    round: Round,
    pol_round: Round,
    proposer: Address,
    id: BlockId,
}

#[derive(Clone, Debug)]
enum Msg {
    Consensus(SignedConsensusMsg<OrvexaCtx>),
    Liveness(LivenessMsg<OrvexaCtx>),
    Body(BodyMsg),
    Status(u64),
    SyncRequest(u64),
    SyncResponse(Block, Cert),
}

#[derive(Debug)]
enum Event {
    Deliver { from: usize, to: usize, msg: Box<Msg> },
    Timer { node: usize, life: u64, token: u64, timeout: Timeout },
    Propose { node: usize, life: u64, height: u64, round: Round },
    StartHeight { node: usize, life: u64, height: u64 },
    StatusTick { node: usize, life: u64 },
}

/// What survives a crash: committed chain, the WAL of the current height,
/// received block bodies above the tip and this validator's own proposals.
#[derive(Default)]
struct Disk {
    chain: Vec<(Block, Cert)>,
    /// Entries are kept per height: a late input for a height that is
    /// already committed must not be replayed as if it belonged to the next one.
    wal: Vec<(u64, Input<OrvexaCtx>)>,
    bodies: BTreeMap<BlockId, Block>,
    headers: Vec<Header>,
    proposed: BTreeMap<(u64, i64), BlockId>,
}

impl Disk {
    fn tip(&self) -> u64 {
        self.chain.len() as u64
    }

    fn tip_id(&self, crypto: &Crypto) -> [u8; 32] {
        self.chain.last().map(|(b, _)| b.id(&crypto.chain_id).0).unwrap_or([0; 32])
    }

    fn commit(&mut self, block: Block, cert: Cert) {
        let height = block.height;
        self.chain.push((block, cert));
        self.wal.retain(|(h, _)| *h > height);
        self.bodies.retain(|_, b| b.height > height);
        self.headers.retain(|h| h.height > height);
        self.proposed.retain(|(h, _), _| *h > height);
    }
}

enum Out {
    Broadcast(Msg),
    SendTo(Vec<usize>, Msg),
    Timer(Timeout, u64, u64),
    Propose(u64, Round),
    Finalized(Block, Cert),
    Signed(VoteType, u64, Round, NilOrVal<BlockId>),
    Evidence,
}

/// Effect handler of one validator incarnation.
struct Io {
    index: usize,
    n: usize,
    behaviour: Behaviour,
    key: PrivateKey,
    crypto: Crypto,
    timeouts: LinearTimeouts,
    timers: HashMap<Timeout, u64>,
    next_token: u64,
    replaying: bool,
    /// Equivocation: the two blocks proposed at (height, round).
    alt: BTreeMap<(u64, i64), (BlockId, BlockId)>,
    out: Vec<Out>,
}

impl Io {
    fn halves(&self) -> (Vec<usize>, Vec<usize>) {
        (0..self.n).filter(|&i| i != self.index).partition(|i| i % 2 == 0)
    }

    fn duration(&self, timeout: Timeout) -> u64 {
        match timeout.kind {
            TimeoutKind::FinalizeHeight(d) => d.as_millis() as u64,
            _ => self.timeouts.duration_for(timeout).as_millis() as u64,
        }
    }

    fn publish(&mut self, msg: SignedConsensusMsg<OrvexaCtx>) {
        if self.behaviour != Behaviour::Equivocate {
            self.out.push(Out::Broadcast(Msg::Consensus(msg)));
            return;
        }
        let (a, b) = self.halves();
        let twin = match &msg {
            SignedConsensusMsg::Proposal(p) => {
                let key = (p.height.0, p.round.as_i64());
                self.alt.get(&key).map(|&(_, other)| {
                    let forged = Proposal { value: other, ..p.message.clone() };
                    SignedConsensusMsg::Proposal(self.crypto.sign_proposal(&self.key, forged))
                })
            }
            SignedConsensusMsg::Vote(v) => {
                let key = (v.height.0, v.round.as_i64());
                let flipped = match (&v.value, self.alt.get(&key)) {
                    (NilOrVal::Val(x), Some(&(p, q))) => NilOrVal::Val(if *x == p { q } else { p }),
                    (NilOrVal::Val(_), None) => NilOrVal::Nil,
                    (NilOrVal::Nil, Some(&(p, _))) => NilOrVal::Val(p),
                    (NilOrVal::Nil, None) => NilOrVal::Nil,
                };
                let forged = Vote { value: flipped, ..v.message.clone() };
                Some(SignedConsensusMsg::Vote(self.crypto.sign_vote(&self.key, forged)))
            }
        };
        match twin {
            Some(twin) => {
                self.out.push(Out::SendTo(a, Msg::Consensus(msg)));
                self.out.push(Out::SendTo(b, Msg::Consensus(twin)));
            }
            None => self.out.push(Out::Broadcast(Msg::Consensus(msg))),
        }
    }

    fn handle(&mut self, effect: Effect<OrvexaCtx>, disk: &mut Disk) -> Result<Resume<OrvexaCtx>, ()> {
        let ctx = OrvexaCtx;
        Ok(match effect {
            Effect::CancelAllTimeouts(r) => {
                self.timers.clear();
                r.resume_with(())
            }
            Effect::CancelTimeout(t, r) => {
                self.timers.remove(&t);
                r.resume_with(())
            }
            Effect::ScheduleTimeout(t, r) => {
                self.next_token += 1;
                self.timers.insert(t, self.next_token);
                self.out.push(Out::Timer(t, self.next_token, self.duration(t)));
                r.resume_with(())
            }
            Effect::StartRound(_, _, _, _, r) => r.resume_with(()),
            Effect::PublishConsensusMsg(msg, r) => {
                self.publish(msg);
                r.resume_with(())
            }
            Effect::PublishLivenessMsg(msg, r) => {
                self.out.push(Out::Broadcast(Msg::Liveness(msg)));
                r.resume_with(())
            }
            Effect::RepublishVote(vote, r) => {
                self.out.push(Out::Broadcast(Msg::Liveness(LivenessMsg::Vote(vote))));
                r.resume_with(())
            }
            Effect::RepublishRoundCertificate(cert, r) => {
                self.out.push(Out::Broadcast(Msg::Liveness(LivenessMsg::SkipRoundCertificate(cert))));
                r.resume_with(())
            }
            Effect::GetValue(height, round, _, r) => {
                self.out.push(Out::Propose(height.0, round));
                r.resume_with(())
            }
            Effect::RestreamProposal(height, round, pol_round, proposer, id, r) => {
                if let Some(block) = disk.bodies.get(&id) {
                    let body = BodyMsg { height: height.0, round, pol_round, proposer, block: block.clone() };
                    self.out.push(Out::Broadcast(Msg::Body(body)));
                }
                r.resume_with(())
            }
            Effect::CertVerifiedSyncValue(_, _, r) => r.resume_with(()),
            Effect::CertRejectedSyncValue(_, _, _, r) => r.resume_with(()),
            Effect::Decide(_, _, r) => r.resume_with(()),
            Effect::Finalize(cert, _, evidence, r) => {
                if !evidence.is_empty() {
                    self.out.push(Out::Evidence);
                }
                let block = disk.bodies.get(&cert.value_id).cloned().expect("decided block body is stored");
                disk.commit(block.clone(), cert.clone());
                self.out.push(Out::Finalized(block, cert));
                r.resume_with(())
            }
            Effect::SignVote(vote, r) => {
                self.out.push(Out::Signed(vote.typ, vote.height.0, vote.round, vote.value));
                r.resume_with(self.crypto.sign_vote(&self.key, vote))
            }
            Effect::SignProposal(proposal, r) => r.resume_with(self.crypto.sign_proposal(&self.key, proposal)),
            Effect::VerifySignature(msg, key, r) => {
                let ok = match &msg.message {
                    ConsensusMsg::Vote(v) => self.crypto.vote_ok(v, &msg.signature, &key),
                    ConsensusMsg::Proposal(p) => self.crypto.proposal_ok(p, &msg.signature, &key),
                };
                r.resume_with(ok)
            }
            Effect::VerifyCommitCertificate(cert, vs, th, r) => {
                r.resume_with(block_on(self.crypto.verify_commit_certificate(&ctx, &cert, &vs, th)))
            }
            Effect::VerifyExtendedCommitCertificate(cert, vs, th, policy, r) => {
                r.resume_with(block_on(self.crypto.verify_extended_commit_certificate(&ctx, &cert, &vs, th, policy)))
            }
            Effect::VerifyPolkaCertificate(cert, vs, th, r) => {
                r.resume_with(block_on(self.crypto.verify_polka_certificate(&ctx, &cert, &vs, th)))
            }
            Effect::VerifyRoundCertificate(cert, vs, th, r) => {
                r.resume_with(block_on(self.crypto.verify_round_certificate(&ctx, &cert, &vs, th)))
            }
            Effect::WalAppend(height, input, r) => {
                if !self.replaying && height.0 > disk.tip() {
                    disk.wal.push((height.0, input));
                }
                r.resume_with(())
            }
            Effect::ExtendVote(_, _, _, _, r) => r.resume_with(None),
            Effect::VerifyVoteExtension(_, _, _, _, _, _, r) => r.resume_with(Err(VoteExtensionError::InvalidVoteExtension)),
        })
    }
}

struct Node {
    behaviour: Behaviour,
    address: Address,
    alive: bool,
    life: u64,
    disk: Disk,
    cons: Option<State<OrvexaCtx>>,
    io: Io,
    peer_tips: BTreeMap<usize, u64>,
    banned: BTreeSet<usize>,
    sync_asked: Option<(u64, u64)>,
    produced: u64,
}

struct Queued(u64, u64, Event);

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        (self.0, self.1) == (other.0, other.1)
    }
}
impl Eq for Queued {}
impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.0, self.1).cmp(&(other.0, other.1))
    }
}

/// SplitMix64: small, seeded, reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub delivered: u64,
    pub dropped: u64,
    /// Finalizations at which a validator reported misbehaviour evidence.
    pub evidence: u64,
    /// Catch-up answers refused because the block or certificate did not check out.
    pub rejected_sync: u64,
    /// Blocks applied through catch-up instead of the validator's own decision.
    pub synced_blocks: u64,
    pub consensus_errors: Vec<String>,
}

pub struct Sim {
    pub now: u64,
    seq: u64,
    queue: BinaryHeap<Reverse<Queued>>,
    rng: Rng,
    nodes: Vec<Node>,
    vs: ValidatorSet,
    crypto: Crypto,
    latency: (u64, u64),
    drop_permille: u32,
    groups: Vec<u32>,
    block_interval: u64,
    thresholds: ThresholdParams,
    replay_wal: bool,
    decided: BTreeMap<u64, BlockId>,
    signed: BTreeMap<(usize, u8, u64, i64), NilOrVal<BlockId>>,
    violations: Vec<String>,
    pub stats: Stats,
}

const STATUS_EVERY_MS: u64 = 200;
const SYNC_RETRY_MS: u64 = 400;

pub fn timeouts() -> LinearTimeouts {
    LinearTimeouts {
        propose: Duration::from_millis(400),
        propose_delta: Duration::from_millis(200),
        prevote: Duration::from_millis(200),
        prevote_delta: Duration::from_millis(100),
        precommit: Duration::from_millis(200),
        precommit_delta: Duration::from_millis(100),
        rebroadcast: Duration::from_millis(1000),
        max_timeout: Duration::from_secs(10),
    }
}

impl Sim {
    pub fn new(config: Config) -> Self {
        let keys: Vec<PrivateKey> = (0..config.validators)
            .map(|i| {
                let mut seed = [0u8; 32];
                seed[..8].copy_from_slice(&(i as u64 + 1).to_be_bytes());
                seed[8..16].copy_from_slice(&config.seed.to_be_bytes());
                PrivateKey::from(seed)
            })
            .collect();
        let vs = ValidatorSet::new(keys.iter().map(|k| Validator::new(k.public_key(), 1)).collect());
        let crypto = Crypto { chain_id: vs.chain_id("orvexa-v2-bft-eval") };
        let n = config.validators;
        let nodes = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| {
                let behaviour = config.behaviours.iter().find(|(i, _)| *i == index).map(|(_, b)| *b).unwrap_or(Behaviour::Honest);
                Node {
                    behaviour,
                    address: Address(*key.public_key().as_bytes()),
                    io: Io {
                        index,
                        n,
                        behaviour,
                        key,
                        crypto: crypto.clone(),
                        timeouts: timeouts(),
                        timers: HashMap::new(),
                        next_token: 0,
                        replaying: false,
                        alt: BTreeMap::new(),
                        out: Vec::new(),
                    },
                    alive: false,
                    life: 0,
                    disk: Disk::default(),
                    cons: None,
                    peer_tips: BTreeMap::new(),
                    banned: BTreeSet::new(),
                    sync_asked: None,
                    produced: 0,
                }
            })
            .collect();
        let mut sim = Sim {
            now: 0,
            seq: 0,
            queue: BinaryHeap::new(),
            rng: Rng(config.seed),
            nodes,
            vs,
            crypto,
            latency: config.latency_ms,
            drop_permille: config.drop_permille,
            groups: vec![0; n],
            block_interval: config.block_interval_ms,
            thresholds: ThresholdParams { quorum: config.quorum, ..ThresholdParams::default() },
            replay_wal: config.replay_wal,
            decided: BTreeMap::new(),
            signed: BTreeMap::new(),
            violations: Vec::new(),
            stats: Stats::default(),
        };
        for i in 0..n {
            sim.boot(i, false);
        }
        sim
    }

    // ----- control -----

    pub fn validators(&self) -> usize {
        self.nodes.len()
    }

    pub fn run_for(&mut self, ms: u64) {
        let until = self.now + ms;
        while let Some(Reverse(Queued(t, _, _))) = self.queue.peek() {
            if *t > until {
                break;
            }
            let Reverse(Queued(t, _, event)) = self.queue.pop().unwrap();
            self.now = t;
            self.dispatch(event);
        }
        self.now = until;
    }

    /// Runs until every honest live validator reached `height` or `limit_ms` passed.
    pub fn run_until_height(&mut self, height: u64, limit_ms: u64) -> bool {
        let until = self.now + limit_ms;
        while self.now < until {
            self.run_for(50);
            if self.live_honest().all(|i| self.height(i) >= height) {
                return true;
            }
        }
        false
    }

    pub fn crash(&mut self, i: usize) {
        let node = &mut self.nodes[i];
        node.alive = false;
        node.cons = None;
        node.io.timers.clear();
        node.io.alt.clear();
        node.peer_tips.clear();
        node.sync_asked = None;
    }

    pub fn restart(&mut self, i: usize) {
        assert!(!self.nodes[i].alive, "validator {i} is running");
        if !self.replay_wal {
            // A node without a WAL also forgets which block it proposed.
            self.nodes[i].disk.wal.clear();
            self.nodes[i].disk.proposed.clear();
        }
        self.boot(i, true);
    }

    /// Splits the network: messages only pass inside a group. Validators not
    /// listed form one more group.
    pub fn partition(&mut self, groups: &[&[usize]]) {
        self.groups = vec![u32::MAX; self.nodes.len()];
        for (g, members) in groups.iter().enumerate() {
            for &m in members.iter() {
                self.groups[m] = g as u32;
            }
        }
    }

    pub fn heal(&mut self) {
        self.groups = vec![0; self.nodes.len()];
    }

    pub fn set_drop(&mut self, permille: u32) {
        self.drop_permille = permille;
    }

    pub fn set_latency(&mut self, lo: u64, hi: u64) {
        self.latency = (lo, hi);
    }

    // ----- observation -----

    pub fn height(&self, i: usize) -> u64 {
        self.nodes[i].disk.tip()
    }

    pub fn is_running(&self, i: usize) -> bool {
        self.nodes[i].alive
    }

    pub fn is_honest(&self, i: usize) -> bool {
        self.nodes[i].behaviour == Behaviour::Honest
    }

    fn live_honest(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(|&i| self.nodes[i].alive && self.is_honest(i))
    }

    pub fn min_honest_height(&self) -> u64 {
        (0..self.nodes.len()).filter(|&i| self.is_honest(i)).map(|i| self.height(i)).min().unwrap_or(0)
    }

    pub fn max_height(&self) -> u64 {
        (0..self.nodes.len()).map(|i| self.height(i)).max().unwrap_or(0)
    }

    /// Validator index proposing `height` in `round`.
    pub fn proposer(&self, height: u64, round: u32) -> usize {
        let address = OrvexaCtx::proposer(&self.vs, Height(height), Round::new(round)).address;
        self.nodes.iter().position(|n| n.address == address).unwrap()
    }

    pub fn payloads(&self, i: usize) -> Vec<Vec<u8>> {
        self.nodes[i].disk.chain.iter().map(|(b, _)| b.payload.clone()).collect()
    }

    /// Checks safety over everything observed so far.
    pub fn check_safety(&self) -> Result<(), String> {
        if let Some(v) = self.violations.first() {
            return Err(format!("{} violation(s), first: {v}", self.violations.len()));
        }
        let honest: Vec<usize> = (0..self.nodes.len()).filter(|&i| self.is_honest(i)).collect();
        for &i in &honest {
            let chain = &self.nodes[i].disk.chain;
            let mut prev = [0u8; 32];
            for (k, (block, cert)) in chain.iter().enumerate() {
                let id = block.id(&self.crypto.chain_id);
                if block.height != k as u64 + 1 || block.prev != prev {
                    return Err(format!("validator {i}: block {} does not extend its chain", k + 1));
                }
                if cert.value_id != id || cert.height != Height(block.height) {
                    return Err(format!("validator {i}: certificate of {} is for another block", k + 1));
                }
                block_on(self.crypto.verify_commit_certificate(&OrvexaCtx, cert, &self.vs, ThresholdParams::default()))
                    .map_err(|e| format!("validator {i}: certificate of {} does not verify: {e:?}", k + 1))?;
                if !self.valid_payload(block) {
                    return Err(format!("validator {i}: committed an invalid block at {}", k + 1));
                }
                if self.decided.get(&block.height) != Some(&id) {
                    return Err(format!("validator {i}: block {} differs from the decided one", k + 1));
                }
                prev = id.0;
            }
        }
        Ok(())
    }

    // ----- internals -----

    fn schedule(&mut self, delay: u64, event: Event) {
        self.seq += 1;
        self.queue.push(Reverse(Queued(self.now + delay, self.seq, event)));
    }

    fn valid_payload(&self, block: &Block) -> bool {
        !block.payload.starts_with(b"invalid") && self.vs.0.iter().any(|v| v.address == block.proposer)
    }

    fn boot(&mut self, i: usize, restart: bool) {
        let vs = self.vs.clone();
        let node = &mut self.nodes[i];
        node.alive = true;
        node.life += 1;
        node.io.timers.clear();
        node.io.out.clear();
        let height = Height(node.disk.tip() + 1);
        let params = Params {
            address: node.address,
            threshold_params: self.thresholds,
            value_payload: ValuePayload::ProposalAndParts,
            enabled: true,
        };
        node.cons = Some(State::new(OrvexaCtx, height, vs.clone(), params, 4096, 1024));
        self.feed(i, Input::StartHeight(height, vs, restart, None, VoteExtensionPolicy::Disabled));
        if restart && self.replay_wal {
            let wal: Vec<Input<OrvexaCtx>> = self.nodes[i].disk.wal.iter().map(|(_, input)| input.clone()).collect();
            if std::env::var("BFT_DEBUG").is_ok() {
                eprintln!("t={} restart v{i} tip={} wal:", self.now, self.nodes[i].disk.tip());
                for e in &wal {
                    eprintln!("   {}", describe(e));
                }
            }
            let node = &mut self.nodes[i];
            node.io.replaying = true;
            node.cons.as_mut().unwrap().index_wal_entries(wal.iter());
            for input in wal {
                self.feed(i, input);
            }
            let node = &mut self.nodes[i];
            node.cons.as_mut().unwrap().reset_entries_index();
            node.io.replaying = false;
        }
        self.offer_stored(i, height.0);
        let life = self.nodes[i].life;
        let jitter = self.rng.range(0, STATUS_EVERY_MS);
        self.schedule(jitter, Event::StatusTick { node: i, life });
    }

    fn feed(&mut self, i: usize, input: Input<OrvexaCtx>) {
        let node = &mut self.nodes[i];
        let Some(state) = node.cons.as_mut() else { return };
        let io = &mut node.io;
        let disk = &mut node.disk;
        let result: Result<(), ConsensusError<OrvexaCtx>> = process!(
            input: input,
            state: state,
            metrics: &(),
            with: effect => io.handle(effect, disk)
        );
        if let Err(e) = result {
            self.stats.consensus_errors.push(format!("t={} validator {i}: {e:?}", self.now));
        }
        let outs = std::mem::take(&mut self.nodes[i].io.out);
        for out in outs {
            self.apply(i, out);
        }
    }

    fn apply(&mut self, i: usize, out: Out) {
        let life = self.nodes[i].life;
        match out {
            Out::Broadcast(msg) => {
                for to in 0..self.nodes.len() {
                    if to != i {
                        self.send(i, to, msg.clone());
                    }
                }
            }
            Out::SendTo(targets, msg) => {
                for to in targets {
                    self.send(i, to, msg.clone());
                }
            }
            Out::Timer(timeout, token, delay) => self.schedule(delay, Event::Timer { node: i, life, token, timeout }),
            Out::Propose(height, round) => self.schedule(5, Event::Propose { node: i, life, height, round }),
            Out::Finalized(block, cert) => {
                self.observe_commit(i, &block, &cert);
                self.schedule(self.block_interval, Event::StartHeight { node: i, life, height: block.height + 1 });
            }
            Out::Signed(typ, height, round, value) => self.observe_vote(i, typ, height, round, value),
            Out::Evidence => self.stats.evidence += 1,
        }
    }

    fn observe_commit(&mut self, i: usize, block: &Block, cert: &Cert) {
        let id = block.id(&self.crypto.chain_id);
        if !self.is_honest(i) {
            return;
        }
        if block_on(self.crypto.verify_commit_certificate(&OrvexaCtx, cert, &self.vs, ThresholdParams::default())).is_err() {
            self.violations.push(format!("validator {i} committed {} without a valid certificate", block.height));
        }
        match self.decided.get(&block.height) {
            Some(other) if *other != id => self.violations.push(format!(
                "conflicting decisions at height {}: {other} and {id} (validator {i})",
                block.height
            )),
            Some(_) => {}
            None => {
                self.decided.insert(block.height, id);
            }
        }
    }

    fn observe_vote(&mut self, i: usize, typ: VoteType, height: u64, round: Round, value: NilOrVal<BlockId>) {
        if !self.is_honest(i) {
            return;
        }
        let key = (i, typ as u8, height, round.as_i64());
        match self.signed.get(&key) {
            Some(previous) if *previous != value => self.violations.push(format!(
                "honest validator {i} signed two {typ:?} votes at height {height} round {round}: {previous:?} and {value:?}"
            )),
            Some(_) => {}
            None => {
                self.signed.insert(key, value);
            }
        }
    }

    fn send(&mut self, from: usize, to: usize, msg: Msg) {
        if self.groups[from] != self.groups[to] || self.rng.range(0, 999) < self.drop_permille as u64 {
            self.stats.dropped += 1;
            return;
        }
        let delay = self.rng.range(self.latency.0, self.latency.1);
        self.schedule(delay, Event::Deliver { from, to, msg: Box::new(msg) });
    }

    fn dispatch(&mut self, event: Event) {
        match event {
            Event::Deliver { from, to, msg } => {
                if !self.nodes[to].alive || self.groups[from] != self.groups[to] {
                    self.stats.dropped += 1;
                    return;
                }
                self.stats.delivered += 1;
                self.receive(to, from, *msg);
            }
            Event::Timer { node, life, token, timeout } => {
                let n = &mut self.nodes[node];
                if n.life == life && n.alive && n.io.timers.get(&timeout) == Some(&token) {
                    n.io.timers.remove(&timeout);
                    self.feed(node, Input::TimeoutElapsed(timeout));
                }
            }
            Event::Propose { node, life, height, round } => {
                if self.nodes[node].life == life && self.nodes[node].alive {
                    self.propose(node, height, round);
                }
            }
            Event::StartHeight { node, life, height } => {
                let n = &self.nodes[node];
                if n.life == life && n.alive && n.disk.tip() + 1 == height && n.cons.as_ref().is_some_and(|s| s.height().0 < height) {
                    let vs = self.vs.clone();
                    self.feed(node, Input::StartHeight(Height(height), vs, false, None, VoteExtensionPolicy::Disabled));
                    self.offer_stored(node, height);
                }
            }
            Event::StatusTick { node, life } => {
                if self.nodes[node].life == life && self.nodes[node].alive {
                    let tip = self.height(node);
                    for to in 0..self.nodes.len() {
                        if to != node {
                            self.send(node, to, Msg::Status(tip));
                        }
                    }
                    self.schedule(STATUS_EVERY_MS, Event::StatusTick { node, life });
                }
            }
        }
    }

    fn propose(&mut self, i: usize, height: u64, round: Round) {
        let chain_id = self.crypto.chain_id;
        let node = &mut self.nodes[i];
        if node.disk.tip() + 1 != height || node.cons.as_ref().map(|s| (s.height().0, s.round())) != Some((height, round)) {
            return;
        }
        let prev = node.disk.tip_id(&self.crypto);
        let key = (height, round.as_i64());
        // After a restart the same (height, round) must get the same block,
        // otherwise the replayed proposal and the new one would conflict.
        let id = match node.disk.proposed.get(&key) {
            Some(id) => *id,
            None => {
                node.produced += 1;
                let tag = if node.behaviour == Behaviour::InvalidBlocks { "invalid" } else { "txs" };
                let block = Block { height, prev, proposer: node.address, payload: format!("{tag} h{height} r{round} v{i} #{}", node.produced).into_bytes() };
                let id = block.id(&chain_id);
                node.disk.bodies.insert(id, block);
                node.disk.proposed.insert(key, id);
                id
            }
        };
        let block = node.disk.bodies[&id].clone();
        let body = BodyMsg { height, round, pol_round: Round::Nil, proposer: node.address, block };
        if node.behaviour == Behaviour::Equivocate {
            let twin = Block { payload: [body.block.payload.as_slice(), b" twin"].concat(), ..body.block.clone() };
            let twin_id = twin.id(&chain_id);
            node.disk.bodies.insert(twin_id, twin.clone());
            node.io.alt.insert(key, (id, twin_id));
            let (a, b) = node.io.halves();
            let twin_body = BodyMsg { block: twin, ..body.clone() };
            self.apply(i, Out::SendTo(a, Msg::Body(body)));
            self.apply(i, Out::SendTo(b, Msg::Body(twin_body)));
        } else {
            self.apply(i, Out::Broadcast(Msg::Body(body)));
        }
        self.feed(i, Input::Propose(LocallyProposedValue::new(Height(height), round, id)));
    }

    /// Hands block bodies received earlier for `height` to consensus.
    fn offer_stored(&mut self, i: usize, height: u64) {
        let headers: Vec<Header> = self.nodes[i].disk.headers.iter().filter(|h| h.height == height).copied().collect();
        for header in headers {
            self.offer(i, header);
        }
    }

    /// Checks a block body against the application rules and reports the
    /// verdict to consensus. Bodies from a sender that is not the expected
    /// proposer of that round are ignored.
    fn offer(&mut self, i: usize, header: Header) {
        let node = &self.nodes[i];
        if node.cons.as_ref().map(|s| s.height().0) != Some(header.height) || node.disk.tip() + 1 != header.height {
            return;
        }
        if OrvexaCtx::proposer(&self.vs, Height(header.height), header.round).address != header.proposer {
            return;
        }
        let Some(block) = node.disk.bodies.get(&header.id) else { return };
        let valid = block.height == header.height && block.prev == node.disk.tip_id(&self.crypto) && self.valid_payload(block);
        let value = ProposedValue {
            height: Height(header.height),
            round: header.round,
            valid_round: header.pol_round,
            proposer: header.proposer,
            value: header.id,
            validity: if valid { Validity::Valid } else { Validity::Invalid },
        };
        self.feed(i, Input::ProposedValue(value, ValueOrigin::Consensus));
    }

    fn receive(&mut self, i: usize, from: usize, msg: Msg) {
        match msg {
            Msg::Consensus(SignedConsensusMsg::Vote(v)) => self.feed(i, Input::Vote(v)),
            Msg::Consensus(SignedConsensusMsg::Proposal(p)) => self.feed(i, Input::Proposal(p)),
            Msg::Liveness(LivenessMsg::Vote(v)) => self.feed(i, Input::Vote(v)),
            Msg::Liveness(LivenessMsg::PolkaCertificate(c)) => self.feed(i, Input::PolkaCertificate(c)),
            Msg::Liveness(LivenessMsg::SkipRoundCertificate(c)) => self.feed(i, Input::RoundCertificate(c)),
            Msg::Body(body) => {
                let id = body.block.id(&self.crypto.chain_id);
                let header = Header { height: body.height, round: body.round, pol_round: body.pol_round, proposer: body.proposer, id };
                let disk = &mut self.nodes[i].disk;
                if body.height <= disk.tip() {
                    return;
                }
                disk.bodies.entry(id).or_insert(body.block);
                if !disk.headers.contains(&header) {
                    disk.headers.push(header);
                }
                self.offer(i, header);
            }
            Msg::Status(tip) => {
                self.nodes[i].peer_tips.insert(from, tip);
                self.maybe_sync(i);
            }
            Msg::SyncRequest(height) => {
                let node = &self.nodes[i];
                let Some((block, cert)) = node.disk.chain.get(height.wrapping_sub(1) as usize).cloned() else { return };
                let block = if node.behaviour == Behaviour::ForgeSync {
                    Block { payload: b"forged: pays the forger".to_vec(), ..block }
                } else {
                    block
                };
                self.send(i, from, Msg::SyncResponse(block, cert));
            }
            Msg::SyncResponse(block, cert) => self.accept_synced(i, from, block, cert),
        }
    }

    fn maybe_sync(&mut self, i: usize) {
        let now = self.now;
        let node = &mut self.nodes[i];
        let tip = node.disk.tip();
        if let Some((height, at)) = node.sync_asked {
            if height > tip && now < at + SYNC_RETRY_MS {
                return;
            }
        }
        // Any peer that is ahead and not caught lying; picked at random so one
        // bad peer cannot keep a validator to itself.
        let peers: Vec<usize> = node.peer_tips.iter().filter(|(p, t)| **t > tip && !node.banned.contains(p)).map(|(p, _)| *p).collect();
        if peers.is_empty() {
            return;
        }
        let peer = peers[self.rng.range(0, peers.len() as u64 - 1) as usize];
        self.nodes[i].sync_asked = Some((tip + 1, now));
        self.send(i, peer, Msg::SyncRequest(tip + 1));
    }

    /// Applies a block learned from a peer only if its commit certificate
    /// verifies against the validator set and the block extends our chain.
    fn accept_synced(&mut self, i: usize, from: usize, block: Block, cert: Cert) {
        let node = &self.nodes[i];
        let tip = node.disk.tip();
        if block.height != tip + 1 {
            return;
        }
        let id = block.id(&self.crypto.chain_id);
        let checks = cert.height == Height(block.height)
            && cert.value_id == id
            && block.prev == node.disk.tip_id(&self.crypto)
            && self.valid_payload(&block)
            && block_on(self.crypto.verify_commit_certificate(&OrvexaCtx, &cert, &self.vs, self.thresholds)).is_ok();
        if !checks {
            self.stats.rejected_sync += 1;
            self.nodes[i].banned.insert(from);
            self.nodes[i].sync_asked = None;
            return;
        }
        self.stats.synced_blocks += 1;
        self.nodes[i].disk.commit(block.clone(), cert.clone());
        self.nodes[i].sync_asked = None;
        self.observe_commit(i, &block, &cert);
        let next = block.height + 1;
        let vs = self.vs.clone();
        self.feed(i, Input::StartHeight(Height(next), vs, false, None, VoteExtensionPolicy::Disabled));
        self.offer_stored(i, next);
        self.maybe_sync(i);
    }
}

fn describe(input: &Input<OrvexaCtx>) -> String {
    match input {
        Input::Vote(v) => format!("vote {:?} h{} r{} {:?} by {}", v.typ, v.height, v.round, v.value, v.validator),
        Input::Proposal(p) => format!("proposal h{} r{} {} pol{} by {}", p.height, p.round, p.value, p.pol_round, p.proposer),
        Input::ProposedValue(v, o) => format!("value h{} r{} {} vr{} {:?} {:?}", v.height, v.round, v.value, v.valid_round, v.validity, o),
        Input::TimeoutElapsed(t) => format!("timeout {:?} r{}", t.kind, t.round),
        Input::PolkaCertificate(c) => format!("polka h{} r{}", c.height, c.round),
        Input::RoundCertificate(c) => format!("roundcert h{} r{}", c.height, c.round),
        other => format!("{other:?}").chars().take(80).collect(),
    }
}
