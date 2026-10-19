//! Validator engine: runs the Malachite state machine on its own thread and
//! plays the host around it — signing, verification, timers, the
//! write-ahead log, gossip of votes and block bodies, and committing decided
//! blocks to the ledger through [`Host`].
//!
//! Rules the fault simulator (`research/bft-eval`) showed are required:
//! * a WAL entry is on disk before the vote it caused leaves this machine,
//!   and entries are kept per height (a late input of a committed height is
//!   never replayed as the next height's);
//! * after a restart the same (height, round) gets the same proposed block,
//!   so proposals and bodies of the current height are on disk too.
//!
//! The engine idles while there is nothing to do: a height starts when the
//! mempool has a transaction, when the WAL shows the height was in progress
//! before a restart, or when a peer is already voting on it.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use futures::executor::block_on;
use malachitebft_core_consensus::{
    process, ConsensusMsg, Effect, Error as ConsensusError, Input, LivenessMsg,
    LocallyProposedValue, Params as ConsensusParams, ProposedValue, Resumable, Resume,
    SignedConsensusMsg, State, ValuePayload, VoteExtensionError,
};
use malachitebft_core_types::{
    CommitCertificate, LinearTimeouts, Round, ThresholdParams, Timeout, TimeoutKind, Validity,
    ValueOrigin, VoteExtensionPolicy,
};
use malachitebft_signing::VerifierExt;
use rusqlite::{params, Connection, OptionalExtension};

use super::codec::{self, Body, NetMsg};
use super::ctx::{Address, BlockId, Crypto, Height, OrvexaCtx, PrivateKey, ValidatorSet};
use crate::ledger::{block_id, Block, Commit, Params};

/// What the engine needs from the node.
pub trait Host: Send + 'static {
    /// Height and ID of the last committed block.
    fn tip(&self) -> (u64, [u8; 32]);
    /// Whether transactions are waiting for a block.
    fn has_work(&self) -> bool;
    /// Mempool transactions valid on top of the tip, for a new block.
    fn candidates(&self) -> Vec<Vec<u8>>;
    /// Full ledger rules for a proposed block on top of the tip (proofs included).
    fn check_body(&self, block: &Block) -> bool;
    /// Stores a decided block; it carries its commit certificate.
    fn commit(&self, block: Block) -> Result<(), String>;
    /// Sends encoded consensus bytes to a peer validator (fire and forget).
    fn send(&self, peer: &str, bytes: Vec<u8>);
}

#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Base URLs of the other validators' nodes.
    pub peers: Vec<String>,
    /// Pause between a committed block and the next height.
    pub block_every: Duration,
    pub timeouts: LinearTimeouts,
}

pub fn default_timeouts() -> LinearTimeouts {
    LinearTimeouts {
        propose: Duration::from_millis(1500),
        propose_delta: Duration::from_millis(500),
        prevote: Duration::from_millis(500),
        prevote_delta: Duration::from_millis(250),
        precommit: Duration::from_millis(500),
        precommit_delta: Duration::from_millis(250),
        rebroadcast: Duration::from_secs(3),
        max_timeout: Duration::from_secs(30),
    }
}

pub enum Event {
    Net(Box<NetMsg>),
    /// A transaction entered the mempool.
    Work,
    /// The ledger moved on without this engine (catch-up from a peer).
    LedgerAdvanced,
    Stop,
}

/// Rows kept per height before more bodies from peers are ignored.
const MAX_BODIES_PER_HEIGHT: i64 = 256;
/// Messages kept while no height is running.
const MAX_PENDING: usize = 4096;

