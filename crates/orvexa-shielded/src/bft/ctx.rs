//! Malachite context for the v2 chain.
//!
//! Consensus agrees on the 32-byte v2 block ID (`ValuePayload::ProposalAndParts`):
//! the proposer gossips the block body separately, every validator runs the
//! full ledger rules on it and only then reports it to consensus as valid or
//! invalid. Votes are signed over [`ledger::vote_sign_bytes`], so the precommits
//! of a decision form the ledger's commit certificate as they are. Signatures
//! are checked with strict Ed25519 (`verify_strict`) — the same rule as the
//! ledger — so a vote consensus accepts can never make a certificate the
//! ledger refuses.

use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use malachitebft_core_types::{
    self as mt, LinearTimeouts, NilOrVal, Round, SignedExtension, SignedMessage, ValidatorProof,
    VoteExtensionScope, VoteType, VotingPower,
};
use malachitebft_signing::{Error as SigningError, VerificationResult, Verifier};

use crate::ledger::{self, Params, PROPOSAL_DOMAIN};

pub use malachitebft_signing_ed25519::{Ed25519, PrivateKey, PublicKey, Signature};

fn short(bytes: &[u8]) -> String {
    bytes[..4].iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Height(pub u64);

impl fmt::Display for Height {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl mt::Height for Height {
    const ZERO: Self = Height(0);
    const INITIAL: Self = Height(1);
    fn increment_by(&self, n: u64) -> Self {
        Height(self.0 + n)
    }
    fn decrement_by(&self, n: u64) -> Option<Self> {
        self.0.checked_sub(n).map(Height)
    }
    fn as_u64(&self) -> u64 {
        self.0
    }
}

/// A validator is addressed by its Ed25519 public key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Address(pub [u8; 32]);

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", short(&self.0))
    }
}

impl mt::Address for Address {}

/// The consensus value: the id of a block whose body travels separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub [u8; 32]);

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "b{}", short(&self.0))
    }
}

impl mt::Value for BlockId {
    type Id = BlockId;
    fn id(&self) -> BlockId {
        *self
    }
}

/// Never streamed through the core state machine (the application gossips
/// bodies itself); required by the `Context` trait.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockPart;

