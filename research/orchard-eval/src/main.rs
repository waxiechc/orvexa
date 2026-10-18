//! P1 evaluation stand for `orchard = "=0.16.0"` (Ironwood v3 bundles,
//! `OrchardCircuitVersion::PostNu6_3`). Real proofs, real signatures, no mocks.
//!
//! This is NOT an ORVEXA ledger, encoding or adapter. It measures the upstream
//! library and records what it accepts and rejects, so a go/no-go decision can
//! be made from evidence. State rules (nullifier set, anchors, value balance
//! policy) are deliberately absent: the negative checks show where the library
//! stops and a future node must take over.
//!
//! Usage: `orchard-eval` runs every phase in its own child process (so peak RSS
//! is per phase) and prints one JSON report. `orchard-eval phase <name> [k]`
//! runs a single phase.

use incrementalmerkletree::{Hashable, Retention};
use orchard::{
    builder::{Builder, BundleType},
    bundle::{Authorized, BatchValidator, BundleVersion, Flags, TxVersion},
    circuit::{OrchardCircuitVersion, ProvingKey, VerifyingKey},
    keys::{FullViewingKey, PreparedIncomingViewingKey, Scope, SpendingKey},
    note_encryption::IronwoodDomain,
    tree::{MerkleHashOrchard, MerklePath},
    value::NoteValue,
    Address, Bundle, Note, Proof,
};
use rand::{rand_core::UnwrapErr, rngs::SysRng};
use serde_json::{json, Value};
use shardtree::{store::memory::MemoryShardStore, ShardTree};
use std::{process::Command, time::Instant};
use zcash_note_encryption::try_note_decryption;

const CIRCUIT: OrchardCircuitVersion = OrchardCircuitVersion::PostNu6_3;
const TX: TxVersion = TxVersion::V6;
const NOTE_VALUE: u64 = 10_000;
const FEE: u64 = 10;
const ACTION_SIZES: [usize; 3] = [2, 4, 8];
const PROVE_RUNS: usize = 3;

fn rng() -> UnwrapErr<SysRng> {
    UnwrapErr(SysRng)
}

fn version() -> BundleVersion {
    BundleVersion::ironwood_v3()
}

fn ms(start: Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 1e6).round() / 1e3
}

/// Peak resident set size of this process in bytes (ru_maxrss is bytes on
/// macOS, KiB on Linux). Not measured on Windows by this stand.
#[cfg(unix)]
fn peak_rss_bytes() -> Option<u64> {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
    let raw = usage.ru_maxrss as u64;
    Some(if cfg!(target_os = "macos") { raw } else { raw * 1024 })
}

#[cfg(not(unix))]
fn peak_rss_bytes() -> Option<u64> {
    None
}

struct Wallet {
    fvk: FullViewingKey,
    addr: Address,
    ivk: PreparedIncomingViewingKey,
}

fn wallet(seed: u8) -> Wallet {
    let sk = SpendingKey::from_bytes([seed; 32]).unwrap();
    let fvk = FullViewingKey::from(&sk);
    let addr = fvk.address_at(0u32, Scope::External);
    let ivk = PreparedIncomingViewingKey::new(&fvk.to_ivk(Scope::External));
    Wallet { fvk, addr, ivk }
}

fn sighash<A: orchard::bundle::Authorization>(bundle: &Bundle<A, i64>) -> [u8; 32] {
    bundle.commitment(TX).expect("representable flags").into()
}

fn prove_and_sign(
    builder: Builder,
    pk: &ProvingKey,
    saks: &[orchard::keys::SpendAuthorizingKey],
) -> Bundle<Authorized, i64> {
    let (unauthorized, _) = builder.build::<i64>(&mut rng()).unwrap().unwrap();
    let hash = sighash(&unauthorized);
    let proven = unauthorized.create_proof(pk, &mut rng()).unwrap();
    proven.apply_signatures(rng(), hash, saks).unwrap()
}