pub fn spawn<H: Host>(
    host: H,
    params: &Params,
    chain_id: [u8; 32],
    key: [u8; 32],
    config: EngineConfig,
    db_path: &Path,
    events: Receiver<Event>,
) -> Result<std::thread::JoinHandle<()>, String> {
    let key = PrivateKey::from(key);
    let address = Address(*key.public_key().as_bytes());
    if !params.validators.contains(&address.0) {
        return Err("this validator key is not in the network's validator set".into());
    }
    let db = Connection::open(db_path).map_err(|e| format!("open consensus log: {e}"))?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;
         CREATE TABLE IF NOT EXISTS wal (seq INTEGER PRIMARY KEY AUTOINCREMENT, height INTEGER NOT NULL, entry BLOB NOT NULL);
         CREATE TABLE IF NOT EXISTS bodies (height INTEGER NOT NULL, round INTEGER NOT NULL, pol_round INTEGER NOT NULL,
             proposer BLOB NOT NULL, id BLOB NOT NULL, block BLOB NOT NULL, PRIMARY KEY (height, round, proposer, id));
         CREATE TABLE IF NOT EXISTS proposed (height INTEGER NOT NULL, round INTEGER NOT NULL, id BLOB NOT NULL, PRIMARY KEY (height, round));",
    )
    .map_err(|e| format!("consensus log schema: {e}"))?;
    let (tip, tip_id) = host.tip();
    let io = Io {
        host,
        crypto: Crypto { chain_id },
        key,
        peers: config.peers.clone(),
        timeouts: config.timeouts,
        db,
        timers: HashMap::new(),
        deadlines: BTreeMap::new(),
        token: 0,
        replaying: false,
        tip,
        tip_id,
        deferred: Vec::new(),
    };
    let mut engine = Engine {
        state: None,
        io,
        vs: ValidatorSet::from_params(params),
        address,
        chain_id,
        block_every: config.block_every,
        height: tip + 1,
        active: false,
        wanted: false,
        next_start: Instant::now(),
        pending: Vec::new(),
        validity: HashMap::new(),
    };
    std::thread::Builder::new()
        .name("orvexa-v2-consensus".into())
        .spawn(move || engine.run(events))
        .map_err(|e| e.to_string())
}

pub type Handle = Sender<Event>;

/// A `bodies` row: round, pol_round, proposer, id, block.
type StoredBody = (i64, i64, Vec<u8>, Vec<u8>, Vec<u8>);

enum Deferred {
    Propose(u64, Round),
    Finalized(CommitCertificate<OrvexaCtx>),
}

struct Io<H: Host> {
    host: H,
    crypto: Crypto,
    key: PrivateKey,
    peers: Vec<String>,
    timeouts: LinearTimeouts,
    db: Connection,
    timers: HashMap<Timeout, u64>,
    deadlines: BTreeMap<(Instant, u64), Timeout>,
    token: u64,
    replaying: bool,
    tip: u64,
    tip_id: [u8; 32],
    deferred: Vec<Deferred>,
}

impl<H: Host> Io<H> {
    fn broadcast(&self, msg: &NetMsg) {
        let bytes = msg.encode();
        for peer in &self.peers {
            self.host.send(peer, bytes.clone());
        }
    }

    fn duration(&self, t: Timeout) -> Duration {
        match t.kind {
            TimeoutKind::FinalizeHeight(d) => d,
            _ => self.timeouts.duration_for(t),
        }
    }

