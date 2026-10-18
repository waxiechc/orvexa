use orvexa_core::{
    Block, Genesis, Ledger, MAX_BLOCK_TXS, ProtocolError, SignedTransaction, Transaction, VERSION,
};
use orvexa_crypto::{Hash32, SecretKey};

fn fixture() -> (SecretKey, SecretKey, Ledger) {
    let alice = SecretKey::from_bytes(&[1; 32]);
    let bob = SecretKey::from_bytes(&[2; 32]);
    let ledger = Ledger::new(Genesis::new(
        alice.address(),
        alice.address(),
        Hash32([3; 32]),
    ))
    .unwrap();
    (alice, bob, ledger)
}
fn tx(
    alice: &SecretKey,
    bob: &SecretKey,
    ledger: &Ledger,
    amount: u64,
    nonce: u64,
) -> SignedTransaction {
    Transaction {
        version: VERSION,
        chain_id: ledger.genesis.chain_id(),
        sender: alice.address(),
        recipient: bob.address(),
        amount,
        fee: 1,
        nonce,
    }
    .sign(alice)
    .unwrap()
}
fn signed_block(ledger: &Ledger, key: &SecretKey, transactions: Vec<SignedTransaction>) -> Block {
    ledger.template(transactions).unwrap().sign(key)
}