impl mt::ProposalPart<OrvexaCtx> for BlockPart {
    fn is_first(&self) -> bool {
        true
    }
    fn is_last(&self) -> bool {
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub height: Height,
    pub round: Round,
    pub value: BlockId,
    pub pol_round: Round,
    pub proposer: Address,
}

impl Proposal {
    pub fn sign_bytes(&self, chain_id: &[u8; 32]) -> Vec<u8> {
        let mut b = Vec::with_capacity(128);
        b.extend_from_slice(PROPOSAL_DOMAIN);
        b.extend_from_slice(chain_id);
        b.extend_from_slice(&self.height.0.to_be_bytes());
        b.extend_from_slice(&self.round.as_i64().to_be_bytes());
        b.extend_from_slice(&self.pol_round.as_i64().to_be_bytes());
        b.extend_from_slice(&self.value.0);
        b.extend_from_slice(&self.proposer.0);
        b
    }
}

impl mt::Proposal<OrvexaCtx> for Proposal {
    fn height(&self) -> Height {
        self.height
    }
    fn round(&self) -> Round {
        self.round
    }
    fn value(&self) -> &BlockId {
        &self.value
    }
    fn take_value(self) -> BlockId {
        self.value
    }
    fn pol_round(&self) -> Round {
        self.pol_round
    }
    fn validator_address(&self) -> &Address {
        &self.proposer
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    pub typ: VoteType,
    pub height: Height,
    pub round: Round,
    pub value: NilOrVal<BlockId>,
    pub validator: Address,
    pub extension: Option<SignedExtension<OrvexaCtx>>,
}

impl Vote {
    /// The extension is not signed: vote extensions are disabled on v2.
    pub fn sign_bytes(&self, chain_id: &[u8; 32]) -> Vec<u8> {
        let kind = match self.typ {
            VoteType::Prevote => 0,
            VoteType::Precommit => 1,
        };
        let value = match &self.value {
            NilOrVal::Nil => None,
            NilOrVal::Val(id) => Some(&id.0),
        };
        ledger::vote_sign_bytes(
            chain_id,
            kind,
            self.height.0,
            self.round.as_i64(),
            value,
            &self.validator.0,
        )
    }
}

impl mt::Vote<OrvexaCtx> for Vote {
    fn height(&self) -> Height {
        self.height
    }
    fn round(&self) -> Round {
        self.round
    }
    fn value(&self) -> &NilOrVal<BlockId> {
        &self.value
    }
    fn take_value(self) -> NilOrVal<BlockId> {
        self.value
    }
    fn vote_type(&self) -> VoteType {
        self.typ
    }
    fn validator_address(&self) -> &Address {
        &self.validator
    }
    fn extension(&self) -> Option<&SignedExtension<OrvexaCtx>> {
        self.extension.as_ref()
    }
    fn take_extension(&mut self) -> Option<SignedExtension<OrvexaCtx>> {
        self.extension.take()
    }
    fn extend(self, extension: SignedExtension<OrvexaCtx>) -> Self {
        Self {
            extension: Some(extension),
            ..self
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Validator {
    pub address: Address,
    pub public_key: PublicKey,
    pub power: VotingPower,
}

impl Validator {
    pub fn new(public_key: PublicKey, power: VotingPower) -> Self {
        Self {
            address: Address(*public_key.as_bytes()),
            public_key,
            power,
        }
    }
}

impl mt::Validator<OrvexaCtx> for Validator {
    fn address(&self) -> &Address {
        &self.address
    }
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
    fn voting_power(&self) -> VotingPower {
        self.power
    }
}

/// Validators sorted by address; the order fixes the proposer rotation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatorSet(pub Arc<Vec<Validator>>);

impl ValidatorSet {
    pub fn new(mut validators: Vec<Validator>) -> Self {
        assert!(!validators.is_empty());
        validators.sort_by_key(|v| v.address);
        validators.dedup_by_key(|v| v.address);
        Self(Arc::new(validators))
    }

    /// The network's validators, one vote each.
    pub fn from_params(params: &Params) -> Self {
        let validators = params
            .validators
            .iter()
            .map(|k| {
                Validator::new(
                    PublicKey::from_bytes(*k).expect("checked by Params::check_validators"),
                    1,
                )
            })
            .collect();
        Self::new(validators)
    }
}

impl mt::ValidatorSet<OrvexaCtx> for ValidatorSet {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn total_voting_power(&self) -> VotingPower {
        self.0.iter().map(|v| v.power).sum()
    }
    fn get_by_address(&self, address: &Address) -> Option<&Validator> {
        self.0.iter().find(|v| &v.address == address)
    }
    fn get_by_index(&self, index: usize) -> Option<&Validator> {
        self.0.get(index)
    }
}

#[derive(Clone, Debug)]
pub struct OrvexaCtx;

impl OrvexaCtx {
    /// Round robin: (height − 1 + round) mod n.
    pub fn proposer(vs: &ValidatorSet, height: Height, round: Round) -> &Validator {
        let round = round.as_i64().max(0) as u64;
        let index = (height.0.saturating_sub(1) + round) % vs.0.len() as u64;
        &vs.0[index as usize]
    }
}

impl mt::Context for OrvexaCtx {
    type Address = Address;
    type Height = Height;
    type ProposalPart = BlockPart;
    type Proposal = Proposal;
    type Validator = Validator;
    type ValidatorSet = ValidatorSet;
    type Timeouts = LinearTimeouts;
    type Value = BlockId;
    type Vote = Vote;
    type Extension = Vec<u8>;
    type SigningScheme = Ed25519;

    fn select_proposer<'a>(
        &self,
        vs: &'a ValidatorSet,
        height: Height,
        round: Round,
    ) -> &'a Validator {
        Self::proposer(vs, height, round)
    }

    fn new_proposal(
        &self,
        height: Height,
        round: Round,
        value: BlockId,
        pol_round: Round,
        address: Address,
    ) -> Proposal {
        Proposal {
            height,
            round,
            value,
            pol_round,
            proposer: address,
        }
    }

    fn new_prevote(
        &self,
        height: Height,
        round: Round,
        value: NilOrVal<BlockId>,
        address: Address,
    ) -> Vote {
        Vote {
            typ: VoteType::Prevote,
            height,
            round,
            value,
            validator: address,
            extension: None,
        }
    }

    fn new_precommit(
        &self,
        height: Height,
        round: Round,
        value: NilOrVal<BlockId>,
        address: Address,
    ) -> Vote {
        Vote {
            typ: VoteType::Precommit,
            height,
            round,
            value,
            validator: address,
            extension: None,
        }
    }
}

/// Stateless verifier bound to one chain id; the commit-certificate checks of
/// `malachitebft_signing::VerifierExt` come with it.
#[derive(Clone, Debug)]
pub struct Crypto {
    pub chain_id: [u8; 32],
}

impl Crypto {
    pub fn sign_vote(&self, key: &PrivateKey, vote: Vote) -> SignedMessage<OrvexaCtx, Vote> {
        let signature = key.sign(&vote.sign_bytes(&self.chain_id));
        SignedMessage::new(vote, signature)
    }