    fn body(&self, id: &BlockId) -> Option<Block> {
        self.db
            .query_row(
                "SELECT block FROM bodies WHERE id = ?1 LIMIT 1",
                [&id.0[..]],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()
            .ok()
            .flatten()
            .and_then(|b| Block::decode(&b))
    }

    fn handle(&mut self, effect: Effect<OrvexaCtx>) -> Result<Resume<OrvexaCtx>, ()> {
        let ctx = OrvexaCtx;
        Ok(match effect {
            Effect::CancelAllTimeouts(r) => {
                self.timers.clear();
                self.deadlines.clear();
                r.resume_with(())
            }
            Effect::CancelTimeout(t, r) => {
                self.timers.remove(&t);
                r.resume_with(())
            }
            Effect::ScheduleTimeout(t, r) => {
                self.token += 1;
                self.timers.insert(t, self.token);
                self.deadlines
                    .insert((Instant::now() + self.duration(t), self.token), t);
                r.resume_with(())
            }
            Effect::StartRound(_, _, _, _, r) => r.resume_with(()),
            Effect::PublishConsensusMsg(msg, r) => {
                self.broadcast(&match msg {
                    SignedConsensusMsg::Vote(v) => NetMsg::Vote(v),
                    SignedConsensusMsg::Proposal(p) => NetMsg::Proposal(p),
                });
                r.resume_with(())
            }
            Effect::PublishLivenessMsg(msg, r) => {
                self.broadcast(&match msg {
                    LivenessMsg::Vote(v) => NetMsg::Vote(v),
                    LivenessMsg::PolkaCertificate(c) => NetMsg::Polka(c),
                    LivenessMsg::SkipRoundCertificate(c) => NetMsg::RoundCert(c),
                });
                r.resume_with(())
            }
            Effect::RepublishVote(v, r) => {
                self.broadcast(&NetMsg::Vote(v));
                r.resume_with(())
            }
            Effect::RepublishRoundCertificate(c, r) => {
                self.broadcast(&NetMsg::RoundCert(c));
                r.resume_with(())
            }
            Effect::GetValue(height, round, _, r) => {
                self.deferred.push(Deferred::Propose(height.0, round));
                r.resume_with(())
            }
            Effect::RestreamProposal(height, round, pol_round, proposer, id, r) => {
                if let Some(block) = self.body(&id) {
                    self.broadcast(&NetMsg::Body(Body {
                        height: height.0,
                        round,
                        pol_round,
                        proposer,
                        block,
                    }));
                }
                r.resume_with(())
            }
            Effect::CertVerifiedSyncValue(_, _, r) => r.resume_with(()),
            Effect::CertRejectedSyncValue(_, _, _, r) => r.resume_with(()),
            Effect::Decide(_, _, r) => r.resume_with(()),
            Effect::Finalize(cert, _, evidence, r) => {
                if !evidence.is_empty() {
                    eprintln!(
                        "ORVEXA v2 consensus: misbehaviour evidence at height {}",
                        cert.height
                    );
                }
                self.deferred.push(Deferred::Finalized(cert));
                r.resume_with(())
            }
            Effect::SignVote(vote, r) => r.resume_with(self.crypto.sign_vote(&self.key, vote)),
            Effect::SignProposal(p, r) => r.resume_with(self.crypto.sign_proposal(&self.key, p)),
            Effect::VerifySignature(msg, key, r) => {
                let ok = match &msg.message {
                    ConsensusMsg::Vote(v) => self.crypto.vote_ok(v, &msg.signature, &key),
                    ConsensusMsg::Proposal(p) => self.crypto.proposal_ok(p, &msg.signature, &key),
                };
                r.resume_with(ok)
            }
            Effect::VerifyCommitCertificate(c, vs, th, r) => r.resume_with(block_on(
                self.crypto.verify_commit_certificate(&ctx, &c, &vs, th),
            )),
            Effect::VerifyExtendedCommitCertificate(c, vs, th, policy, r) => {
                r.resume_with(block_on(
                    self.crypto
                        .verify_extended_commit_certificate(&ctx, &c, &vs, th, policy),
                ))
            }
            Effect::VerifyPolkaCertificate(c, vs, th, r) => r.resume_with(block_on(
                self.crypto.verify_polka_certificate(&ctx, &c, &vs, th),
            )),
            Effect::VerifyRoundCertificate(c, vs, th, r) => r.resume_with(block_on(
                self.crypto.verify_round_certificate(&ctx, &c, &vs, th),
            )),
            Effect::WalAppend(height, input, r) => {
                if !self.replaying && height.0 > self.tip {
                    if let Some(entry) = codec::encode_input(&input) {
                        // Durable (synchronous=FULL) before consensus goes on to publish.
                        if let Err(e) = self.db.execute(
                            "INSERT INTO wal (height, entry) VALUES (?1, ?2)",
                            params![height.0 as i64, entry],
                        ) {
                            eprintln!("ORVEXA v2 consensus: WAL write failed: {e}");
                            return Err(());
                        }
                    }
                }
                r.resume_with(())
            }
            Effect::ExtendVote(_, _, _, _, r) => r.resume_with(None),
            Effect::VerifyVoteExtension(_, _, _, _, _, _, r) => {
                r.resume_with(Err(VoteExtensionError::InvalidVoteExtension))
            }
        })
    }
}

struct Engine<H: Host> {
    state: Option<State<OrvexaCtx>>,
    io: Io<H>,
    vs: ValidatorSet,
    address: Address,
    chain_id: [u8; 32],
    block_every: Duration,
    /// Height being decided: tip + 1.
    height: u64,
    /// `StartHeight(height)` was fed.
    active: bool,
    /// A peer is already working on `height`.
    wanted: bool,
    next_start: Instant,
    /// Messages received before the state machine exists (fresh start, idle).
    pending: Vec<NetMsg>,
    validity: HashMap<BlockId, bool>,
}

impl<H: Host> Engine<H> {
    fn run(&mut self, events: Receiver<Event>) {
        eprintln!(
            "ORVEXA v2 validator {} at height {}",
            self.address, self.height
        );
        loop {
            let now = Instant::now();
            let mut wait = Duration::from_millis(500);
            if let Some(((at, _), _)) = self.io.deadlines.first_key_value() {
                wait = wait.min(at.saturating_duration_since(now));
            }
            if !self.active {
                wait = wait.min(
                    self.next_start
                        .saturating_duration_since(now)
                        .max(Duration::from_millis(5)),
                );
            }
            match events.recv_timeout(wait) {
                Ok(Event::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Event::Net(msg)) => self.receive(*msg),
                Ok(Event::Work) => {}
                Ok(Event::LedgerAdvanced) => self.follow_ledger(),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.fire_timers();
            self.maybe_start();
        }
        eprintln!("ORVEXA v2 validator stopped");
    }

    fn feed(&mut self, input: Input<OrvexaCtx>) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let io = &mut self.io;
        let result: Result<(), ConsensusError<OrvexaCtx>> = process!(
            input: input,
            state: state,
            metrics: &(),
            with: effect => io.handle(effect)
        );
        if let Err(e) = result {
            eprintln!("ORVEXA v2 consensus error at height {}: {e:?}", self.height);
        }
        for action in std::mem::take(&mut self.io.deferred) {
            match action {
                Deferred::Propose(height, round) => self.propose(height, round),
                Deferred::Finalized(cert) => self.finalize(cert),
            }
        }
    }

