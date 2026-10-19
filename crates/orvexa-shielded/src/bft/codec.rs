//! Byte encoding of consensus messages, for the network between validators
//! and for the write-ahead log. Fixed layouts, integers big-endian, rounds as
//! i64 (−1 = nil). Decoding is strict: wrong lengths, unknown tags and
//! trailing bytes are refused.

use malachitebft_core_consensus::{Input, ProposedValue};
use malachitebft_core_types::{
    NilOrVal, PolkaCertificate, PolkaSignature, Round, RoundCertificate, RoundCertificateType,
    RoundSignature, SignedMessage, Timeout, TimeoutKind, Validity, ValueOrigin, VoteType,
};
use std::time::Duration;

use super::ctx::{Address, BlockId, Height, OrvexaCtx, Proposal, Signature, Vote};
use crate::ledger::{Block, MAX_VALIDATORS};

pub type SignedVote = SignedMessage<OrvexaCtx, Vote>;
pub type SignedProposal = SignedMessage<OrvexaCtx, Proposal>;

const MAGIC: &[u8; 4] = b"OVXC";
const VERSION: u8 = 1;
/// Largest block body accepted from a peer.
pub const MAX_BODY: usize = 8 * 1024 * 1024;

/// A block body as gossiped by the proposer of `round` (the "parts").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Body {
    pub height: u64,
    pub round: Round,
    pub pol_round: Round,
    pub proposer: Address,
    /// The block without a commit certificate.
    pub block: Block,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetMsg {
    Vote(SignedVote),
    Proposal(SignedProposal),
    Polka(PolkaCertificate<OrvexaCtx>),
    RoundCert(RoundCertificate<OrvexaCtx>),
    Body(Body),
}

