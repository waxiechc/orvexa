//! Fixtures were emitted by the original crates before the project rename.
use orvexa_core::{
    Block, GENESIS_SUPPLY, Genesis, Ledger, MAX_BLOCK_TXS, MIN_FEE, SignedTransaction,
};
use orvexa_crypto::SecretKey;
use orvexa_node::Node;
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/v1-protocol.json")).unwrap()
}

#[test]
fn v1_addresses_signing_bytes_signatures_ids_and_economics_are_unchanged() {
    let expected = fixture();
    let genesis: Genesis = serde_json::from_value(expected["genesis"].clone()).unwrap();
    let signed: SignedTransaction =
        serde_json::from_value(expected["transaction"].clone()).unwrap();
    let block: Block = serde_json::from_value(expected["block"].clone()).unwrap();
    let transaction_bytes: Vec<u8> =
        serde_json::from_value(expected["transaction_signing_bytes"].clone()).unwrap();
    let block_bytes: Vec<u8> =
        serde_json::from_value(expected["block_signing_bytes"].clone()).unwrap();
    let sender = SecretKey::from_bytes(&[1; 32]);
    let recipient = SecretKey::from_bytes(&[2; 32]);

    assert_eq!(sender.address().to_string(), expected["sender_address"]);
    assert_eq!(
        recipient.address().to_string(),
        expected["recipient_address"]
    );
    assert_eq!(genesis.chain_id().to_string(), expected["chain_id"]);
    assert_eq!(signed.transaction.signing_bytes(), transaction_bytes);
    assert_eq!(block.header.signing_bytes(), block_bytes);
    assert_eq!(signed.id().unwrap().to_string(), expected["transaction_id"]);
    assert_eq!(block.id().unwrap().to_string(), expected["block_id"]);
    assert_eq!(
        block.header.transactions_hash.to_string(),
        expected["transactions_hash"]
    );
    signed.verify(genesis.chain_id()).unwrap();
    assert_eq!(signed.transaction.clone().sign(&sender).unwrap(), signed);
    assert_eq!(serde_json::to_value(&genesis).unwrap(), expected["genesis"]);
    assert_eq!(
        serde_json::to_value(&signed).unwrap(),
        expected["transaction"]
    );
    assert_eq!(serde_json::to_value(&block).unwrap(), expected["block"]);

    let mut ledger = Ledger::new(genesis).unwrap();
    assert_eq!(ledger.template(vec![signed]).unwrap().sign(&sender), block);
    ledger.apply_block(block).unwrap();
    assert_eq!(ledger.account(sender.address()).balance, 999748);
    assert_eq!(ledger.account(recipient.address()).balance, 250);
    assert_eq!(ledger.burned(), 2);
    assert_eq!(GENESIS_SUPPLY, expected["initial_supply"]);
    assert_eq!(MIN_FEE, expected["minimum_fee"]);
    assert_eq!(MAX_BLOCK_TXS as u64, expected["max_block_transactions"]);
    ledger.check_supply().unwrap();
}

#[test]
fn wallet_created_before_rename_still_authenticates_and_unlocks() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy-wallet.json");
    std::fs::write(&path, include_bytes!("fixtures/v1-encrypted-wallet.json")).unwrap();
    let expected = fixture();
    let before = std::fs::read(&path).unwrap();
    let address = orvexa_wallet::address(&path).unwrap();
    let secret = orvexa_wallet::unlock(&path, "orvexa compatibility test only").unwrap();
    assert_eq!(address.to_string(), expected["wallet_address"]);
    assert_eq!(secret.address(), address);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn v1_sqlite_schema_and_saved_blocks_replay_without_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("legacy.sqlite");
    let expected = fixture();
    // The original v1 schema, containing an original signed block and genesis.
    let database = rusqlite::Connection::open(&path).unwrap();
    database.execute_batch("CREATE TABLE genesis (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL); CREATE TABLE blocks (height INTEGER PRIMARY KEY, json TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    database
        .execute(
            "INSERT INTO genesis(id,json) VALUES(1,?1)",
            [expected["genesis"].to_string()],
        )
        .unwrap();
    database
        .execute(
            "INSERT INTO blocks(height,json) VALUES(1,?1)",
            [expected["block"].to_string()],
        )
        .unwrap();
    drop(database);

    let node = Node::open(&path).unwrap();
    assert_eq!(node.ledger.tip().to_string(), expected["block_id"]);
    assert_eq!(node.ledger.height(), 1);
    assert_eq!(node.ledger.burned(), 2);
    assert!(node.status().network.starts_with("ORVEXA "));
    assert_eq!(node.status().protocol, 1);
}

#[test]
fn original_three_process_demo_database_replays_without_migration() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("original-demo.sqlite");
    // Public chain state copied unchanged from the pre-rename three-process demo.
    std::fs::write(&path, include_bytes!("fixtures/v1-node.db")).unwrap();
    let node = Node::open(&path).unwrap();
    assert_eq!(node.ledger.height(), 1);
    assert_eq!(
        node.ledger.genesis.chain_id().to_string(),
        "d015a69e0e397a2a257f76261003d638985b67ef5ade9bf0d17170df3e08bb84"
    );
    assert_eq!(
        node.ledger.tip().to_string(),
        "f5c2e974811d89fa9caed18d2404a77cf23f3323e63472932f28f1503497e205"
    );
    let recipient = node.ledger.blocks()[0].transactions[0]
        .transaction
        .recipient;
    assert_eq!(node.ledger.account(recipient).balance, 250);
    assert_eq!(node.ledger.burned(), 2);
    assert!(node.status().network.starts_with("ORVEXA "));
}
