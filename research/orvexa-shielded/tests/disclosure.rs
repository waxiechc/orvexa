//! Stage 6: one-payment receipts, view-only keys (P4-04), explicit public payments.

use orchard::keys::Scope;
use orvexa_shielded::{
    disclosure::{DisclosureError, PaymentReceipt, PublicConsent, ViewingKey},
    envelope::Envelope,
    ledger::{Block, Ledger, Params},
    seed_file::{self, SeedFileError},
    seed_wallet::{Access, SeedWallet},
    wallet::{build_genesis, WalletError},
};

const SUPPLY: u64 = 1_000_000;

struct Net {
    ledger: Ledger,
    history: Vec<(u64, Vec<Envelope>)>,
    a: SeedWallet,
    b: SeedWallet,
}

impl Net {
    fn new() -> Self {
        let params = Params::test(SUPPLY);
        let (mut a, mut b) = (SeedWallet::generate(), SeedWallet::generate());
        let genesis = build_genesis(&params, &[(a.address(), SUPPLY)]);
        let ledger = Ledger::from_genesis(params, &genesis).unwrap();
        let envs = vec![Envelope::decode(&genesis).unwrap()];
        a.scan_block(0, &envs).unwrap();
        b.scan_block(0, &envs).unwrap();
        Net { ledger, history: vec![(0, envs)], a, b }
    }
    fn chain(&self) -> [u8; 32] {
        self.ledger.chain_id()
    }
    fn mine(&mut self, txs: Vec<Envelope>) {
        let height = self.ledger.height() + 1;
        let block = Block { height, prev: self.ledger.tip(), txs: txs.iter().map(Envelope::encode).collect() };
        self.ledger.apply_block(&block).unwrap();
        self.a.scan_block(height, &txs).unwrap();
        self.b.scan_block(height, &txs).unwrap();
        self.history.push((height, txs));
    }
    fn rescan(&self, w: &mut SeedWallet) {
        for (h, txs) in &self.history {
            w.scan_block(*h, txs).unwrap();
        }
    }
    /// A pays B, B pays some back (so both have change), all mined.
    fn with_history() -> (Self, Envelope) {
        let mut n = Self::new();
        let chain = n.chain();
        let to_b = n.a.transfer(chain, n.b.address(), 12_345, 10, 20).unwrap();
        n.mine(vec![to_b.clone()]);
        let back = n.b.transfer(chain, n.a.address(), 2_000, 10, 20).unwrap();
        n.mine(vec![back]);
        (n, to_b)
    }
}

fn sent_receipt(n: &Net, tx: &Envelope) -> PaymentReceipt {
    let txid = tx.txid();
    let sent = n.a.sent.iter().find(|p| p.txid == txid).expect("sender sees its payment");
    n.a.payment_receipt(n.chain(), txid, sent.action).unwrap()
}

#[test]
fn receipt_from_sender_and_from_receiver_verifies() {
    let (n, tx) = Net::with_history();
    let receipt = sent_receipt(&n, &tx);
    let text = receipt.encode();
    assert!(text.starts_with("orvexa-receipt-v2:"));
    let decoded = PaymentReceipt::decode(&text).unwrap();
    let ok = decoded.verify(&n.ledger, &tx).unwrap();
    assert_eq!(ok.value, 12_345);
    assert_eq!(ok.recipient, n.b.address());
    assert_eq!((ok.height, ok.confirmations), (1, 2));

    // The receiver can prove the same payment from its own note.
    let note = n.b.notes.iter().find(|x| x.txid == tx.txid()).unwrap();
    let by_receiver = n.b.payment_receipt(n.chain(), tx.txid(), note.action).unwrap();
    assert_eq!(by_receiver, receipt);
}

#[test]
fn receipt_cannot_misstate_anything() {
    let (mut n, tx) = Net::with_history();
    let good = sent_receipt(&n, &tx);

    let mut more = good.clone();
    more.value += 1;
    assert_eq!(more.verify(&n.ledger, &tx), Err(DisclosureError::CommitmentMismatch));

    let mut other_recipient = good.clone();
    other_recipient.recipient = n.a.address().to_raw_address_bytes();
    assert_eq!(other_recipient.verify(&n.ledger, &tx), Err(DisclosureError::CommitmentMismatch));

    let mut other_action = good.clone();
    other_action.action = 1 - good.action; // the change output of the same tx
    assert_eq!(other_action.verify(&n.ledger, &tx), Err(DisclosureError::CommitmentMismatch));

    let mut out_of_range = good.clone();
    out_of_range.action = 40;
    assert_eq!(out_of_range.verify(&n.ledger, &tx), Err(DisclosureError::NoSuchAction));

    let mut other_chain = good.clone();
    other_chain.chain_id[0] ^= 1;
    assert_eq!(other_chain.verify(&n.ledger, &tx), Err(DisclosureError::WrongChain));

    let unrelated = n.history[2].1[0].clone();
    assert_eq!(good.verify(&n.ledger, &unrelated), Err(DisclosureError::TxMismatch));

    // A real, valid payment that was never mined proves nothing.
    let chain = n.chain();
    let unmined = n.a.transfer(chain, n.b.address(), 7, 10, 20).unwrap();
    n.a.scan_block(n.ledger.height() + 1, std::slice::from_ref(&unmined)).unwrap();
    let action = n.a.sent.iter().find(|p| p.txid == unmined.txid()).unwrap().action;
    let r = n.a.payment_receipt(chain, unmined.txid(), action).unwrap();
    assert_eq!(r.verify(&n.ledger, &unmined), Err(DisclosureError::TxNotInChain));

    let mut text = good.encode();
    let last = text.pop().unwrap();
    text.push(if last == '0' { '1' } else { '0' });
    assert_eq!(PaymentReceipt::decode(&text), Err(DisclosureError::Checksum));
    assert_eq!(PaymentReceipt::decode("orvexa-receipt-v2:zz"), Err(DisclosureError::Encoding));
}