/// Full single-bundle check: proof, every spend-auth signature, binding
/// signature. Returns the first failing component or "ok".
fn check(bundle: &Bundle<Authorized, i64>, vk: &VerifyingKey, hash: [u8; 32]) -> &'static str {
    if bundle.verify_proof(vk).is_err() {
        return "proof_rejected";
    }
    for action in bundle.actions() {
        if action.rk().verify(&hash, action.authorization()).is_err() {
            return "spend_auth_signature_rejected";
        }
    }
    if bundle
        .binding_validating_key()
        .verify(&hash, bundle.authorization().binding_signature())
        .is_err()
    {
        return "binding_signature_rejected";
    }
    "ok"
}

fn batch_check(bundles: &[(&Bundle<Authorized, i64>, [u8; 32])], vk: &VerifyingKey) -> bool {
    let mut batch = BatchValidator::new(vk);
    for (bundle, hash) in bundles {
        if batch.add_bundle(*bundle, *hash).is_err() {
            return false;
        }
    }
    batch.validate(rng())
}

/// Coinbase-style output-only bundle that mints `k` notes to `to`, then a
/// commitment tree holding them with witnesses for each.
fn fund(
    pk: &ProvingKey,
    to: &Wallet,
    k: usize,
) -> (Bundle<Authorized, i64>, MerkleHashOrchard, Vec<(Note, MerklePath)>) {
    let empty = MerkleHashOrchard::empty_root(32.into()).into();
    let mut builder =
        Builder::new(BundleType::Coinbase, version(), Flags::SPENDS_DISABLED, empty).unwrap();
    for _ in 0..k {
        builder
            .add_output(None, to.addr, NoteValue::from_raw(NOTE_VALUE), [0u8; 512])
            .unwrap();
    }
    let bundle = prove_and_sign(builder, pk, &[]);

    let mut tree: ShardTree<MemoryShardStore<MerkleHashOrchard, u32>, 32, 16> =
        ShardTree::new(MemoryShardStore::empty(), 100);
    let mut notes = Vec::new();
    for action in bundle.actions() {
        let leaf = MerkleHashOrchard::from_cmx(action.cmx());
        tree.append(leaf, Retention::Marked).unwrap();
        if let Some((note, _, _)) =
            try_note_decryption(&IronwoodDomain::for_action(action), &to.ivk, action)
        {
            let position = tree.max_leaf_position(None).unwrap().unwrap();
            notes.push((note, position));
        }
    }
    tree.checkpoint(0).unwrap();
    let root = tree.root_at_checkpoint_id(&0).unwrap().unwrap();
    let witnessed = notes
        .into_iter()
        .map(|(note, position)| {
            let path = tree.witness_at_checkpoint_id(position, &0).unwrap().unwrap();
            (note, MerklePath::from(path))
        })
        .collect();
    (bundle, root, witnessed)
}

/// Private transfer: spends every funded note, pays `k` outputs to `to`, the
/// last one reduced by FEE so value_balance == FEE (leaves the pool as fee).
fn transfer_builder(
    from: &Wallet,
    to: &Wallet,
    root: MerkleHashOrchard,
    notes: &[(Note, MerklePath)],
) -> Builder {
    let mut builder =
        Builder::new(BundleType::DEFAULT, version(), version().default_flags(), root.into())
            .unwrap();
    for (note, path) in notes {
        builder.add_spend(from.fvk.clone(), *note, path.clone()).unwrap();
    }
    for i in 0..notes.len() {
        let value = if i + 1 == notes.len() { NOTE_VALUE - FEE } else { NOTE_VALUE };
        builder
            .add_output(None, to.addr, NoteValue::from_raw(value), [0u8; 512])
            .unwrap();
    }
    builder
}

fn sak(seed: u8) -> orchard::keys::SpendAuthorizingKey {
    (&SpendingKey::from_bytes([seed; 32]).unwrap()).into()
}

/// Byte size using Zcash v5/v6-style field widths. ORVEXA has no encoding yet,
/// so this is an estimate of the cryptographic payload, not a wire format.
fn estimated_bytes(bundle: &Bundle<Authorized, i64>) -> (usize, usize) {
    let action = bundle.actions().first();
    let per_action = 32 * 5
        + action.encrypted_note().enc_ciphertext.as_ref().len()
        + action.encrypted_note().out_ciphertext.len()
        + 64;
    let proof = bundle.authorization().proof().as_ref().len();
    let n = bundle.actions().len();
    let total = 1 + n * per_action + 1 + 8 + 32 + 3 + proof + 64;
    (per_action, total)
}