    fn fire_timers(&mut self) {
        let now = Instant::now();
        while let Some(((at, token), timeout)) =
            self.io.deadlines.first_key_value().map(|(k, v)| (*k, *v))
        {
            if at > now {
                break;
            }
            self.io.deadlines.remove(&(at, token));
            if self.io.timers.get(&timeout) == Some(&token) {
                self.io.timers.remove(&timeout);
                self.feed(Input::TimeoutElapsed(timeout));
            }
        }
    }

    fn wal(&self, height: u64) -> Vec<Input<OrvexaCtx>> {
        let mut stmt = match self
            .io
            .db
            .prepare("SELECT entry FROM wal WHERE height = ?1 ORDER BY seq")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([height as i64], |r| r.get::<_, Vec<u8>>(0));
        rows.map(|rows| {
            rows.filter_map(Result::ok)
                .filter_map(|b| codec::decode_input(&b))
                .collect()
        })
        .unwrap_or_default()
    }

    fn maybe_start(&mut self) {
        if self.active || Instant::now() < self.next_start {
            return;
        }
        let wal = self.wal(self.height);
        if !(self.wanted || !wal.is_empty() || self.io.host.has_work()) {
            return;
        }
        let height = Height(self.height);
        let restart = !wal.is_empty();
        if self.state.is_none() {
            let params = ConsensusParams {
                address: self.address,
                threshold_params: ThresholdParams::default(),
                value_payload: ValuePayload::ProposalAndParts,
                enabled: true,
            };
            self.state = Some(State::new(
                OrvexaCtx,
                height,
                self.vs.clone(),
                params,
                8192,
                2048,
            ));
        }
        self.active = true;
        self.wanted = false;
        self.feed(Input::StartHeight(
            height,
            self.vs.clone(),
            restart,
            None,
            VoteExtensionPolicy::Disabled,
        ));
        if restart {
            eprintln!(
                "ORVEXA v2 consensus: resuming height {} from {} WAL entries",
                self.height,
                wal.len()
            );
            self.io.replaying = true;
            if let Some(state) = self.state.as_mut() {
                state.index_wal_entries(wal.iter());
            }
            for input in wal {
                self.feed(input);
            }
            if let Some(state) = self.state.as_mut() {
                state.reset_entries_index();
            }
            self.io.replaying = false;
        }
        for msg in std::mem::take(&mut self.pending) {
            self.receive(msg);
        }
        self.offer_stored();
    }

