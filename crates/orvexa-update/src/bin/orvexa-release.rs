//! orvexa-release — signs ORVEXA Desktop update manifests.
//!
//!   orvexa-release keygen --key ~/.orvexa-release/update.key
//!       Creates the update signing key (never overwritten) and prints its public key.
//!       Keep the key file offline and backed up: without it no installed app
//!       will accept a new update.
//!
//!   orvexa-release manifest --key KEY --release 0.1.0-desktop-preview.5
//!       --mac release/macos-…/ORVEXA-macOS-universal.zip --win release/windows-…/ORVEXA.exe
//!       --notes-ru "…" --notes-en "…" [--days 30] --out docs/updates/preview.json
//!       Writes the manifest and `<out>.sig`. Files must be uploaded to the
//!       GitHub release v<release> under the same names.

use ed25519_dalek::SigningKey;
use orvexa_update::manifest::{ALLOWED_PREFIX, Artifact, FORMAT, Manifest, VERSION, compare, sign};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::Path;

fn usage() -> ! {
    eprintln!("usage: orvexa-release keygen --key FILE");
    eprintln!(
        "       orvexa-release manifest --key FILE --release VERSION --mac ZIP --win EXE --notes-ru TEXT --notes-en TEXT [--days N] --out FILE"
    );
    std::process::exit(2)
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("orvexa-release: {message}");
    std::process::exit(1)
}

fn read_key(path: &str) -> SigningKey {
    let raw: [u8; 32] = std::fs::read(path)
        .unwrap_or_else(|e| fail(e))
        .try_into()
        .unwrap_or_else(|_| fail("key file must be 32 bytes"));
    SigningKey::from_bytes(&raw)
}

fn artifact(path: &str, release: &str, base: &str) -> Artifact {
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(format!("{path}: {e}")));
    let name = Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_else(|| fail("bad file name"));
    Artifact {
        url: format!("{base}v{release}/{name}"),
        sha256: hex::encode(Sha256::digest(&bytes)),
        bytes: bytes.len() as u64,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else { usage() };
    let mut f = HashMap::new();
    let mut it = args[1..].iter();
    while let Some(k) = it.next() {
        let name = k.strip_prefix("--").unwrap_or_else(|| usage());
        f.insert(
            name.to_owned(),
            it.next().unwrap_or_else(|| usage()).clone(),
        );
    }
    let get = |n: &str| f.get(n).cloned().unwrap_or_else(|| usage());
    match command.as_str() {
        "keygen" => {
            let path = get("key");
            if let Some(dir) = Path::new(&path).parent() {
                std::fs::create_dir_all(dir).unwrap_or_else(|e| fail(e));
            }
            let mut raw = [0u8; 32];
            getrandom::getrandom(&mut raw).unwrap_or_else(|e| fail(e));
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&path)
                .unwrap_or_else(|e| fail(format!("{path}: {e} (keys are never overwritten)")));
            file.write_all(&raw).unwrap_or_else(|e| fail(e));
            file.sync_all().unwrap_or_else(|e| fail(e));
            println!(
                "{}",
                hex::encode(SigningKey::from_bytes(&raw).verifying_key().to_bytes())
            );
        }
        "manifest" => {
            let key = read_key(&get("key"));
            let release = get("release");
            if compare(&release, &release).is_none() {
                fail("release must look like 0.1.0-desktop-preview.5");
            }
            let days: u64 = f
                .get("days")
                .map_or(30, |d| d.parse().unwrap_or_else(|_| usage()));
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_secs();
            let mut files = BTreeMap::new();
            // Default: this repository's GitHub release assets (the only prefix apps accept).
            let base = f
                .get("base-url")
                .cloned()
                .unwrap_or_else(|| ALLOWED_PREFIX.to_owned());
            files.insert(
                "macos-universal".to_owned(),
                artifact(&get("mac"), &release, &base),
            );
            files.insert(
                "windows-x64".to_owned(),
                artifact(&get("win"), &release, &base),
            );
            let manifest = Manifest {
                format: FORMAT.into(),
                version: VERSION,
                channel: "preview".into(),
                release: release.clone(),
                expires: now + days * 86_400,
                notes_ru: get("notes-ru"),
                notes_en: get("notes-en"),
                files,
            };
            let bytes = serde_json::to_vec_pretty(&manifest).expect("plain struct");
            let out = get("out");
            std::fs::write(&out, &bytes).unwrap_or_else(|e| fail(e));
            std::fs::write(format!("{out}.sig"), sign(&bytes, &key)).unwrap_or_else(|e| fail(e));
            println!("signed {release} → {out} (expires in {days} days)");
        }
        _ => usage(),
    }
}
