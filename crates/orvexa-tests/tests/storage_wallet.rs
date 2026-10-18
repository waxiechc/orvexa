use orvexa_core::{Genesis, Transaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey};
use orvexa_node::Node;
use std::fs;
use tempfile::tempdir;

#[test]
fn encrypted_wallet_roundtrip_wrong_password_and_tampering() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("alice.wallet.json");
    let password = "test-only long password";
    let address = orvexa_wallet::create(&path, password).unwrap();
    assert_eq!(orvexa_wallet::address(&path).unwrap(), address);
    assert_eq!(
        orvexa_wallet::unlock(&path, password).unwrap().address(),
        address
    );
    assert!(orvexa_wallet::unlock(&path, "wrong password").is_err());
    assert!(orvexa_wallet::create(&path, password).is_err());
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let ciphertext = value["ciphertext"].as_str().unwrap();
    assert_eq!(ciphertext.len(), 96);
    let replacement = if ciphertext.starts_with('0') {
        "1"
    } else {
        "0"
    };
    value["ciphertext"] = format!("{replacement}{}", &ciphertext[1..]).into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(orvexa_wallet::unlock(&path, password).is_err());
}
#[test]
fn wallet_authenticates_public_metadata_and_rejects_kdf_resource_attack() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("wallet.json");
    let password = "test-only long password";
    orvexa_wallet::create(&path, password).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let mut value = original.clone();
    value["metadata"]["address"] = SecretKey::from_bytes(&[4; 32]).address().to_string().into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(orvexa_wallet::unlock(&path, password).is_err());
    let mut value = original;
    value["metadata"]["memory_kib"] = u32::MAX.into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(orvexa_wallet::unlock(&path, password).is_err());
    assert!(orvexa_wallet::create(&directory.path().join("weak.json"), "short").is_err());
}
#[test]
fn restart_replays_signed_blocks_and_rejects_disk_corruption() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("node.sqlite");
    let alice = SecretKey::from_bytes(&[1; 32]);
    let bob = SecretKey::from_bytes(&[2; 32]);
    let genesis = Genesis::new(alice.address(), alice.address(), Hash32([9; 32]));
    let mut node = Node::create(&path, genesis.clone()).unwrap();
    assert!(Node::create(&path, genesis).is_err());
    assert!(
        Node::open(&path).is_err(),
        "second process must not open this database"
    );
    let signed = Transaction {
        version: VERSION,
        chain_id: node.ledger.genesis.chain_id(),
        sender: alice.address(),
        recipient: bob.address(),
        amount: 50,
        fee: 1,
        nonce: 0,
    }
    .sign(&alice)
    .unwrap();
    node.submit(signed.clone()).unwrap();
    assert!(node.submit(signed.clone()).is_err());
    let block = node.template().unwrap().sign(&alice);
    let tip = node.commit(block.clone()).unwrap();
    assert!(node.mempool().is_empty());
    assert!(node.submit(signed).is_err());
    drop(node);
    let node = Node::open(&path).unwrap();
    assert_eq!(node.ledger.tip(), tip);
    assert_eq!(node.ledger.height(), 1);
    assert_eq!(node.ledger.account(bob.address()).balance, 50);
    assert_eq!(node.ledger.burned(), 1);
    drop(node);
    let db = rusqlite::Connection::open(&path).unwrap();
    let mut corrupt = block;
    corrupt.transactions[0].transaction.amount = 51;
    db.execute(
        "UPDATE blocks SET json=?1 WHERE height=1",
        [serde_json::to_string(&corrupt).unwrap()],
    )
    .unwrap();
    drop(db);
    assert!(Node::open(&path).is_err());
}
#[test]
fn pending_transactions_are_volatile_and_revalidated_after_foreign_block() {
    let directory = tempdir().unwrap();
    let path = directory.path().join("node.sqlite");
    let key = SecretKey::from_bytes(&[1; 32]);
    let other = SecretKey::from_bytes(&[2; 32]);
    let genesis = Genesis::new(key.address(), key.address(), Hash32([8; 32]));
    let mut node = Node::create(&path, genesis).unwrap();
    let make = |amount| {
        Transaction {
            version: VERSION,
            chain_id: node.ledger.genesis.chain_id(),
            sender: key.address(),
            recipient: other.address(),
            amount,
            fee: 1,
            nonce: 0,
        }
        .sign(&key)
        .unwrap()
    };
    let pending = make(50);
    let alternative = make(60);
    node.submit(pending.clone()).unwrap();
    let block = node.ledger.template(vec![alternative]).unwrap().sign(&key);
    node.commit(block).unwrap();
    assert!(node.mempool().is_empty());
    let next = Transaction {
        version: VERSION,
        chain_id: node.ledger.genesis.chain_id(),
        sender: key.address(),
        recipient: other.address(),
        amount: 10,
        fee: 1,
        nonce: 1,
    }
    .sign(&key)
    .unwrap();
    node.submit(next).unwrap();
    drop(node);
    let node = Node::open(&path).unwrap();
    assert!(node.mempool().is_empty());
    assert_eq!(node.ledger.account(other.address()).balance, 60);
}