    pub fn sign_proposal(
        &self,
        key: &PrivateKey,
        proposal: Proposal,
    ) -> SignedMessage<OrvexaCtx, Proposal> {
        let signature = key.sign(&proposal.sign_bytes(&self.chain_id));
        SignedMessage::new(proposal, signature)
    }

    pub fn vote_ok(&self, vote: &Vote, signature: &Signature, key: &PublicKey) -> bool {
        strict(&vote.sign_bytes(&self.chain_id), signature, key)
    }

    pub fn proposal_ok(&self, proposal: &Proposal, signature: &Signature, key: &PublicKey) -> bool {
        strict(&proposal.sign_bytes(&self.chain_id), signature, key)
    }
}

#[async_trait]
impl Verifier<OrvexaCtx> for Crypto {
    async fn verify_signed_vote(
        &self,
        vote: &Vote,
        signature: &Signature,
        key: &PublicKey,
    ) -> Result<VerificationResult, SigningError> {
        Ok(VerificationResult::from_bool(
            self.vote_ok(vote, signature, key),
        ))
    }

    async fn verify_signed_proposal(
        &self,
        proposal: &Proposal,
        signature: &Signature,
        key: &PublicKey,
    ) -> Result<VerificationResult, SigningError> {
        Ok(VerificationResult::from_bool(
            self.proposal_ok(proposal, signature, key),
        ))
    }

    /// Vote extensions are disabled on v2: none is ever accepted.
    async fn verify_signed_vote_extension(
        &self,
        _: &VoteExtensionScope<OrvexaCtx>,
        _: &Vec<u8>,
        _: &Signature,
        _: &PublicKey,
    ) -> Result<VerificationResult, SigningError> {
        Ok(VerificationResult::Invalid)
    }

    /// Peer identity proofs belong to the networking layer, which this stand does not use.
    async fn verify_validator_proof(
        &self,
        _: &ValidatorProof<OrvexaCtx>,
    ) -> Result<VerificationResult, SigningError> {
        Ok(VerificationResult::Invalid)
    }
}

/// Strict Ed25519, as in the ledger (`ed25519-consensus` alone would accept
/// some non-canonical signatures the ledger refuses).
fn strict(msg: &[u8], signature: &Signature, key: &PublicKey) -> bool {
    ed25519_dalek::VerifyingKey::from_bytes(key.as_bytes())
        .and_then(|k| {
            k.verify_strict(
                msg,
                &ed25519_dalek::Signature::from_bytes(&signature.to_bytes()),
            )
        })
        .is_ok()
}