#[test]
fn receipt_reveals_one_output_only() {
    let (n, tx) = Net::with_history();
    let bytes = sent_receipt(&n, &tx).to_bytes();
    let sender = n.a.address().to_raw_address_bytes();
    let change = n.a.fvk().unwrap().address_at(0u32, Scope::Internal).to_raw_address_bytes();
    for secret in [&sender[..], &change[..]] {
        assert!(!bytes.windows(secret.len()).any(|w| w == secret), "receipt contains sender data");
    }
    let change_value = (SUPPLY - 12_345 - 10).to_le_bytes();
    assert!(!bytes.windows(8).any(|w| w == change_value), "receipt contains the change amount");
}

#[test]
fn p4_04_full_viewing_key_sees_everything_and_cannot_spend() {
    let (n, _) = Net::with_history();
    let key = n.a.viewing_key(true).unwrap();
    assert!(!key.scope_ru().is_empty());
    let text = key.encode();
    assert!(text.starts_with("orvexa-fvk-v2:"));
    let mut watch = SeedWallet::view_only(ViewingKey::decode(&text).unwrap());
    n.rescan(&mut watch);

    assert_eq!(watch.access(), Access::ViewFull);
    assert_eq!(watch.balance(), n.a.balance());
    assert_eq!(watch.spent_nullifiers(), n.a.spent_nullifiers());
    assert_eq!(watch.sent.len(), n.a.sent.len());
    assert!(watch.notes.iter().any(|x| x.scope == Scope::Internal), "sees change");
    assert!(watch.phrase().is_none());
    assert_eq!(watch.transfer(n.chain(), n.b.address(), 1, 10, 20).err(), Some(WalletError::ViewOnly));
    let dir = std::env::temp_dir().join(format!("orvexa-watch-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        seed_file::create(&dir.join("w"), &watch, n.chain(), "long enough password").err(),
        Some(SeedFileError::NoSeed)
    );
}

#[test]
fn p4_04_incoming_key_sees_only_incoming() {
    let (n, _) = Net::with_history();
    let key = n.b.viewing_key(false).unwrap();
    let text = key.encode();
    assert!(text.starts_with("orvexa-ivk-v2:"));
    let mut watch = SeedWallet::view_only(ViewingKey::decode(&text).unwrap());
    n.rescan(&mut watch);

    assert_eq!(watch.access(), Access::ViewIncoming);
    assert_eq!(watch.received(), 12_345, "sees the incoming payment");
    assert_eq!(watch.received(), n.b.received());
    assert_eq!(watch.balance(), None, "no balance without spend visibility");
    assert!(watch.notes.iter().all(|x| x.scope == Scope::External && x.nullifier.is_none()), "no change, no nullifiers");
    assert!(watch.sent.is_empty(), "no outgoing history");
    assert!(n.b.notes.iter().any(|x| x.scope == Scope::Internal), "the owner does have change");
    assert_eq!(watch.transfer(n.chain(), n.a.address(), 1, 10, 20).err(), Some(WalletError::ViewOnly));
    assert_ne!(ViewingKey::Full(n.b.fvk().unwrap().clone()).scope_ru(), key.scope_ru());
}

#[test]
fn viewing_key_text_is_checked() {
    let n = Net::new();
    let text = n.a.viewing_key(true).unwrap().encode();
    let mut bad = text.clone();
    let i = bad.len() - 10;
    bad.replace_range(i..i + 1, if &text[i..i + 1] == "a" { "b" } else { "a" });
    assert!(matches!(ViewingKey::decode(&bad), Err(DisclosureError::Checksum)));
    assert!(matches!(ViewingKey::decode("orvexa-xyz-v2:00"), Err(DisclosureError::Encoding)));
}

#[test]
fn public_payment_needs_matching_consent() {
    let mut n = Net::new();
    let chain = n.chain();
    let to = n.b.address();
    assert_eq!(PublicConsent::REVEALS_RU.len(), 3);

    let wrong = PublicConsent { to, amount: 499 };
    assert_eq!(n.a.public_transfer(chain, &wrong, to, 500, 10, 20).err(), Some(WalletError::ConsentMismatch));
    assert_eq!(n.a.pending(), 0, "nothing was built or reserved");

    let consent = PublicConsent { to, amount: 500 };
    let (tx, receipt) = n.a.public_transfer(chain, &consent, to, 500, 10, 20).unwrap();
    n.mine(vec![tx.clone()]);
    let shown = PaymentReceipt::decode(&receipt.encode()).unwrap().verify(&n.ledger, &tx).unwrap();
    assert_eq!((shown.recipient, shown.value), (to, 500));
}

#[test]
fn a_failed_private_transfer_never_becomes_public() {
    let mut n = Net::new();
    let chain = n.chain();
    let sent_before = n.a.sent.len();
    let to = n.b.address();
    assert_eq!(n.a.transfer(chain, to, SUPPLY, 10, 20).err(), Some(WalletError::InsufficientFunds));
    let consent = PublicConsent { to, amount: SUPPLY };
    assert_eq!(n.a.public_transfer(chain, &consent, to, SUPPLY, 10, 20).err(), Some(WalletError::InsufficientFunds));
    assert_eq!(n.a.sent.len(), sent_before);
    assert_eq!(n.a.pending(), 0);
}
