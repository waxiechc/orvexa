//! P4 (docs/PRIVACY_TEST_PLAN.md): seed phrase, derivation, diversified
//! receiving, restore + rescan, encrypted file, secrets, pending.

use orvexa_shielded::{
    envelope::Envelope,
    ledger::{Block, Ledger, Params},
    seed_file::{self, SeedFileError},
    seed_wallet::{SeedError, SeedWallet},
    wallet::{build_genesis, WalletError},
};
use std::path::PathBuf;

const SUPPLY: u64 = 1_000_000;

/// Official BIP39 vectors (trezor/python-mnemonic vectors.json, 256-bit, passphrase "TREZOR").
const TREZOR: [(&str, &str, &str); 3] = [
    (
        "0000000000000000000000000000000000000000000000000000000000000000",
        "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art",
        "bda85446c68413707090a52022edd26a1c9462295029f2e60cd7c4f2bbd3097170af7a4d73245cafa9c3cca8d561a7c3de6f5d4a10be8ed2a5e608d68f92fcc8",
    ),
    (
        "7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f7f",
        "legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth useful legal winner thank year wave sausage worth title",
        "bc09fca1804f7e69da93c2f2028eb238c227f2e9dda30cd63699232578480a4021b146ad717fbb7e451ce9eb835f43620bf5c514db0f8add49f5d121449d3e87",
    ),
    (
        "8080808080808080808080808080808080808080808080808080808080808080",
        "letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic avoid letter advice cage absurd amount doctor acoustic bless",
        "c0c519bd0e91a2ed54357d9d1ebef6f5af218a153624cf4f2da911a0ed8f7a09e2ef61af0aca007096df430022f7a2b6fb91661a9589097069720d015e4e982f",
    ),
];

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("orvexa-seed-{}-{}", name, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A chain whose whole supply starts with seed wallet A; every block is kept.
struct SeedNet {
    ledger: Ledger,
    history: Vec<(u64, Vec<Envelope>)>,
    a: SeedWallet,
    b: SeedWallet,
}

impl SeedNet {
    fn new() -> Self {
        let params = Params::test(SUPPLY);
        let mut a = SeedWallet::generate();
        let mut b = SeedWallet::generate();
        let genesis = build_genesis(&params, &[(a.address(), 600_000), (a.address(), 400_000)]);
        let ledger = Ledger::from_genesis(params, &genesis).unwrap();
        let envs = vec![Envelope::decode(&genesis).unwrap()];
        a.scan_block(0, &envs).unwrap();
        b.scan_block(0, &envs).unwrap();
        SeedNet { ledger, history: vec![(0, envs)], a, b }
    }

    fn mine(&mut self, txs: Vec<Envelope>) {
        let height = self.ledger.height() + 1;
        let block = Block { height, prev: self.ledger.tip(), txs: txs.iter().map(Envelope::encode).collect() };
        self.ledger.apply_block(&block).unwrap();
        self.a.scan_block(height, &txs).unwrap();
        self.b.scan_block(height, &txs).unwrap();
        assert_eq!(self.a.anchor().unwrap().to_bytes(), self.ledger.anchor().to_bytes(), "wallet tree = ledger tree");
        self.history.push((height, txs));
    }

    fn rescan(&self, wallet: &mut SeedWallet) {
        for (height, txs) in &self.history {
            wallet.scan_block(*height, txs).unwrap();
        }
    }
}

#[test]
fn bip39_matches_official_vectors() {
    for (entropy, phrase, seed) in TREZOR {
        let m = bip0039::Mnemonic::<bip0039::English>::from_entropy(hex::decode(entropy).unwrap()).unwrap();
        assert_eq!(m.phrase(), phrase);
        assert_eq!(hex::encode(m.to_seed("TREZOR")), seed);
        assert_eq!(SeedWallet::from_phrase(phrase, 0).unwrap().phrase().as_str(), phrase);
    }
}

#[test]
fn phrase_round_trip_and_rejections() {
    let w = SeedWallet::generate();
    let phrase = w.phrase();
    assert_eq!(phrase.split(' ').count(), 24);
    let messy = format!("  {}\n", phrase.replace(' ', "   "));
    let restored = SeedWallet::from_phrase(&messy, 0).unwrap();
    assert_eq!(restored.address(), w.address(), "same phrase, same keys");

    let mut words: Vec<&str> = phrase.split(' ').collect();
    let last = words[23];
    words[23] = if last == "abandon" { "ability" } else { "abandon" };
    assert_eq!(SeedWallet::from_phrase(&words.join(" "), 0).err(), Some(SeedError::InvalidPhrase), "checksum");
    words[23] = "notaword";
    assert_eq!(SeedWallet::from_phrase(&words.join(" "), 0).err(), Some(SeedError::InvalidPhrase), "unknown word");
    let twelve = TREZOR[0].1.split(' ').take(11).chain(["about"]).collect::<Vec<_>>().join(" ");
    assert_eq!(SeedWallet::from_phrase(&twelve, 0).err(), Some(SeedError::InvalidPhrase), "only 24 words");

    let all: Vec<String> = phrase.split(' ').map(str::to_owned).collect();
    assert!(w.verify_words(&[(0, &all[0]), (11, &all[11]), (23, &all[23])]));
    assert!(!w.verify_words(&[(0, &all[0]), (11, &all[12])]));
    assert!(!w.verify_words(&[]));
}

#[test]
fn derivation_is_deterministic_per_account() {
    let phrase = TREZOR[0].1;
    let a0 = SeedWallet::from_phrase(phrase, 0).unwrap();
    assert_eq!(a0.address(), SeedWallet::from_phrase(phrase, 0).unwrap().address());
    assert_ne!(a0.address(), SeedWallet::from_phrase(phrase, 1).unwrap().address());
    assert_ne!(a0.address(), SeedWallet::from_phrase(TREZOR[1].1, 0).unwrap().address());
}

#[test]
fn p4_01_diversified_addresses_one_scan() {
    let mut n = SeedNet::new();
    let addrs = [n.b.address(), n.b.new_address(), n.b.new_address()];
    assert!(addrs[0] != addrs[1] && addrs[1] != addrs[2], "addresses are distinct");
    let chain = n.ledger.chain_id();
    for (i, to) in addrs.iter().enumerate() {
        let tx = n.a.transfer(chain, *to, 10_000 * (i as u64 + 1), 10, 20).unwrap();
        n.mine(vec![tx]);
    }
    assert_eq!(n.b.balance(), 60_000, "one viewing key finds every diversified address");
    assert_eq!(n.a.balance(), SUPPLY - 60_000 - 30);
    assert_eq!(n.a.pending(), 0);
}

#[test]
fn p4_02_tampered_ciphertext_and_strangers_credit_nothing() {
    let mut n = SeedNet::new();
    let tx = n.a.transfer(n.ledger.chain_id(), n.b.address(), 5_000, 10, 20).unwrap();
    // Flip one byte of the first encrypted note, outside the ledger.
    let mut bytes = tx.encode();
    for i in 0..tx.bundle.actions().len() {
        bytes[55 + i * 884 + 160 + 50] ^= 1;
    }
    let tampered = Envelope::decode(&bytes).unwrap();
    let mut b_copy = SeedWallet::from_phrase(&n.b.phrase(), 0).unwrap();
    n.rescan(&mut b_copy);
    b_copy.scan_block(n.ledger.height() + 1, &[tampered]).unwrap();
    assert_eq!(b_copy.balance(), 0, "a modified ciphertext is not credited");

    let mut stranger = SeedWallet::generate();
    n.mine(vec![tx]);
    n.rescan(&mut stranger);
    assert_eq!(stranger.balance(), 0);
    assert!(stranger.notes.is_empty());
}

#[test]
fn p4_03_restore_on_another_device_and_rescan() {
    let mut n = SeedNet::new();
    let chain = n.ledger.chain_id();
    let t1 = n.a.transfer(chain, n.b.address(), 250_000, 10, 20).unwrap();
    n.mine(vec![t1]);
    let t2 = n.b.transfer(chain, n.a.address(), 70_000, 10, 20).unwrap(); // B gets change
    n.mine(vec![t2]);
    let addr = n.b.new_address();
    let t3 = n.a.transfer(chain, addr, 1_234, 10, 20).unwrap();
    n.mine(vec![t3]);

    for original in [&n.a, &n.b] {
        let mut restored = SeedWallet::from_phrase(&original.phrase(), 0).unwrap();
        n.rescan(&mut restored);
        assert_eq!(restored.balance(), original.balance());
        assert_eq!(restored.spent_nullifiers(), original.spent_nullifiers());
        assert_eq!(restored.notes.len(), original.notes.len());
        assert_eq!(restored.anchor().unwrap().to_bytes(), n.ledger.anchor().to_bytes());
    }
    assert_eq!(n.b.balance(), 250_000 - 70_000 - 10 + 1_234);
    assert!(n.b.notes.iter().any(|x| x.scope == orchard::keys::Scope::Internal), "change was found");
    assert_eq!(n.a.balance() + n.b.balance() + n.ledger.burned(), SUPPLY);

    // The restored wallet can spend.
    let mut restored = SeedWallet::from_phrase(&n.b.phrase(), 0).unwrap();
    n.rescan(&mut restored);
    let spend = restored.transfer(chain, n.a.address(), 100, 10, 20).unwrap();
    n.mine(vec![spend]);
}

#[test]
fn p4_05_06_encrypted_file() {
    let dir = scratch("file");
    let path = dir.join("wallet.orvexa-seed");
    let w = SeedWallet::from_phrase(TREZOR[1].1, 0).unwrap();
    let chain = [5u8; 32];
    let password = "correct horse battery staple";

    assert_eq!(seed_file::create(&path, &w, chain, "short").err(), Some(SeedFileError::WeakPassword));
    let meta = seed_file::create(&path, &w, chain, password).unwrap();
    assert_eq!(meta.chain_id, hex::encode(chain));
    let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
    assert_eq!(leftovers.len(), 1, "no temporary file left: {leftovers:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    let (unlocked, _) = seed_file::unlock(&path, password).unwrap();
    assert_eq!(unlocked.phrase().as_str(), TREZOR[1].1);
    assert_eq!(seed_file::unlock(&path, "correct horse battery stapler").err(), Some(SeedFileError::Decrypt));

    let original = std::fs::read(&path).unwrap();
    // Never overwritten, even by a create with another wallet.
    let other = SeedWallet::generate();
    assert_eq!(seed_file::create(&path, &other, chain, password).err(), Some(SeedFileError::AlreadyExists));
    assert_eq!(std::fs::read(&path).unwrap(), original);

    // Authenticated metadata: changing the birthday breaks decryption.
    let text = String::from_utf8(original.clone()).unwrap().replace("\"birthday\": 0", "\"birthday\": 7");
    let tampered = dir.join("tampered");
    std::fs::write(&tampered, text).unwrap();
    assert_eq!(seed_file::unlock(&tampered, password).err(), Some(SeedFileError::Decrypt));
    // Truncated and oversized files.
    std::fs::write(&tampered, &original[..original.len() / 2]).unwrap();
    assert_eq!(seed_file::unlock(&tampered, password).err(), Some(SeedFileError::Format));
    std::fs::write(&tampered, vec![b' '; 9000]).unwrap();
    assert_eq!(seed_file::unlock(&tampered, password).err(), Some(SeedFileError::TooLarge));
    // Unknown format version.
    let text = String::from_utf8(original.clone()).unwrap().replace("\"version\": 2", "\"version\": 3");
    std::fs::write(&tampered, text).unwrap();
    assert_eq!(seed_file::unlock(&tampered, password).err(), Some(SeedFileError::Format));
}

#[test]
fn p4_07_secrets_never_printed() {
    let dir = scratch("secrets");
    let w = SeedWallet::from_phrase(TREZOR[2].1, 0).unwrap();
    let debug = format!("{w:?}");
    for word in ["letter", "advice", "bless", "8080"] {
        assert!(!debug.contains(word), "Debug leaks {word}: {debug}");
    }
    let path = dir.join("w");
    let password = "a very secret password 42";
    seed_file::create(&path, &w, [1; 32], password).unwrap();
    let file = std::fs::read_to_string(&path).unwrap();
    for leak in [TREZOR[2].0, "letter advice", password] {
        assert!(!file.contains(leak), "file leaks plaintext");
    }
    let err = format!("{:?}", seed_file::unlock(&path, "wrong password here").err());
    assert!(!err.contains(password) && !err.contains("wrong password here"));
}

#[test]
fn p4_08_pending_expiry_and_no_double_credit() {
    let mut n = SeedNet::new();
    let chain = n.ledger.chain_id();
    let total = n.a.balance();
    // Sent but never mined, expires after 2 blocks.
    let lost = n.a.transfer(chain, n.b.address(), 1_000, 10, 2).unwrap();
    assert!(n.a.pending() > 0);
    assert_eq!(n.a.balance() + n.a.pending(), total, "pending is separate from confirmed");
    drop(lost);
    n.mine(vec![]);
    n.mine(vec![]);
    assert!(n.a.pending() > 0, "still includable at its expiry height");
    n.mine(vec![]);
    assert_eq!(n.a.pending(), 0, "expired transfer releases its notes");
    assert_eq!(n.a.balance(), total);

    // Scanning a block twice is refused, so nothing is credited twice.
    let tx = n.a.transfer(chain, n.b.address(), 2_000, 10, 20).unwrap();
    n.mine(vec![tx.clone()]);
    let h = n.ledger.height();
    assert_eq!(n.b.scan_block(h, &[tx]), Err(SeedError::OutOfOrder { expected: h + 1, got: h }));
    assert_eq!(n.b.balance(), 2_000);
}

#[test]
fn seed_wallet_amount_checks() {
    let mut n = SeedNet::new();
    let chain = n.ledger.chain_id();
    let to = n.b.address();
    assert_eq!(n.a.transfer(chain, to, 0, 10, 20).err(), Some(WalletError::ZeroAmount));
    assert_eq!(n.a.transfer(chain, to, 1, 0, 20).err(), Some(WalletError::FeeBelowMinimum));
    assert_eq!(n.a.transfer(chain, to, u64::MAX, 10, 20).err(), Some(WalletError::Overflow));
    assert_eq!(n.b.transfer(chain, n.a.address(), 1, 10, 20).err(), Some(WalletError::InsufficientFunds));
}