#[test]
fn transfer_burns_exact_fee_and_preserves_supply() {
    let (alice, bob, mut ledger) = fixture();
    let signed = tx(&alice, &bob, &ledger, 100, 0);
    let block = signed_block(&ledger, &alice, vec![signed.clone()]);
    ledger.apply_block(block).unwrap();
    assert_eq!(ledger.account(alice.address()).balance, 999899);
    assert_eq!(ledger.account(alice.address()).nonce, 1);
    assert_eq!(ledger.account(bob.address()).balance, 100);
    assert_eq!(ledger.burned(), 1);
    ledger.check_supply().unwrap();
    assert!(matches!(
        ledger.project(&[signed]),
        Err(ProtocolError::Nonce { .. })
    ));
}
#[test]
fn rejects_bad_signature_and_wrong_network() {
    let (alice, bob, ledger) = fixture();
    let mut signed = tx(&alice, &bob, &ledger, 10, 0);
    signed.transaction.amount = 11;
    assert!(ledger.project(&[signed]).is_err());
    let mut transaction = tx(&alice, &bob, &ledger, 10, 0).transaction;
    transaction.chain_id = Hash32([9; 32]);
    assert!(matches!(
        ledger.project(&[transaction.sign(&alice).unwrap()]),
        Err(ProtocolError::Network)
    ));
}
#[test]
fn rejects_future_nonce_insufficient_funds_and_overflow() {
    let (alice, bob, ledger) = fixture();
    assert!(matches!(
        ledger.project(&[tx(&alice, &bob, &ledger, 1, 1)]),
        Err(ProtocolError::Nonce { .. })
    ));
    assert!(matches!(
        ledger.project(&[tx(&alice, &bob, &ledger, 1_000_000, 0)]),
        Err(ProtocolError::Funds)
    ));
    assert!(matches!(
        ledger.project(&[tx(&alice, &bob, &ledger, u64::MAX, 0)]),
        Err(ProtocolError::Overflow)
    ));
    assert!(matches!(
        ledger.project(&[tx(&alice, &bob, &ledger, 0, 0)]),
        Err(ProtocolError::Amount)
    ));
}
#[test]
fn rejects_zero_fee_unsupported_version_and_spoofed_sender() {
    let (alice, bob, ledger) = fixture();
    let mut transaction = tx(&alice, &bob, &ledger, 10, 0).transaction;
    transaction.fee = 0;
    assert!(matches!(
        ledger.project(&[transaction.clone().sign(&alice).unwrap()]),
        Err(ProtocolError::Amount)
    ));
    transaction.fee = 1;
    transaction.version = 2;
    assert!(matches!(
        ledger.project(&[transaction.clone().sign(&alice).unwrap()]),
        Err(ProtocolError::Version)
    ));
    assert!(transaction.sign(&bob).is_err());
}
#[test]
fn pending_sequence_cannot_overspend_or_replay() {
    let (alice, bob, ledger) = fixture();
    let first = tx(&alice, &bob, &ledger, 600000, 0);
    let second = tx(&alice, &bob, &ledger, 600000, 1);
    assert!(ledger.project(&[first.clone(), second]).is_err());
    assert!(ledger.project(&[first.clone(), first]).is_err());
}
#[test]
fn self_transfer_only_burns_fee() {
    let (alice, _, mut ledger) = fixture();
    let signed = tx(&alice, &alice, &ledger, 100, 0);
    let block = signed_block(&ledger, &alice, vec![signed]);
    ledger.apply_block(block).unwrap();
    assert_eq!(ledger.account(alice.address()).balance, 999999);
    assert_eq!(ledger.account(alice.address()).nonce, 1);
    ledger.check_supply().unwrap();
}
#[test]
fn can_spend_entire_balance_including_fee() {
    let (alice, bob, mut ledger) = fixture();
    let block = signed_block(&ledger, &alice, vec![tx(&alice, &bob, &ledger, 999999, 0)]);
    ledger.apply_block(block).unwrap();
    assert_eq!(ledger.account(alice.address()).balance, 0);
    assert_eq!(ledger.account(bob.address()).balance, 999999);
}
#[test]
fn block_rejects_wrong_authority_parent_and_changed_body() {
    let (alice, bob, mut ledger) = fixture();
    let original_tip = ledger.tip();
    let signed = tx(&alice, &bob, &ledger, 10, 0);
    let wrong_authority = signed_block(&ledger, &bob, vec![signed.clone()]);
    assert!(ledger.apply_block(wrong_authority).is_err());
    let block = signed_block(&ledger, &alice, vec![signed]);
    let mut wrong_parent = block.clone();
    wrong_parent.header.parent = Hash32::ZERO;
    assert!(ledger.apply_block(wrong_parent.sign(&alice)).is_err());
    let mut wrong_body = block.clone();
    wrong_body.transactions.clear();
    assert!(matches!(
        ledger.apply_block(wrong_body),
        Err(ProtocolError::Commitment)
    ));
    assert_eq!(ledger.tip(), original_tip);
    assert_eq!(ledger.account(alice.address()).balance, 1_000_000);
    ledger.apply_block(block.clone()).unwrap();
    assert!(ledger.apply_block(block).is_err());
}
#[test]
fn whole_block_is_atomic_when_later_transaction_is_invalid() {
    let (alice, bob, mut ledger) = fixture();
    let first = tx(&alice, &bob, &ledger, 10, 0);
    let mut block = signed_block(&ledger, &alice, vec![first.clone()]);
    block.transactions.push(first);
    block.header.transactions_hash =
        orvexa_core::transaction_commitment(&block.transactions).unwrap();
    assert!(ledger.apply_block(block.sign(&alice)).is_err());
    assert_eq!(ledger.height(), 0);
    assert_eq!(ledger.burned(), 0);
    assert_eq!(ledger.account(bob.address()).balance, 0);
}
#[test]
fn empty_block_is_valid_but_size_time_and_height_are_checked() {
    let (alice, bob, mut ledger) = fixture();
    let empty = signed_block(&ledger, &alice, vec![]);
    let mut bad_time = empty.clone();
    bad_time.header.timestamp = 0;
    assert!(ledger.apply_block(bad_time.sign(&alice)).is_err());
    let mut bad_height = empty.clone();
    bad_height.header.height = 2;
    assert!(ledger.apply_block(bad_height.sign(&alice)).is_err());
    let many = vec![tx(&alice, &bob, &ledger, 1, 0); MAX_BLOCK_TXS + 1];
    assert!(matches!(
        orvexa_core::transaction_commitment(&many),
        Err(ProtocolError::BlockSize)
    ));
    ledger.apply_block(empty).unwrap();
    assert_eq!(ledger.height(), 1);
}
#[test]
fn genesis_supply_and_version_are_not_runtime_settings() {
    let (_, _, ledger) = fixture();
    let mut genesis = ledger.genesis.clone();
    genesis.supply += 1;
    assert!(Ledger::new(genesis).is_err());
    let mut genesis = ledger.genesis;
    genesis.version += 1;
    assert!(Ledger::new(genesis).is_err());
}
#[test]
fn canonical_signatures_survive_json_reordering_and_unknown_fields_fail() {
    let (alice, bob, ledger) = fixture();
    let signed = tx(&alice, &bob, &ledger, 123, 0);
    let value = serde_json::to_value(&signed).unwrap();
    let recovered: SignedTransaction = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        signed.transaction.signing_bytes(),
        recovered.transaction.signing_bytes()
    );
    assert_eq!(signed.id().unwrap(), recovered.id().unwrap());
    recovered.verify(ledger.genesis.chain_id()).unwrap();
    let mut unknown = value;
    unknown["admin"] = true.into();
    assert!(serde_json::from_value::<SignedTransaction>(unknown).is_err());
}
