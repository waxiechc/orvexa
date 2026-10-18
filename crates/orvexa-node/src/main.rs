use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use orvexa_core::Genesis;
use orvexa_crypto::Hash32;
use orvexa_node::{Node, mesh, p2p};
use std::{io::Read, net::SocketAddr, path::PathBuf, time::Duration};
use zeroize::Zeroizing;

#[derive(Parser)]
#[command(
    about = "ORVEXA transparent v1 testnet node — one authority, test atoms only",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Validate a numeric native seed without dialing it.
    PeerAddress {
        #[arg(long)]
        address: String,
        #[arg(long)]
        allow_private_peers: bool,
    },
    /// Create/read a separate transport identity and print its public PeerId.
    PeerId {
        #[arg(long)]
        identity: PathBuf,
    },
    /// Create a shared peer-access credential, never a wallet or authority key.
    MeshToken {
        #[arg(long)]
        out: PathBuf,
    },
    Init {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        genesis: PathBuf,
    },
    Run(Box<RunArgs>),
}
#[derive(clap::Args)]
struct RunArgs {
    #[arg(long)]
    db: PathBuf,
    #[arg(long, default_value = "127.0.0.1:4242")]
    listen: SocketAddr,
    /// Opt in to the same-origin browser Wallet and Explorer (built web/dist).
    #[arg(long)]
    web_dir: Option<PathBuf>,
    /// Explicit opt-in: bind read-only peer RPC to this Tailscale IP/port.
    #[arg(long, conflicts_with = "p2p_listen")]
    mesh_listen: Option<SocketAddr>,
    /// Native TCP/Noise listener, e.g. /ip4/0.0.0.0/tcp/4243.
    #[arg(long)]
    p2p_listen: Option<libp2p::Multiaddr>,
    #[arg(long, requires = "p2p_listen")]
    p2p_identity: Option<PathBuf>,
    /// Repeat numeric /ip4/.../tcp/.../p2p/PeerId seeds on the common chain.
    #[arg(long, requires = "p2p_listen")]
    p2p_seed: Vec<String>,
    #[arg(long, requires = "p2p_listen")]
    p2p_announce: Vec<String>,
    #[arg(long, requires = "p2p_listen")]
    allow_private_peers: bool,
    #[arg(long, requires = "mesh_listen")]
    mesh_token_file: Option<PathBuf>,
    /// Numeric Tailscale peer origin. Repeat for each participant.
    #[arg(long, requires = "mesh_listen")]
    peer: Vec<String>,
    #[arg(long)]
    expect_chain: Option<Hash32>,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u64).range(1..=60))]
    sync_every: u64,
    /// Load only the separate authority wallet. Omit on all replicas.
    #[arg(long)]
    authority_wallet: Option<PathBuf>,
    #[arg(long, requires = "authority_wallet")]
    password_stdin: bool,
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..=300))]
    block_every: u64,
    #[arg(long)]
    evidence_dir: Option<PathBuf>,
}
#[tokio::main]
async fn main() -> Result<()> {
    match Args::parse().command {
        Command::PeerAddress {
            address,
            allow_private_peers,
        } => {
            let (_, address) = p2p::peer_address(&address, allow_private_peers)?;
            println!("{address}");
        }
        Command::PeerId { identity } => {
            println!("{}", p2p::load_identity(&identity)?.public().to_peer_id());
        }
        Command::MeshToken { out } => {
            mesh::create_token(&out)?;
            println!(
                "Mesh credential created. Share the file only with invited node operators; keep it out of logs and URLs."
            );
        }
        Command::Init { db, genesis } => {
            ensure!(
                std::fs::metadata(&genesis)?.len() <= 8192,
                "genesis file too large"
            );
            let genesis: Genesis = serde_json::from_slice(&std::fs::read(genesis)?)?;
            let node = Node::create(&db, genesis)?;
            println!("{}", serde_json::to_string_pretty(&node.status())?);
        }
        Command::Run(arguments) => {
            let RunArgs {
                db,
                listen,
                web_dir,
                mesh_listen,
                p2p_listen,
                p2p_identity,
                p2p_seed,
                p2p_announce,
                allow_private_peers,
                mesh_token_file,
                peer,
                expect_chain,
                sync_every,
                authority_wallet,
                password_stdin,
                block_every,
                evidence_dir,
            } = *arguments;
            ensure!(
                authority_wallet.is_none() || mesh_listen.is_some() || p2p_listen.is_some(),
                "automatic authority requires a network mode"
            );
            ensure!(
                listen.ip().is_loopback(),
                "only loopback listeners are allowed"
            );
            let node = Node::open(&db)?;
            if let Some(expected) = expect_chain {
                ensure!(
                    node.ledger.genesis.chain_id() == expected,
                    "database belongs to another network"
                );
            }
            let key = if let Some(path) = authority_wallet {
                let password = if password_stdin {
                    let mut value = Zeroizing::new(String::new());
                    std::io::stdin().take(4097).read_to_string(&mut value)?;
                    ensure!(value.len() <= 4096, "password input exceeds limit");
                    let length = value.trim_end_matches(['\r', '\n']).len();
                    value.truncate(length);
                    ensure!(
                        !value.contains(['\r', '\n']),
                        "password stdin must contain one line"
                    );
                    value
                } else {
                    Zeroizing::new(rpassword::prompt_password(
                        "Authority wallet password (local memory only): ",
                    )?)
                };
                Some(orvexa_wallet::unlock(&path, &password)?)
            } else {
                None
            };
            let evidence = evidence_dir.unwrap_or_else(|| db.with_extension("evidence"));
            let p2p_config = if let Some(address) = p2p_listen {
                Some(p2p::Config {
                    identity: p2p::load_identity(
                        p2p_identity
                            .as_deref()
                            .ok_or_else(|| anyhow::anyhow!("P2P requires --p2p-identity"))?,
                    )?,
                    listen: address,
                    seeds: p2p_seed,
                    announce: p2p_announce,
                    allow_private: allow_private_peers,
                    expected_chain: expect_chain
                        .ok_or_else(|| anyhow::anyhow!("P2P requires --expect-chain"))?,
                    evidence_dir: evidence.clone(),
                    authority: None,
                    block_every: Duration::from_secs(block_every),
                    sync_every: Duration::from_secs(sync_every),
                })
            } else {
                None
            };
            let mesh_config = if let Some(address) = mesh_listen {
                ensure!(
                    orvexa_rpc::mesh_address(address.ip()),
                    "mesh listener must be an explicit Tailscale or loopback IP; no wildcard/public/LAN listeners"
                );
                ensure!(
                    address.port() != 0,
                    "mesh listener requires an explicit nonzero port"
                );
                ensure!(
                    address != listen,
                    "UI and mesh listeners must use different addresses/ports"
                );
                let expected = expect_chain
                    .ok_or_else(|| anyhow::anyhow!("mesh mode requires --expect-chain"))?;
                let token = mesh::read_token(
                    mesh_token_file
                        .as_deref()
                        .ok_or_else(|| anyhow::anyhow!("mesh mode requires --mesh-token-file"))?,
                )?;
                ensure!(peer.len() <= mesh::MAX_PEERS, "at most eight peers");
                let peers = peer
                    .iter()
                    .map(|url| mesh::Peer::new(url, &token))
                    .collect::<Result<Vec<_>>>()?;
                Some((address, token, peers, expected, evidence))
            } else {
                None
            };
            let listener = tokio::net::TcpListener::bind(listen).await?;
            eprintln!(
                "ORVEXA TRANSPARENT DEVNET | EXPERIMENTAL SINGLE SEQUENCER | {} | height {}",
                listener.local_addr()?,
                node.ledger.height()
            );
            let origin = format!("http://{}", listener.local_addr()?);
            let shared = node.shared();
            let mut network = None;
            let mut mesh_server = None;
            let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
            if let Some(mut config) = p2p_config {
                config.authority = key;
                let worker = p2p::Network::new(shared.clone(), config)?;
                eprintln!("ORVEXA native P2P PeerId: {}", worker.peer_id());
                network = Some(tokio::spawn(worker.run()));
            } else if let Some((address, token, peers, expected, evidence)) = mesh_config {
                let worker = mesh::Network::new(
                    shared.clone(),
                    peers,
                    key,
                    expected,
                    &evidence,
                    Duration::from_secs(block_every),
                )?;
                let peer_listener = tokio::net::TcpListener::bind(address).await?;
                let peer_app = mesh::router(shared.clone(), token);
                let stopping = stop_rx.clone();
                mesh_server = Some(tokio::spawn(async move {
                    axum::serve(peer_listener, peer_app)
                        .with_graceful_shutdown(shutdown(stopping))
                        .await
                }));
                network = Some(tokio::spawn(worker.run(Duration::from_secs(sync_every))));
                eprintln!(
                    "ORVEXA friends testnet peer listener: {address}; encrypted VPN required; no public Internet listener"
                );
            }
            let app = if let Some(directory) = web_dir {
                eprintln!("Wallet & Explorer: {origin}");
                orvexa_node::web::router(shared, &directory, &origin)?
            } else {
                orvexa_node::router(shared)
            };
            let mut server = tokio::spawn(async move {
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown(stop_rx))
                    .await
            });
            let outcome: Result<()> = if let (Some(worker), Some(peer_server)) =
                (&mut network, &mut mesh_server)
            {
                tokio::select! {
                    result = &mut server => { result??; Ok(()) },
                    result = worker => { result??; anyhow::bail!("network worker stopped unexpectedly") },
                    result = peer_server => { result??; anyhow::bail!("peer listener stopped unexpectedly") },
                    result = tokio::signal::ctrl_c() => { result?; Ok(()) },
                }
            } else if let Some(worker) = &mut network {
                tokio::select! {
                    result = &mut server => { result??; Ok(()) },
                    result = worker => { result??; anyhow::bail!("network worker stopped unexpectedly") },
                    result = tokio::signal::ctrl_c() => { result?; Ok(()) },
                }
            } else {
                tokio::select! {
                    result = &mut server => { result??; Ok(()) },
                    result = tokio::signal::ctrl_c() => { result?; Ok(()) },
                }
            };
            let _ = stop_tx.send(true);
            if let Some(worker) = network {
                worker.abort();
                let _ = worker.await;
            }
            if !server.is_finished()
                && tokio::time::timeout(Duration::from_secs(2), &mut server)
                    .await
                    .is_err()
            {
                server.abort();
            }
            if let Some(mut peer_server) = mesh_server
                && !peer_server.is_finished()
                && tokio::time::timeout(Duration::from_secs(2), &mut peer_server)
                    .await
                    .is_err()
            {
                peer_server.abort();
            }
            outcome?;
        }
    }
    Ok(())
}

async fn shutdown(mut stopping: tokio::sync::watch::Receiver<bool>) {
    if !*stopping.borrow() {
        let _ = stopping.changed().await;
    }
}