fn phase_keygen() -> Value {
    let t = Instant::now();
    let _pk = ProvingKey::build(CIRCUIT);
    let pk_ms = ms(t);
    let t = Instant::now();
    let _vk = VerifyingKey::build(CIRCUIT);
    json!({ "proving_key_build_ms": pk_ms, "verifying_key_build_ms": ms(t) })
}

fn phase_transfer(k: usize) -> Value {
    let pk = ProvingKey::build(CIRCUIT);
    let vk = VerifyingKey::build(CIRCUIT);
    let (a, b, c) = (wallet(1), wallet(2), wallet(3));
    let (funding, root, notes) = fund(&pk, &a, k);
    assert_eq!(check(&funding, &vk, sighash(&funding)), "ok");

    let mut prove_ms = Vec::new();
    let mut last = None;
    for _ in 0..PROVE_RUNS {
        let builder = transfer_builder(&a, &b, root, &notes);
        let t = Instant::now();
        let bundle = prove_and_sign(builder, &pk, &[sak(1)]);
        prove_ms.push(ms(t));
        last = Some(bundle);
    }
    let bundle = last.unwrap();
    assert_eq!(bundle.actions().len(), k, "spends == outputs == k gives k actions");
    let hash = sighash(&bundle);

    let mut verify_ms = Vec::new();
    for _ in 0..PROVE_RUNS {
        let t = Instant::now();
        assert_eq!(check(&bundle, &vk, hash), "ok");
        verify_ms.push(ms(t));
    }
    let t = Instant::now();
    assert!(batch_check(&[(&bundle, hash)], &vk));
    let batch_ms = ms(t);

    // Wallet scan: trial decryption of every action by the recipient and by a
    // stranger. The stranger learns nothing; both pay the same per-action cost.
    let scan = |w: &Wallet| {
        let t = Instant::now();
        let found: Vec<u64> = bundle
            .actions()
            .iter()
            .filter_map(|act| try_note_decryption(&IronwoodDomain::for_action(act), &w.ivk, act))
            .map(|(note, _, _)| note.value().inner())
            .collect();
        (ms(t), found)
    };
    let (scan_b_ms, found_b) = scan(&b);
    let (scan_c_ms, found_c) = scan(&c);
    let received: u64 = found_b.iter().sum();
    assert_eq!(received, k as u64 * NOTE_VALUE - FEE);
    assert!(found_c.is_empty());

    let (per_action, total) = estimated_bytes(&bundle);
    json!({
        "actions": k,
        "prove_and_sign_ms": prove_ms,
        "verify_single_ms": verify_ms,
        "verify_batch_of_one_ms": batch_ms,
        "value_balance": *bundle.value_balance(),
        "recipient_scan_ms": scan_b_ms,
        "recipient_found_values_sum": received,
        "stranger_scan_ms": scan_c_ms,
        "stranger_found": found_c.len(),
        "proof_bytes": bundle.authorization().proof().as_ref().len(),
        "estimated_bytes_per_action": per_action,
        "estimated_bundle_bytes": total,
    })
}

