//! Transparent v1 devnet rules. All amounts are integer test atoms.
//! Legacy `VOID/...` domains are immutable v1 signing/hash identifiers.
use orvexa_crypto::{Address, CryptoError, Hash32, SecretKey, decode, hash, verify};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

pub const VERSION: u32 = 1;
pub const GENESIS_SUPPLY: u64 = 1_000_000;
pub const MIN_FEE: u64 = 1;
pub const MAX_BLOCK_TXS: usize = 128;

#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("unsupported protocol version")]
    Version,
    #[error("wrong network / chain ID")]
    Network,
    #[error("invalid genesis supply")]
    Genesis,
    #[error("amount must be positive and fee at least one atom")]
    Amount,
    #[error("arithmetic overflow")]
    Overflow,
    #[error("nonce mismatch: expected {expected}, received {actual}")]
    Nonce { expected: u64, actual: u64 },
    #[error("insufficient confirmed/pending balance")]
    Funds,
    #[error("invalid block height, parent or logical timestamp")]
    Parent,
    #[error("transaction commitment does not match block body")]
    Commitment,
    #[error("too many transactions in block")]
    BlockSize,
    #[error("wallet is not the genesis authority")]
    Authority,
    #[error("supply conservation invariant violated")]
    Supply,
}

fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend(value.to_le_bytes());
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Genesis {
    pub version: u32,
    pub network_salt: Hash32,
    pub authority: Address,
    pub recipient: Address,
    pub supply: u64,
}
impl Genesis {
    pub fn new(authority: Address, recipient: Address, network_salt: Hash32) -> Self {
        Self {
            version: VERSION,
            network_salt,
            authority,
            recipient,
            supply: GENESIS_SUPPLY,
        }
    }
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.version != VERSION {
            return Err(ProtocolError::Version);
        }
        if self.supply != GENESIS_SUPPLY {
            return Err(ProtocolError::Genesis);
        }
        Ok(())
    }
    pub fn chain_id(&self) -> Hash32 {
        let mut bytes = self.version.to_le_bytes().to_vec();
        bytes.extend(self.network_salt.0);
        bytes.extend(self.authority.public_key());
        bytes.extend(self.recipient.public_key());
        put_u64(&mut bytes, self.supply);
        hash(b"VOID/genesis/v1", &bytes)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub version: u32,
    pub chain_id: Hash32,
    pub sender: Address,
    pub recipient: Address,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
}
impl Transaction {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = b"VOID/transaction/v1".to_vec();
        bytes.extend(self.version.to_le_bytes());
        bytes.extend(self.chain_id.0);
        bytes.extend(self.sender.public_key());
        bytes.extend(self.recipient.public_key());
        put_u64(&mut bytes, self.amount);
        put_u64(&mut bytes, self.fee);
        put_u64(&mut bytes, self.nonce);
        bytes
    }
    pub fn sign(self, key: &SecretKey) -> Result<SignedTransaction, ProtocolError> {
        if self.sender != key.address() {
            return Err(ProtocolError::Authority);
        }
        let signature = key.sign(&self.signing_bytes());
        Ok(SignedTransaction {
            transaction: self,
            signature,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedTransaction {
    pub transaction: Transaction,
    pub signature: String,
}
impl SignedTransaction {
    pub fn verify(&self, chain_id: Hash32) -> Result<(), ProtocolError> {
        let tx = &self.transaction;
        if tx.version != VERSION {
            return Err(ProtocolError::Version);
        }
        if tx.chain_id != chain_id {
            return Err(ProtocolError::Network);
        }
        if tx.amount == 0 || tx.fee < MIN_FEE {
            return Err(ProtocolError::Amount);
        }
        tx.amount
            .checked_add(tx.fee)
            .ok_or(ProtocolError::Overflow)?;
        verify(tx.sender, &tx.signing_bytes(), &self.signature)?;
        Ok(())
    }
    pub fn id(&self) -> Result<Hash32, ProtocolError> {
        let mut bytes = self.transaction.signing_bytes();
        bytes.extend(decode::<64>(&self.signature)?);
        Ok(hash(b"VOID/tx-id/v1", &bytes))
    }
}

pub fn transaction_commitment(txs: &[SignedTransaction]) -> Result<Hash32, ProtocolError> {
    if txs.len() > MAX_BLOCK_TXS {
        return Err(ProtocolError::BlockSize);
    }
    let mut bytes = Vec::new();
    put_u64(&mut bytes, txs.len() as u64);
    for tx in txs {
        bytes.extend(tx.id()?.0);
    }
    Ok(hash(b"VOID/ordered-transactions/v1", &bytes))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockHeader {
    pub version: u32,
    pub chain_id: Hash32,
    pub height: u64,
    pub parent: Hash32,
    /// Logical slot counter; deliberately not a claim about wall-clock time.
    pub timestamp: u64,
    pub transactions_hash: Hash32,
}
impl BlockHeader {
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut bytes = b"VOID/block-header/v1".to_vec();
        bytes.extend(self.version.to_le_bytes());
        bytes.extend(self.chain_id.0);
        put_u64(&mut bytes, self.height);
        bytes.extend(self.parent.0);
        put_u64(&mut bytes, self.timestamp);
        bytes.extend(self.transactions_hash.0);
        bytes
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<SignedTransaction>,
    pub signature: String,
}
impl Block {
    pub fn sign(mut self, key: &SecretKey) -> Self {
        self.signature = key.sign(&self.header.signing_bytes());
        self
    }
    pub fn id(&self) -> Result<Hash32, ProtocolError> {
        let mut bytes = self.header.signing_bytes();
        bytes.extend(decode::<64>(&self.signature)?);
        Ok(hash(b"VOID/block-id/v1", &bytes))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub balance: u64,
    pub nonce: u64,
}

#[derive(Clone, Debug)]
pub struct Ledger {
    pub genesis: Genesis,
    accounts: BTreeMap<Address, Account>,
    blocks: Vec<Block>,
    burned: u64,
}
impl Ledger {
    pub fn new(genesis: Genesis) -> Result<Self, ProtocolError> {
        genesis.validate()?;
        let mut accounts = BTreeMap::new();
        accounts.insert(
            genesis.recipient,
            Account {
                balance: genesis.supply,
                nonce: 0,
            },
        );
        Ok(Self {
            genesis,
            accounts,
            blocks: Vec::new(),
            burned: 0,
        })
    }
    pub fn account(&self, address: Address) -> Account {
        self.accounts.get(&address).copied().unwrap_or_default()
    }
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }
    pub fn height(&self) -> u64 {
        self.blocks.len() as u64
    }
    pub fn burned(&self) -> u64 {
        self.burned
    }
    pub fn tip(&self) -> Hash32 {
        self.blocks
            .last()
            .map(|b| b.id().expect("validated block ID"))
            .unwrap_or_else(|| self.genesis.chain_id())
    }
    /// Only used on a temporary projection. Public block application is atomic.
    fn apply_transaction(&mut self, signed: &SignedTransaction) -> Result<(), ProtocolError> {
        signed.verify(self.genesis.chain_id())?;
        let tx = &signed.transaction;
        let sender = self.account(tx.sender);
        if sender.nonce != tx.nonce {
            return Err(ProtocolError::Nonce {
                expected: sender.nonce,
                actual: tx.nonce,
            });
        }
        let cost = tx
            .amount
            .checked_add(tx.fee)
            .ok_or(ProtocolError::Overflow)?;
        let balance = sender
            .balance
            .checked_sub(cost)
            .ok_or(ProtocolError::Funds)?;
        let nonce = sender.nonce.checked_add(1).ok_or(ProtocolError::Overflow)?;
        let recipient_balance = if tx.sender == tx.recipient {
            balance
        } else {
            self.account(tx.recipient).balance
        };
        let recipient_balance = recipient_balance
            .checked_add(tx.amount)
            .ok_or(ProtocolError::Overflow)?;
        let burned = self
            .burned
            .checked_add(tx.fee)
            .ok_or(ProtocolError::Overflow)?;
        self.accounts.insert(tx.sender, Account { balance, nonce });
        let recipient = self.accounts.entry(tx.recipient).or_default();
        recipient.balance = recipient_balance;
        self.burned = burned;
        Ok(())
    }
    pub fn project(&self, transactions: &[SignedTransaction]) -> Result<Self, ProtocolError> {
        let mut staged = self.clone();
        for tx in transactions {
            staged.apply_transaction(tx)?;
        }
        staged.check_supply()?;
        Ok(staged)
    }
    pub fn template(&self, transactions: Vec<SignedTransaction>) -> Result<Block, ProtocolError> {
        self.project(&transactions)?;
        let height = self
            .height()
            .checked_add(1)
            .ok_or(ProtocolError::Overflow)?;
        let timestamp = self
            .blocks
            .last()
            .map_or(0, |b| b.header.timestamp)
            .checked_add(1)
            .ok_or(ProtocolError::Overflow)?;
        Ok(Block {
            header: BlockHeader {
                version: VERSION,
                chain_id: self.genesis.chain_id(),
                height,
                parent: self.tip(),
                timestamp,
                transactions_hash: transaction_commitment(&transactions)?,
            },
            transactions,
            signature: String::new(),
        })
    }
    pub fn apply_block(&mut self, block: Block) -> Result<(), ProtocolError> {
        let header = &block.header;
        if header.version != VERSION {
            return Err(ProtocolError::Version);
        }
        if header.chain_id != self.genesis.chain_id() {
            return Err(ProtocolError::Network);
        }
        let previous_time = self.blocks.last().map_or(0, |b| b.header.timestamp);
        if header.height
            != self
                .height()
                .checked_add(1)
                .ok_or(ProtocolError::Overflow)?
            || header.parent != self.tip()
            || header.timestamp <= previous_time
        {
            return Err(ProtocolError::Parent);
        }
        if header.transactions_hash != transaction_commitment(&block.transactions)? {
            return Err(ProtocolError::Commitment);
        }
        verify(
            self.genesis.authority,
            &header.signing_bytes(),
            &block.signature,
        )?;
        let mut staged = self.project(&block.transactions)?;
        staged.blocks.push(block);
        *self = staged;
        Ok(())
    }
    pub fn check_supply(&self) -> Result<(), ProtocolError> {
        let total = self.accounts.values().try_fold(self.burned, |total, a| {
            total.checked_add(a.balance).ok_or(ProtocolError::Overflow)
        })?;
        if total != self.genesis.supply {
            return Err(ProtocolError::Supply);
        }
        Ok(())
    }
}
