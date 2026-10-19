//! orvexa-v2-node — private v2 test network node for operators.
//!
//!   orvexa-v2-node keygen --out validator.key
//!   orvexa-v2-node genesis --to <ovxt1… address> --out network.json
//!                          (--validators KEY[@URL],KEY[@URL]… | --validator-key FILE)
//!                          [--name "ORVEXA v2 test"] [--nodes https://a.example,…] [--supply N]
//!   orvexa-v2-node run --data-dir DIR --network network.json [--listen 127.0.0.1:4260]
//!                      (--validator-key FILE [--block-every-ms N] [--timeouts-ms N] | [--follow URL] [--poll-ms N])
//!
//! Each validator creates its own key with `keygen` and hands out only the
//! public key; one operator then writes the network file with all of them.
//! More than 2/3 of the validators must be online for blocks to be decided.
//! Test only.

use orvexa_shielded::{
    address,
    bft::engine::{default_timeouts, EngineConfig},
    ledger::Params,
    node::{run, NodeConfig, Role, ValidatorKey},
    profile::{create_validator_key, read_validator_key, NetworkProfile},
    wallet::build_genesis,
    Envelope,
};
use rand::rand_core::Rng as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn usage() -> ! {
    eprintln!("usage: orvexa-v2-node keygen --out validator.key");
    eprintln!("       orvexa-v2-node genesis --to <ovxt1-address> --out network.json (--validators KEY[@URL],… | --validator-key FILE) [--name NAME] [--nodes URL,URL] [--supply N]");
    eprintln!("       orvexa-v2-node run --data-dir DIR --network network.json [--listen ADDR] (--validator-key FILE [--block-every-ms N] [--timeouts-ms N] | [--follow URL] [--poll-ms N])");
    std::process::exit(2)
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("ORVEXA v2: {message}");
    std::process::exit(1)
}

fn flags(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(key) = it.next() {
        let Some(name) = key.strip_prefix("--") else {
            usage()
        };
        let Some(value) = it.next() else { usage() };
        map.insert(name.to_owned(), value.clone());
    }
    map
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else { usage() };
    let f = flags(&args[1..]);
    let get = |name: &str| f.get(name).cloned().unwrap_or_else(|| usage());
    match command.as_str() {
        "keygen" => {
            let out = get("out");
            let key = create_validator_key(Path::new(&out)).unwrap_or_else(|e| fail(e));
            println!("{}", hex::encode(key.verifying_key().to_bytes()));
            eprintln!("validator key written to {out} (keep it secret); public key above");
        }
        "genesis" => {
            let to = address::decode(&get("to"))
                .unwrap_or_else(|| fail("--to must be an ovxt1… address"));
            // (public key, node URL) of every validator.
            let validators: Vec<([u8; 32], String)> =
                match (f.get("validators"), f.get("validator-key")) {
                    (Some(list), None) => list
                        .split(',')
                        .map(|item| {
                            let (key, url) =
                                item.trim().split_once('@').unwrap_or((item.trim(), ""));
                            let key: [u8; 32] = hex::decode(key)
                                .ok()
                                .and_then(|k| k.try_into().ok())
                                .unwrap_or_else(|| {
                                    fail(format!("{key} is not a 32-byte hex public key"))
                                });
                            (key, url.trim_end_matches('/').to_owned())
                        })
                        .collect(),
                    (None, Some(path)) => {
                        let key = if Path::new(path).exists() {
                            read_validator_key(Path::new(path))
                        } else {
                            create_validator_key(Path::new(path))
                        }
                        .unwrap_or_else(|e| fail(e));
                        vec![(key.verifying_key().to_bytes(), String::new())]
                    }
                    _ => usage(),
                };
            let mut params = Params::test(
                f.get("supply")
                    .map_or(1_000_000, |v| v.parse().unwrap_or_else(|_| usage())),
            );
            orvexa_shielded::keys::os_rng().fill_bytes(&mut params.salt);
            params
                .set_validators(validators.iter().map(|(k, _)| *k).collect())
                .unwrap_or_else(|e| fail(e));
            let genesis = build_genesis(&params, &[(to, params.supply)]);
            let chain_id = Envelope::decode(&genesis).expect("own genesis").chain_id;
            let nodes = f
                .get("nodes")
                .map(|v| {
                    v.split(',')
                        .map(|u| u.trim().trim_end_matches('/').to_owned())
                        .collect()
                })
                .unwrap_or_default();
            let mut profile = NetworkProfile::new(
                f.get("name").map_or("ORVEXA v2 test", String::as_str),
                &params,
                &genesis,
                chain_id,
                nodes,
            );
            for (key, url) in &validators {
                profile.set_validator_url(key, url);
            }
            profile.validate().unwrap_or_else(|e| fail(e));
            let out = get("out");
            if Path::new(&out).exists() {
                fail(format!("{out} already exists"));
            }
            std::fs::write(&out, profile.to_json()).unwrap_or_else(|e| fail(e));
            println!(
                "network {} written to {out}; chain {}; {} validator(s), quorum {}",
                profile.name,
                profile.chain_id,
                params.validators.len(),
                params.quorum()
            );
        }
        "run" => {
            let text = std::fs::read_to_string(get("network")).unwrap_or_else(|e| fail(e));
            let profile = NetworkProfile::parse(&text).unwrap_or_else(|e| fail(e));
            let ms = |name: &str, default: u64| {
                f.get(name)
                    .map_or(default, |v| v.parse().unwrap_or_else(|_| usage()))
            };
            let role = match f.get("validator-key") {
                Some(path) => {
                    let key = read_validator_key(Path::new(path)).unwrap_or_else(|e| fail(e));
                    let public = key.verifying_key().to_bytes();
                    if !profile.is_validator(&public) {
                        fail("this key is not one of the network's validators");
                    }
                    let mut timeouts = default_timeouts();
                    if let Some(base) = f.get("timeouts-ms") {
                        let base: u64 = base.parse().unwrap_or_else(|_| usage());
                        timeouts.propose = Duration::from_millis(base);
                        timeouts.propose_delta = Duration::from_millis(base / 3);
                        timeouts.prevote = Duration::from_millis(base / 3);
                        timeouts.prevote_delta = Duration::from_millis(base / 6);
                        timeouts.precommit = Duration::from_millis(base / 3);
                        timeouts.precommit_delta = Duration::from_millis(base / 6);
                        timeouts.rebroadcast = Duration::from_millis(base * 2);
                    }
                    Role::Validator {
                        engine: EngineConfig {
                            peers: profile.peers_of(&public),
                            block_every: Duration::from_millis(ms("block-every-ms", 1000)),
                            timeouts,
                        },
                        key: ValidatorKey(Arc::new(key)),
                    }
                }
                None => Role::Follower {
                    upstream: f
                        .get("follow")
                        .cloned()
                        .or_else(|| profile.public_nodes().first().cloned())
                        .unwrap_or_else(|| fail("no node to follow")),
                    poll: Duration::from_millis(ms("poll-ms", 1000)),
                },
            };
            let config = NodeConfig {
                data_dir: get("data-dir").into(),
                genesis: Some(profile.genesis().unwrap_or_else(|e| fail(e))),
                params: profile.params().unwrap_or_else(|e| fail(e)),
                listen: f
                    .get("listen")
                    .map_or("127.0.0.1:4260", String::as_str)
                    .parse()
                    .unwrap_or_else(|_| usage()),
                role,
            };
            if let Err(e) = run(config).await {
                fail(format!("node stopped: {e}"));
            }
        }
        _ => usage(),
    }
}