fn phase_negative() -> Value {
    let pk = ProvingKey::build(CIRCUIT);
    let vk = VerifyingKey::build(CIRCUIT);
    let (a, b) = (wallet(1), wallet(2));
    let (_, root, notes) = fund(&pk, &a, 2);
    let good = prove_and_sign(transfer_builder(&a, &b, root, &notes), &pk, &[sak(1)]);
    let hash = sighash(&good);
    let mut out = serde_json::Map::new();
    out.insert("valid_bundle".into(), json!(check(&good, &vk, hash)));

    // 1. One flipped bit in the proof.
    let tampered = good.clone().map_authorization(
        &mut (),
        |_, _, sig| sig,
        |_, auth: Authorized| {
            let mut bytes = auth.proof().as_ref().to_vec();
            let middle = bytes.len() / 2;
            bytes[middle] ^= 1;
            Authorized::from_parts(Proof::new(bytes), auth.binding_signature().clone())
        },
    );
    out.insert("flipped_proof_bit".into(), json!(check(&tampered, &vk, hash)));
    out.insert("flipped_proof_bit_batch_accepts".into(), json!(batch_check(&[(&tampered, hash)], &vk)));

    // 2. Truncated proof.
    let truncated = good.clone().map_authorization(
        &mut (),
        |_, _, sig| sig,
        |_, auth: Authorized| {
            let bytes = auth.proof().as_ref()[..64].to_vec();
            Authorized::from_parts(Proof::new(bytes), auth.binding_signature().clone())
        },
    );
    out.insert("truncated_proof".into(), json!(check(&truncated, &vk, hash)));

    // 3. Signatures checked against a different transaction digest.
    let mut other = hash;
    other[0] ^= 1;
    out.insert("signatures_under_other_sighash".into(), json!(check(&good, &vk, other)));

    // 4. Value balance changed after signing (would mint or burn value).
    let inflated = good.clone().try_map_value_balance::<i64, (), _>(|v| Ok(v - 1)).unwrap();
    out.insert("value_balance_changed_after_signing".into(), json!(check(&inflated, &vk, sighash(&inflated))));
    // Same, but the verifier is handed the original digest: only the binding
    // signature (value commitment balance) is left to catch it.
    out.insert("value_balance_changed_original_sighash".into(), json!(check(&inflated, &vk, hash)));

    // 5. Verifying key of a different circuit version.
    let fixed_vk = VerifyingKey::build(OrchardCircuitVersion::FixedPostNu6_2);
    out.insert("verified_with_FixedPostNu6_2_key".into(), json!(check(&good, &fixed_vk, hash)));

    // 6. Wrong spend authorizing key: the builder refuses to sign.
    let (unauthorized, _) = transfer_builder(&a, &b, root, &notes).build::<i64>(&mut rng()).unwrap().unwrap();
    let h = sighash(&unauthorized);
    let proven = unauthorized.create_proof(&pk, &mut rng()).unwrap();
    out.insert("signed_with_wrong_spend_key".into(), json!(match proven.apply_signatures(rng(), h, &[sak(9)]) {
        Ok(_) => "accepted",
        Err(_) => "builder_refused_missing_signature",
    }));

    // 7. Spend with a witness whose root differs from the builder's anchor.
    let wrong_anchor = MerkleHashOrchard::empty_root(32.into());
    let mut builder = Builder::new(BundleType::DEFAULT, version(), version().default_flags(), wrong_anchor.into()).unwrap();
    out.insert("spend_path_not_matching_anchor".into(), json!(match builder.add_spend(a.fvk.clone(), notes[0].0, notes[0].1.clone()) {
        Ok(()) => "builder_accepted".to_string(),
        Err(e) => format!("builder_refused: {e:?}"),
    }));

    // 8. Same note spent twice inside one bundle.
    let mut builder = Builder::new(BundleType::DEFAULT, version(), version().default_flags(), root.into()).unwrap();
    builder.add_spend(a.fvk.clone(), notes[0].0, notes[0].1.clone()).unwrap();
    let second = builder.add_spend(a.fvk.clone(), notes[0].0, notes[0].1.clone());
    let same_bundle = match second {
        Err(e) => json!({ "builder": format!("refused: {e:?}") }),
        Ok(()) => {
            builder.add_output(None, b.addr, NoteValue::from_raw(2 * NOTE_VALUE - FEE), [0u8; 512]).unwrap();
            let bundle = prove_and_sign(builder, &pk, &[sak(1)]);
            let nfs: Vec<_> = bundle.actions().iter().map(|x| x.nullifier().to_bytes()).collect();
            let duplicate = nfs.iter().enumerate().any(|(i, n)| nfs[..i].contains(n));
            json!({ "builder": "accepted", "library_check": check(&bundle, &vk, sighash(&bundle)), "duplicate_nullifier_in_bundle": duplicate })
        }
    };
    out.insert("same_note_twice_in_one_bundle".into(), same_bundle);

    // 9. Same note spent in two different bundles: each is valid alone; only
    // a node's nullifier set can reject the second one.
    let one = |dest: &Wallet| {
        let mut builder = Builder::new(BundleType::DEFAULT, version(), version().default_flags(), root.into()).unwrap();
        builder.add_spend(a.fvk.clone(), notes[0].0, notes[0].1.clone()).unwrap();
        builder.add_output(None, dest.addr, NoteValue::from_raw(NOTE_VALUE - FEE), [0u8; 512]).unwrap();
        prove_and_sign(builder, &pk, &[sak(1)])
    };
    let (x, y) = (one(&b), one(&wallet(3)));
    let nf = |bundle: &Bundle<Authorized, i64>| -> Vec<[u8; 32]> { bundle.actions().iter().map(|a| a.nullifier().to_bytes()).collect() };
    let shared = nf(&x).iter().any(|n| nf(&y).contains(n));
    out.insert("same_note_in_two_bundles".into(), json!({
        "first": check(&x, &vk, sighash(&x)),
        "second": check(&y, &vk, sighash(&y)),
        "share_a_nullifier": shared,
        "batch_of_both_accepts": batch_check(&[(&x, sighash(&x)), (&y, sighash(&y))], &vk),
    }));

    // 10. Outputs worth more than spends: value_balance goes negative. The
    // library accepts it; whether value may enter the pool is ledger policy.
    let mut builder = Builder::new(BundleType::DEFAULT, version(), version().default_flags(), root.into()).unwrap();
    builder.add_spend(a.fvk.clone(), notes[0].0, notes[0].1.clone()).unwrap();
    builder.add_output(None, b.addr, NoteValue::from_raw(NOTE_VALUE * 5), [0u8; 512]).unwrap();
    let mint = prove_and_sign(builder, &pk, &[sak(1)]);
    out.insert("outputs_exceed_spends".into(), json!({
        "library_check": check(&mint, &vk, sighash(&mint)),
        "value_balance": *mint.value_balance(),
    }));

    // 11. Output above the 2^64-1 note range cannot be expressed; largest
    // value builds. Overflow of a sum is the builder's job to refuse.
    let mut builder = Builder::new(BundleType::DEFAULT, version(), version().default_flags(), root.into()).unwrap();
    builder.add_output(None, b.addr, NoteValue::from_raw(u64::MAX), [0u8; 512]).unwrap();
    builder.add_output(None, b.addr, NoteValue::from_raw(u64::MAX), [0u8; 512]).unwrap();
    out.insert("two_max_value_outputs".into(), json!(match builder.build::<i64>(&mut rng()) {
        Ok(_) => "builder_accepted".to_string(),
        Err(e) => format!("builder_refused: {e:?}"),
    }));

    Value::Object(out)
}

