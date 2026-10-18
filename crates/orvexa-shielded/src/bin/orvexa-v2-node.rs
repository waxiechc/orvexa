//! orvexa-v2-node — private v2 test network node for operators.
//!
//!   orvexa-v2-node genesis --to <ovxt1… address> --producer-key producer.key --out network.json
//!                          [--name "ORVEXA v2 test"] [--nodes https://a.example,https://b.example] [--supply N]
//!   orvexa-v2-node run --data-dir DIR --network network.json [--listen 127.0.0.1:4260]
//!                      (--producer-key producer.key --produce-every-ms N | [--follow URL] [--poll-ms N])
//!
//! `genesis` creates the producer key (never overwritten) and the network
//! file to hand out: people import it in ORVEXA Desktop and join. Test only.

use orvexa_shielded::{
    address,
    ledger::Params,
    node::{run, NodeConfig, ProducerKey, Role},
    profile::{create_producer_key, read_producer_key, NetworkProfile},
    wallet::build_genesis,
    Envelope,
};
use rand::rand_core::Rng as _;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

fn usage() -> ! {
    eprintln!("usage: orvexa-v2-node genesis --to <ovxt1-address> --producer-key FILE --out network.json [--name NAME] [--nodes URL,URL] [--supply N]");
    eprintln!("       orvexa-v2-node run --data-dir DIR --network network.json [--listen ADDR] (--producer-key FILE --produce-every-ms N | [--follow URL] [--poll-ms N])");
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
        "genesis" => {
            let to = address::decode(&get("to"))
                .unwrap_or_else(|| fail("--to must be an ovxt1… address"));
            let key_path = get("producer-key");
            let key = if Path::new(&key_path).exists() {
                read_producer_key(Path::new(&key_path))
            } else {
                create_producer_key(Path::new(&key_path))
            }
            .unwrap_or_else(|e| fail(e));
            let mut params = Params::test(
                f.get("supply")
                    .map_or(1_000_000, |v| v.parse().unwrap_or_else(|_| usage())),
            );
            orvexa_shielded::keys::os_rng().fill_bytes(&mut params.salt);
            params.producer = key.verifying_key().to_bytes();
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
            let profile = NetworkProfile::new(
                f.get("name").map_or("ORVEXA v2 test", String::as_str),
                &params,
                &genesis,
                chain_id,
                nodes,
            );
            profile.validate().unwrap_or_else(|e| fail(e));
            let out = get("out");
            if Path::new(&out).exists() {
                fail(format!("{out} already exists"));
            }
            std::fs::write(&out, profile.to_json()).unwrap_or_else(|e| fail(e));
            println!(
                "network {} written to {out}; chain {}",
                profile.name, profile.chain_id
            );
        }
        "run" => {
            let text = std::fs::read_to_string(get("network")).unwrap_or_else(|e| fail(e));
            let profile = NetworkProfile::parse(&text).unwrap_or_else(|e| fail(e));
            let role = match (f.get("produce-every-ms"), f.get("producer-key")) {
                (Some(ms), Some(key)) => Role::Producer {
                    every: Duration::from_millis(ms.parse().unwrap_or_else(|_| usage())),
                    key: ProducerKey(Arc::new(
                        read_producer_key(Path::new(key)).unwrap_or_else(|e| fail(e)),
                    )),
                },
                (None, None) => Role::Follower {
                    upstream: f
                        .get("follow")
                        .cloned()
                        .or_else(|| profile.nodes.first().cloned())
                        .unwrap_or_else(|| fail("no node to follow")),
                    poll: Duration::from_millis(
                        f.get("poll-ms")
                            .map_or(1000, |v| v.parse().unwrap_or_else(|_| usage())),
                    ),
                },
                _ => usage(),
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