impl NetMsg {
    pub fn height(&self) -> u64 {
        match self {
            NetMsg::Vote(v) => v.height.0,
            NetMsg::Proposal(p) => p.height.0,
            NetMsg::Polka(c) => c.height.0,
            NetMsg::RoundCert(c) => c.height.0,
            NetMsg::Body(b) => b.height,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(256);
        w.extend_from_slice(MAGIC);
        w.push(VERSION);
        match self {
            NetMsg::Vote(v) => {
                w.push(1);
                put_signed_vote(&mut w, v);
            }
            NetMsg::Proposal(p) => {
                w.push(2);
                put_signed_proposal(&mut w, p);
            }
            NetMsg::Polka(c) => {
                w.push(3);
                put_polka(&mut w, c);
            }
            NetMsg::RoundCert(c) => {
                w.push(4);
                put_round_cert(&mut w, c);
            }
            NetMsg::Body(b) => {
                w.push(5);
                w.extend_from_slice(&b.height.to_be_bytes());
                put_round(&mut w, b.round);
                put_round(&mut w, b.pol_round);
                w.extend_from_slice(&b.proposer.0);
                w.extend_from_slice(&b.block.encode());
            }
        }
        w
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        if r.take(4)? != MAGIC || r.u8()? != VERSION {
            return None;
        }
        let msg = match r.u8()? {
            1 => NetMsg::Vote(r.signed_vote()?),
            2 => NetMsg::Proposal(r.signed_proposal()?),
            3 => NetMsg::Polka(r.polka()?),
            4 => NetMsg::RoundCert(r.round_cert()?),
            5 => {
                let height = r.u64()?;
                let round = r.round()?;
                let pol_round = r.round()?;
                let proposer = Address(r.arr()?);
                let rest = r.rest();
                if rest.len() > MAX_BODY {
                    return None;
                }
                let block = Block::decode(rest)?;
                if block.height != height || !block.commit.signatures.is_empty() {
                    return None;
                }
                return Some(NetMsg::Body(Body {
                    height,
                    round,
                    pol_round,
                    proposer,
                    block,
                }));
            }
            _ => return None,
        };
        r.0.is_empty().then_some(msg)
    }
}

/// WAL entry. `None` for inputs consensus never logs.
pub fn encode_input(input: &Input<OrvexaCtx>) -> Option<Vec<u8>> {
    let mut w = vec![VERSION];
    match input {
        Input::Vote(v) => {
            w.push(1);
            put_signed_vote(&mut w, v);
        }
        Input::Proposal(p) => {
            w.push(2);
            put_signed_proposal(&mut w, p);
        }
        Input::ProposedValue(v, origin) => {
            w.push(3);
            w.extend_from_slice(&v.height.0.to_be_bytes());
            put_round(&mut w, v.round);
            put_round(&mut w, v.valid_round);
            w.extend_from_slice(&v.proposer.0);
            w.extend_from_slice(&v.value.0);
            w.push(matches!(v.validity, Validity::Valid) as u8);
            w.push(matches!(origin, ValueOrigin::Sync) as u8);
        }
        Input::TimeoutElapsed(t) => {
            w.push(4);
            put_timeout(&mut w, t);
        }
        Input::PolkaCertificate(c) => {
            w.push(5);
            put_polka(&mut w, c);
        }
        Input::RoundCertificate(c) => {
            w.push(6);
            put_round_cert(&mut w, c);
        }
        _ => return None,
    }
    Some(w)
}

pub fn decode_input(bytes: &[u8]) -> Option<Input<OrvexaCtx>> {
    let mut r = Reader(bytes);
    if r.u8()? != VERSION {
        return None;
    }
    let input = match r.u8()? {
        1 => Input::Vote(r.signed_vote()?),
        2 => Input::Proposal(r.signed_proposal()?),
        3 => {
            let value = ProposedValue {
                height: Height(r.u64()?),
                round: r.round()?,
                valid_round: r.round()?,
                proposer: Address(r.arr()?),
                value: BlockId(r.arr()?),
                validity: match r.u8()? {
                    1 => Validity::Valid,
                    0 => Validity::Invalid,
                    _ => return None,
                },
            };
            let origin = match r.u8()? {
                0 => ValueOrigin::Consensus,
                1 => ValueOrigin::Sync,
                _ => return None,
            };
            Input::ProposedValue(value, origin)
        }
        4 => Input::TimeoutElapsed(r.timeout()?),
        5 => Input::PolkaCertificate(r.polka()?),
        6 => Input::RoundCertificate(r.round_cert()?),
        _ => return None,
    };
    r.0.is_empty().then_some(input)
}

fn put_round(w: &mut Vec<u8>, round: Round) {
    w.extend_from_slice(&round.as_i64().to_be_bytes());
}

fn put_value(w: &mut Vec<u8>, value: &NilOrVal<BlockId>) {
    match value {
        NilOrVal::Nil => w.push(0),
        NilOrVal::Val(id) => {
            w.push(1);
            w.extend_from_slice(&id.0);
        }
    }
}

fn vote_type(t: VoteType) -> u8 {
    match t {
        VoteType::Prevote => 0,
        VoteType::Precommit => 1,
    }
}

fn put_signed_vote(w: &mut Vec<u8>, v: &SignedVote) {
    w.push(vote_type(v.typ));
    w.extend_from_slice(&v.height.0.to_be_bytes());
    put_round(w, v.round);
    put_value(w, &v.value);
    w.extend_from_slice(&v.validator.0);
    w.extend_from_slice(&v.signature.to_bytes());
}

fn put_signed_proposal(w: &mut Vec<u8>, p: &SignedProposal) {
    w.extend_from_slice(&p.height.0.to_be_bytes());
    put_round(w, p.round);
    w.extend_from_slice(&p.value.0);
    put_round(w, p.pol_round);
    w.extend_from_slice(&p.proposer.0);
    w.extend_from_slice(&p.signature.to_bytes());
}

fn put_timeout(w: &mut Vec<u8>, t: &Timeout) {
    match t.kind {
        TimeoutKind::Propose => w.push(0),
        TimeoutKind::Prevote => w.push(1),
        TimeoutKind::Precommit => w.push(2),
        TimeoutKind::Rebroadcast => w.push(3),
        TimeoutKind::FinalizeHeight(d) => {
            w.push(4);
            w.extend_from_slice(&(d.as_millis() as u64).to_be_bytes());
        }
    }
    put_round(w, t.round);
}

fn put_polka(w: &mut Vec<u8>, c: &PolkaCertificate<OrvexaCtx>) {
    w.extend_from_slice(&c.height.0.to_be_bytes());
    put_round(w, c.round);
    w.extend_from_slice(&c.value_id.0);
    w.extend_from_slice(&(c.polka_signatures.len() as u16).to_be_bytes());
    for s in &c.polka_signatures {
        w.extend_from_slice(&s.address.0);
        w.extend_from_slice(&s.signature.to_bytes());
    }
}

fn put_round_cert(w: &mut Vec<u8>, c: &RoundCertificate<OrvexaCtx>) {
    w.extend_from_slice(&c.height.0.to_be_bytes());
    put_round(w, c.round);
    w.push(match c.cert_type {
        RoundCertificateType::Skip => 0,
        RoundCertificateType::Precommit => 1,
    });
    w.extend_from_slice(&(c.round_signatures.len() as u16).to_be_bytes());
    for s in &c.round_signatures {
        w.push(vote_type(s.vote_type));
        put_value(w, &s.value_id);
        w.extend_from_slice(&s.address.0);
        w.extend_from_slice(&s.signature.to_bytes());
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }
    fn arr<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.arr()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_be_bytes(self.arr()?))
    }
    fn round(&mut self) -> Option<Round> {
        match i64::from_be_bytes(self.arr()?) {
            -1 => Some(Round::Nil),
            r if (0..=u32::MAX as i64).contains(&r) => Some(Round::new(r as u32)),
            _ => None,
        }
    }
    fn defined_round(&mut self) -> Option<Round> {
        self.round().filter(|r| r.is_defined())
    }
    fn signature(&mut self) -> Option<Signature> {
        Some(Signature::from_bytes(self.arr()?))
    }
    fn value(&mut self) -> Option<NilOrVal<BlockId>> {
        match self.u8()? {
            0 => Some(NilOrVal::Nil),
            1 => Some(NilOrVal::Val(BlockId(self.arr()?))),
            _ => None,
        }
    }
    fn vote_type(&mut self) -> Option<VoteType> {
        match self.u8()? {
            0 => Some(VoteType::Prevote),
            1 => Some(VoteType::Precommit),
            _ => None,
        }
    }
    fn count(&mut self) -> Option<usize> {
        let n = self.u16()? as usize;
        (n <= MAX_VALIDATORS).then_some(n)
    }
    fn signed_vote(&mut self) -> Option<SignedVote> {
        let vote = Vote {
            typ: self.vote_type()?,
            height: Height(self.u64()?),
            round: self.defined_round()?,
            value: self.value()?,
            validator: Address(self.arr()?),
            extension: None,
        };
        Some(SignedMessage::new(vote, self.signature()?))
    }
    fn signed_proposal(&mut self) -> Option<SignedProposal> {
        let proposal = Proposal {
            height: Height(self.u64()?),
            round: self.defined_round()?,
            value: BlockId(self.arr()?),
            pol_round: self.round()?,
            proposer: Address(self.arr()?),
        };
        Some(SignedMessage::new(proposal, self.signature()?))
    }
    fn timeout(&mut self) -> Option<Timeout> {
        let kind = match self.u8()? {
            0 => TimeoutKind::Propose,
            1 => TimeoutKind::Prevote,
            2 => TimeoutKind::Precommit,
            3 => TimeoutKind::Rebroadcast,
            4 => TimeoutKind::FinalizeHeight(Duration::from_millis(self.u64()?)),
            _ => return None,
        };
        Some(Timeout::new(self.defined_round()?, kind))
    }
    fn polka(&mut self) -> Option<PolkaCertificate<OrvexaCtx>> {
        let height = Height(self.u64()?);
        let round = self.defined_round()?;
        let value_id = BlockId(self.arr()?);
        let n = self.count()?;
        let mut polka_signatures = Vec::with_capacity(n);
        for _ in 0..n {
            polka_signatures.push(PolkaSignature::new(Address(self.arr()?), self.signature()?));
        }
        Some(PolkaCertificate {
            height,
            round,
            value_id,
            polka_signatures,
        })
    }
    fn round_cert(&mut self) -> Option<RoundCertificate<OrvexaCtx>> {
        let height = Height(self.u64()?);
        let round = self.defined_round()?;
        let cert_type = match self.u8()? {
            0 => RoundCertificateType::Skip,
            1 => RoundCertificateType::Precommit,
            _ => return None,
        };
        let n = self.count()?;
        let mut round_signatures = Vec::with_capacity(n);
        for _ in 0..n {
            round_signatures.push(RoundSignature {
                vote_type: self.vote_type()?,
                value_id: self.value()?,
                address: Address(self.arr()?),
                signature: self.signature()?,
            });
        }
        Some(RoundCertificate {
            height,
            round,
            cert_type,
            round_signatures,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bft::ctx::{Crypto, PrivateKey};
    use crate::ledger::Commit;

    #[test]
    fn messages_and_wal_entries_round_trip() {
        let key = PrivateKey::from([3u8; 32]);
        let me = Address(*key.public_key().as_bytes());
        let crypto = Crypto { chain_id: [8; 32] };
        let id = BlockId([5; 32]);
        let vote = |typ, value| {
            crypto.sign_vote(
                &key,
                Vote {
                    typ,
                    height: Height(7),
                    round: Round::new(2),
                    value,
                    validator: me,
                    extension: None,
                },
            )
        };
        let proposal = crypto.sign_proposal(
            &key,
            Proposal {
                height: Height(7),
                round: Round::new(2),
                value: id,
                pol_round: Round::Nil,
                proposer: me,
            },
        );
        let polka = PolkaCertificate {
            height: Height(7),
            round: Round::new(1),
            value_id: id,
            polka_signatures: vec![PolkaSignature::new(
                me,
                vote(VoteType::Prevote, NilOrVal::Val(id)).signature,
            )],
        };
        let round_cert = RoundCertificate {
            height: Height(7),
            round: Round::new(3),
            cert_type: RoundCertificateType::Skip,
            round_signatures: vec![RoundSignature {
                vote_type: VoteType::Precommit,
                value_id: NilOrVal::Nil,
                address: me,
                signature: vote(VoteType::Precommit, NilOrVal::Nil).signature,
            }],
        };
        let block = Block {
            height: 7,
            prev: [1; 32],
            commit: Commit::default(),
            txs: vec![vec![1, 2, 3]],
        };
        let messages = [
            NetMsg::Vote(vote(VoteType::Prevote, NilOrVal::Val(id))),
            NetMsg::Vote(vote(VoteType::Precommit, NilOrVal::Nil)),
            NetMsg::Proposal(proposal.clone()),
            NetMsg::Polka(polka.clone()),
            NetMsg::RoundCert(round_cert.clone()),
            NetMsg::Body(Body {
                height: 7,
                round: Round::new(2),
                pol_round: Round::new(1),
                proposer: me,
                block: block.clone(),
            }),
        ];
        for msg in &messages {
            let bytes = msg.encode();
            assert_eq!(NetMsg::decode(&bytes).as_ref(), Some(msg));
            assert_eq!(
                NetMsg::decode(&[bytes.as_slice(), &[0]].concat()),
                None,
                "trailing byte"
            );
            assert_eq!(NetMsg::decode(&bytes[..bytes.len() - 1]), None, "truncated");
        }
        // A body must not smuggle a certificate, and must match its height.
        let mut sealed = block.clone();
        sealed.commit.signatures.push(([0; 32], [0; 64]));
        let bad = NetMsg::Body(Body {
            height: 7,
            round: Round::new(0),
            pol_round: Round::Nil,
            proposer: me,
            block: sealed,
        });
        assert_eq!(NetMsg::decode(&bad.encode()), None);
        let wrong = NetMsg::Body(Body {
            height: 8,
            round: Round::new(0),
            pol_round: Round::Nil,
            proposer: me,
            block,
        });
        assert_eq!(NetMsg::decode(&wrong.encode()), None);
        assert_eq!(NetMsg::decode(b"OVXC\x02\x01"), None, "unknown version");

        let inputs = [
            Input::Vote(vote(VoteType::Prevote, NilOrVal::Val(id))),
            Input::Proposal(proposal),
            Input::ProposedValue(
                ProposedValue {
                    height: Height(7),
                    round: Round::new(2),
                    valid_round: Round::Nil,
                    proposer: me,
                    value: id,
                    validity: Validity::Invalid,
                },
                ValueOrigin::Consensus,
            ),
            Input::TimeoutElapsed(Timeout::new(Round::new(4), TimeoutKind::Precommit)),
            Input::TimeoutElapsed(Timeout::new(
                Round::new(0),
                TimeoutKind::FinalizeHeight(Duration::from_millis(1500)),
            )),
            Input::PolkaCertificate(polka),
            Input::RoundCertificate(round_cert),
        ];
        for input in &inputs {
            let bytes = encode_input(input).expect("logged input");
            assert_eq!(decode_input(&bytes).as_ref(), Some(input));
            assert_eq!(decode_input(&[bytes.as_slice(), &[0]].concat()), None);
        }
    }
}