fn run_phase(name: &str, k: Option<usize>) -> Value {
    let mut result = match name {
        "keygen" => phase_keygen(),
        "transfer" => phase_transfer(k.expect("k")),
        "negative" => phase_negative(),
        _ => panic!("unknown phase {name}"),
    };
    result["peak_rss_bytes"] = json!(peak_rss_bytes());
    result
}

fn child(name: &str, k: Option<usize>) -> Value {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.arg("phase").arg(name);
    if let Some(k) = k {
        cmd.arg(k.to_string());
    }
    let out = cmd.output().expect("spawn phase");
    assert!(out.status.success(), "{name} failed: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).expect("phase json")
}

fn sh(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("phase") {
        let k = args.get(3).map(|s| s.parse().unwrap());
        println!("{}", run_phase(&args[2], k));
        return;
    }
    let machine = json!({
        "os": format!("{} {}", std::env::consts::OS, sh("uname", &["-r"])),
        "arch": std::env::consts::ARCH,
        "cpu": if cfg!(target_os = "macos") { sh("sysctl", &["-n", "machdep.cpu.brand_string"]) } else { sh("uname", &["-p"]) },
        "logical_cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
    });
    let report = json!({
        "stand": "orchard-eval P1",
        "library": "orchard =0.16.0 (std, circuit, multicore)",
        "bundle_version": "BundleVersion::ironwood_v3()",
        "circuit": "OrchardCircuitVersion::PostNu6_3",
        "tx_version_for_sighash": "TxVersion::V6",
        "machine": machine,
        "keygen": child("keygen", None),
        "transfers": ACTION_SIZES.iter().map(|&k| child("transfer", Some(k))).collect::<Vec<_>>(),
        "negative": child("negative", None),
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