    fn receive(&mut self, msg: NetMsg) {
        let height = msg.height();
        if height <= self.io.tip {
            return;
        }
        if height == self.height && !self.active {
            self.wanted = true;
        }
        match msg {
            NetMsg::Body(body) => self.store_body(body),
            other if self.state.is_none() => {
                if self.pending.len() < MAX_PENDING {
                    self.pending.push(other);
                }
            }
            NetMsg::Vote(v) => self.feed(Input::Vote(v)),
            NetMsg::Proposal(p) => self.feed(Input::Proposal(p)),
            NetMsg::Polka(c) => self.feed(Input::PolkaCertificate(c)),
            NetMsg::RoundCert(c) => self.feed(Input::RoundCertificate(c)),
        }
    }

    fn store_body(&mut self, body: Body) {
        if body.height > self.height + 1 || body.block.height != body.height {
            return;
        }
        let id = BlockId(block_id(&self.chain_id, &body.block));
        let count: i64 = self
            .io
            .db
            .query_row(
                "SELECT COUNT(*) FROM bodies WHERE height = ?1",
                [body.height as i64],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if count >= MAX_BODIES_PER_HEIGHT {
            return;
        }
        let _ = self.io.db.execute(
            "INSERT OR IGNORE INTO bodies VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                body.height as i64,
                body.round.as_i64(),
                body.pol_round.as_i64(),
                &body.proposer.0[..],
                &id.0[..],
                body.block.encode()
            ],
        );
        if self.active && body.height == self.height {
            self.offer(body.round, body.pol_round, body.proposer, id, &body.block);
        }
    }

    /// Hands bodies stored for the current height to consensus (after a start or restart).
    fn offer_stored(&mut self) {
        let rows: Vec<StoredBody> = {
            let Ok(mut stmt) = self.io.db.prepare(
                "SELECT round, pol_round, proposer, id, block FROM bodies WHERE height = ?1",
            ) else {
                return;
            };
            let rows = stmt.query_map([self.height as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            });
            match rows {
                Ok(rows) => rows.filter_map(Result::ok).collect(),
                Err(_) => return,
            }
        };
        for (round, pol_round, proposer, id, block) in rows {
            let (Some(round), Some(pol_round), Ok(proposer), Ok(id), Some(block)) = (
                to_round(round),
                to_round(pol_round),
                proposer.try_into(),
                id.try_into(),
                Block::decode(&block),
            ) else {
                continue;
            };
            self.offer(round, pol_round, Address(proposer), BlockId(id), &block);
        }
    }

    /// Runs the ledger rules on a proposed body and reports the verdict.
    /// Bodies attributed to anyone but the proposer of that round are ignored.
    fn offer(
        &mut self,
        round: Round,
        pol_round: Round,
        proposer: Address,
        id: BlockId,
        block: &Block,
    ) {
        if !round.is_defined()
            || OrvexaCtx::proposer(&self.vs, Height(self.height), round).address != proposer
        {
            return;
        }
        let valid = match self.validity.get(&id) {
            Some(v) => *v,
            None => {
                let v = block.prev == self.io.tip_id
                    && block.commit.signatures.is_empty()
                    && self.io.host.check_body(block);
                self.validity.insert(id, v);
                v
            }
        };
        let value = ProposedValue {
            height: Height(self.height),
            round,
            valid_round: pol_round,
            proposer,
            value: id,
            validity: if valid {
                Validity::Valid
            } else {
                Validity::Invalid
            },
        };
        self.feed(Input::ProposedValue(value, ValueOrigin::Consensus));
    }

