//! orvexa-v2-node — local private v2 test node (prototype).
//!
//!   orvexa-v2-node genesis --to <address-hex> --out genesis.bin [--salt HEX] [--supply N]
//!   orvexa-v2-node run --data-dir DIR [--genesis genesis.bin] [--salt HEX] [--supply N]
//!                      [--listen 127.0.0.1:4260] (--produce-every-ms N | --follow URL [--poll-ms N])
//!
//! The address is the 43-byte raw Orchard address in hex. Test network only.

use orvexa_shielded::{
    ledger::Params,
    node::{run, NodeConfig, Role},
    wallet::build_genesis,
};
use std::collections::HashMap;
use std::time::Duration;

fn usage() -> ! {
    eprintln!("usage: orvexa-v2-node genesis --to <address-hex> --out FILE [--salt HEX] [--supply N]");
    eprintln!("       orvexa-v2-node run --data-dir DIR [--genesis FILE] [--listen ADDR] (--produce-every-ms N | --follow URL [--poll-ms N])");
    std::process::exit(2)
}

fn flags(args: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut it = args.iter();
    while let Some(key) = it.next() {
        let Some(name) = key.strip_prefix("--") else { usage() };
        let Some(value) = it.next() else { usage() };
        map.insert(name.to_owned(), value.clone());
    }
    map
}

fn params(f: &HashMap<String, String>) -> Params {
    let mut p = Params::test(1_000_000);
    if let Some(salt) = f.get("salt") {
        p.salt = hex::decode(salt).ok().and_then(|v| v.try_into().ok()).unwrap_or_else(|| usage());
    }
    if let Some(supply) = f.get("supply") {
        p.supply = supply.parse().unwrap_or_else(|_| usage());
    }
    p
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else { usage() };
    let f = flags(&args[1..]);
    match command.as_str() {
        "genesis" => {
            let to: [u8; 43] = f.get("to").and_then(|h| hex::decode(h).ok()).and_then(|v| v.try_into().ok()).unwrap_or_else(|| usage());
            let address = Option::from(orchard::Address::from_raw_address_bytes(&to)).unwrap_or_else(|| usage());
            let p = params(&f);
            let genesis = build_genesis(&p, &[(address, p.supply)]);
            std::fs::write(f.get("out").unwrap_or_else(|| usage()), &genesis).expect("write genesis");
            println!("genesis written, {} bytes", genesis.len());
        }
        "run" => {
            let role = match (f.get("produce-every-ms"), f.get("follow")) {
                (Some(ms), None) => Role::Producer { every: Duration::from_millis(ms.parse().unwrap_or_else(|_| usage())) },
                (None, Some(url)) => Role::Follower {
                    upstream: url.trim_end_matches('/').to_owned(),
                    poll: Duration::from_millis(f.get("poll-ms").map_or(500, |v| v.parse().unwrap_or_else(|_| usage()))),
                },
                _ => usage(),
            };
            let config = NodeConfig {
                data_dir: f.get("data-dir").unwrap_or_else(|| usage()).into(),
                genesis: f.get("genesis").map(|path| std::fs::read(path).expect("read genesis")),
                params: params(&f),
                listen: f.get("listen").map_or("127.0.0.1:4260", String::as_str).parse().unwrap_or_else(|_| usage()),
                role,
            };
            if let Err(e) = run(config).await {
                eprintln!("ORVEXA v2 node stopped: {e}");
                std::process::exit(1);
            }
        }
        _ => usage(),
    }
}