    fn propose(&mut self, height: u64, round: Round) {
        if height != self.height || !self.active {
            return;
        }
        let stored: Option<Vec<u8>> = self
            .io
            .db
            .query_row(
                "SELECT id FROM proposed WHERE height = ?1 AND round = ?2",
                params![height as i64, round.as_i64()],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten();
        // After a restart the same (height, round) must get the same block.
        let block = match stored
            .and_then(|id| <[u8; 32]>::try_from(id).ok())
            .and_then(|id| self.io.body(&BlockId(id)))
        {
            Some(block) => block,
            None => {
                let block = Block {
                    height,
                    prev: self.io.tip_id,
                    commit: Commit::default(),
                    txs: self.io.host.candidates(),
                };
                let id = block_id(&self.chain_id, &block);
                let saved = self.io.db.execute(
                    "INSERT OR IGNORE INTO bodies VALUES (?1, ?2, -1, ?3, ?4, ?5)",
                    params![
                        height as i64,
                        round.as_i64(),
                        &self.address.0[..],
                        &id[..],
                        block.encode()
                    ],
                );
                let recorded = self.io.db.execute(
                    "INSERT OR REPLACE INTO proposed VALUES (?1, ?2, ?3)",
                    params![height as i64, round.as_i64(), &id[..]],
                );
                if saved.is_err() || recorded.is_err() {
                    eprintln!("ORVEXA v2 consensus: cannot record own proposal; not proposing");
                    return;
                }
                block
            }
        };
        let id = BlockId(block_id(&self.chain_id, &block));
        self.validity.insert(id, true);
        self.io.broadcast(&NetMsg::Body(Body {
            height,
            round,
            pol_round: Round::Nil,
            proposer: self.address,
            block,
        }));
        self.feed(Input::Propose(LocallyProposedValue::new(
            Height(height),
            round,
            id,
        )));
    }

    fn finalize(&mut self, cert: CommitCertificate<OrvexaCtx>) {
        let height = cert.height.0;
        let Some(mut block) = self.io.body(&cert.value_id) else {
            eprintln!(
                "ORVEXA v2 consensus: decided block {} at {height} has no stored body",
                cert.value_id
            );
            return;
        };
        let mut signatures: Vec<_> = cert
            .commit_signatures
            .iter()
            .map(|s| (s.address.0, s.signature.to_bytes()))
            .collect();
        signatures.sort_by_key(|(k, _)| *k);
        block.commit = Commit {
            round: cert.round.as_u32().unwrap_or(0),
            signatures,
        };
        let txs = block.txs.len();
        match self.io.host.commit(block) {
            Ok(()) => eprintln!(
                "ORVEXA v2 committed block {height} {} with {txs} tx (round {})",
                cert.value_id, cert.round
            ),
            Err(e) => {
                if self.io.host.tip().0 < height {
                    eprintln!("ORVEXA v2 decided block {height} was refused by the ledger: {e}");
                    return;
                }
            }
        }
        self.advance();
    }

    /// Moves to the height after the ledger's tip and drops logs below it.
    fn advance(&mut self) {
        let (tip, tip_id) = self.io.host.tip();
        self.io.tip = tip;
        self.io.tip_id = tip_id;
        self.height = tip + 1;
        self.active = false;
        self.validity.clear();
        self.next_start = Instant::now() + self.block_every;
        for table in ["wal", "bodies", "proposed"] {
            let _ = self.io.db.execute(
                &format!("DELETE FROM {table} WHERE height <= ?1"),
                [tip as i64],
            );
        }
    }

    /// The node applied blocks from a peer: abandon the height in progress.
    fn follow_ledger(&mut self) {
        let (tip, _) = self.io.host.tip();
        if tip < self.height {
            return;
        }
        // A fresh state machine: the old one may hold inputs of decided heights.
        self.state = None;
        self.io.timers.clear();
        self.io.deadlines.clear();
        self.pending.clear();
        self.advance();
        self.next_start = Instant::now();
    }
}

fn to_round(r: i64) -> Option<Round> {
    match r {
        -1 => Some(Round::Nil),
        r if (0..=u32::MAX as i64).contains(&r) => Some(Round::new(r as u32)),
        _ => None,
    }
}
