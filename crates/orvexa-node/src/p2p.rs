//! Native, opt-in peer transport. It never changes v1 validity or fork rules.
use crate::{MAX_MEMPOOL, SharedNode, locked, mesh::check_overlap};
use anyhow::{Context, Result, ensure};
use futures_util::StreamExt;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, Swarm, SwarmBuilder, connection_limits, identify, identity,
    multiaddr::Protocol,
    noise,
    request_response::{self, OutboundRequestId, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use orvexa_core::{Block, BlockHeader, Genesis, SignedTransaction, VERSION};
use orvexa_crypto::{Hash32, SecretKey, verify};
use orvexa_rpc::{BLOCK_PAGE, MAX_BODY, PeerStatus, RuntimeStatus, Status};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    net::IpAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_KNOWN: usize = 256;
const MAX_CONNECTED: usize = 128;
const ADDRESS_PAGE: usize = 32;

/// Separate transport identity; never derive it from a spending/authority key.
pub fn load_identity(path: &Path) -> Result<identity::Keypair> {
    if path.try_exists()? {
        ensure!(
            std::fs::metadata(path)?.len() <= 4096,
            "identity file too large"
        );
        let bytes = Zeroizing::new(std::fs::read(path)?);
        return identity::Keypair::from_protobuf_encoding(&bytes).context("invalid peer identity");
    }
    let key = identity::Keypair::generate_ed25519();
    let bytes = Zeroizing::new(key.to_protobuf_encoding()?);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .context("create separate peer identity")?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(key)
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let [x, y, _, _] = a.octets();
            !a.is_private()
                && !a.is_loopback()
                && !a.is_link_local()
                && !a.is_broadcast()
                && !a.is_documentation()
                && !a.is_unspecified()
                && !a.is_multicast()
                && x != 0
                && x < 240
                && !(x == 100 && (64..=127).contains(&y))
                && !(x == 198 && (y == 18 || y == 19))
        }
        IpAddr::V6(a) => {
            // Only global unicast; reject mapped IPv4 and special-purpose ranges.
            let s = a.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && s[1] <= 0x1ff)
                && !(s[0] == 0x2001 && s[1] == 0xdb8)
        }
    }
}

/// Strict numeric TCP multiaddr + authenticated PeerId. No DNS/URL/relay paths.
pub fn peer_address(value: &str, allow_private: bool) -> Result<(PeerId, Multiaddr)> {
    ensure!(value.len() <= 256, "peer address too long");
    let address: Multiaddr = value.parse().context("invalid peer multiaddress")?;
    let mut parts = address.iter();
    let ip = match parts.next() {
        Some(Protocol::Ip4(a)) => IpAddr::V4(a),
        Some(Protocol::Ip6(a)) => IpAddr::V6(a),
        _ => anyhow::bail!("numeric IP required"),
    };
    ensure!(
        !ip.is_unspecified() && !ip.is_multicast(),
        "invalid peer IP"
    );
    ensure!(
        public_ip(ip)
            || (allow_private
                && (ip.is_loopback()
                    || match ip {
                        IpAddr::V4(a) => a.is_private(),
                        IpAddr::V6(a) => a.is_unique_local(),
                    })),
        "peer requires a global IP (or explicit --allow-private-peers for LAN/tests)"
    );
    ensure!(
        matches!(parts.next(), Some(Protocol::Tcp(p)) if p != 0),
        "nonzero TCP port required"
    );
    let Some(Protocol::P2p(peer)) = parts.next() else {
        anyhow::bail!("peer ID required")
    };
    ensure!(parts.next().is_none(), "unexpected peer address suffix");
    Ok((peer, address))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    chain_id: Hash32,
    from: u64,
    anchor: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    genesis: Genesis,
    status: Status,
    anchor: Option<Block>,
    blocks: Vec<Block>,
    head: Option<SignedHead>,
    pending: Vec<SignedTransaction>,
    peers: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SignedHead {
    header: BlockHeader,
    signature: String,
}
#[derive(NetworkBehaviour)]
struct Behaviour {
    exchange: request_response::json::Behaviour<Request, Response>,
    identify: identify::Behaviour,
    limits: connection_limits::Behaviour,
}

pub struct Config {
    pub identity: identity::Keypair,
    pub listen: Multiaddr,
    pub seeds: Vec<String>,
    pub announce: Vec<String>,
    pub allow_private: bool,
    pub expected_chain: Hash32,
    pub evidence_dir: PathBuf,
    pub authority: Option<SecretKey>,
    pub block_every: Duration,
    pub sync_every: Duration,
}
pub struct Network {
    node: SharedNode,
    swarm: Swarm<Behaviour>,
    config: Config,
    evidence: PathBuf,
    known: HashMap<PeerId, Multiaddr>,
    connected: HashSet<PeerId>,
    pending: HashMap<OutboundRequestId, (PeerId, Request)>,
    heights: HashMap<PeerId, u64>,
    last_poll: HashMap<PeerId, Instant>,
    last_dial: HashMap<PeerId, Instant>,
    quota: (Instant, u32),
    peer_quota: HashMap<PeerId, (Instant, u32)>,
    last_produced: Instant,
}
impl Network {
    pub fn new(node: SharedNode, config: Config) -> Result<Self> {
        ensure!(
            config.seeds.len() <= 32 && config.announce.len() <= 8,
            "too many configured addresses"
        );
        ensure!(
            (1..=300).contains(&config.block_every.as_secs()),
            "invalid block interval"
        );
        ensure!(
            (1..=60).contains(&config.sync_every.as_secs()),
            "invalid sync interval"
        );
        let mut listen = config.listen.iter();
        ensure!(
            matches!(listen.next(), Some(Protocol::Ip4(_) | Protocol::Ip6(_)))
                && matches!(listen.next(), Some(Protocol::Tcp(_)))
                && listen.next().is_none(),
            "listener must be numeric IP/TCP"
        );
        let peer_id = config.identity.public().to_peer_id();
        std::fs::create_dir_all(&config.evidence_dir)?;
        let evidence = config.evidence_dir.join("authority-conflict.json");
        {
            let mut n = locked(&node)?;
            ensure!(
                n.ledger.genesis.chain_id() == config.expected_chain,
                "chain pin mismatch"
            );
            if let Some(key) = &config.authority {
                ensure!(
                    key.address() == n.ledger.genesis.authority,
                    "wrong authority wallet"
                );
                ensure!(
                    key.address() != n.ledger.genesis.recipient,
                    "use a separate unfunded authority key"
                );
            }
            n.runtime = RuntimeStatus {
                enabled: true,
                transport: "libp2p-tcp-noise".into(),
                peer_id: peer_id.to_string(),
                role: if config.authority.is_some() {
                    "authority"
                } else {
                    "replica"
                }
                .into(),
                halted: evidence.try_exists()?,
                ..Default::default()
            };
        }
        let mut swarm = SwarmBuilder::with_existing_identity(config.identity.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default().nodelay(true),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key| Behaviour {
                exchange: request_response::Behaviour::with_codec(
                    request_response::json::codec::Codec::default()
                        .set_request_size_maximum(4096)
                        .set_response_size_maximum(MAX_BODY as u64),
                    [(StreamProtocol::new("/orvexa/sync/1"), ProtocolSupport::Full)],
                    request_response::Config::default()
                        .with_request_timeout(Duration::from_secs(10))
                        .with_max_concurrent_streams(256),
                ),
                identify: identify::Behaviour::new(
                    identify::Config::new("/orvexa/1".into(), key.public())
                        .with_agent_version("orvexa/0.1".into()),
                ),
                limits: connection_limits::Behaviour::new(
                    connection_limits::ConnectionLimits::default()
                        .with_max_pending_incoming(Some(32))
                        .with_max_pending_outgoing(Some(8))
                        .with_max_established(Some(MAX_CONNECTED as u32))
                        .with_max_established_per_peer(Some(1)),
                ),
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(90)))
            .build();
        swarm.listen_on(config.listen.clone())?;
        let mut known = HashMap::new();
        for seed in &config.seeds {
            let (peer, address) = peer_address(seed, config.allow_private)?;
            if peer != peer_id {
                known.insert(peer, address);
            }
        }
        for address in &config.announce {
            let (peer, address) = peer_address(address, config.allow_private)?;
            ensure!(
                peer == peer_id,
                "announced address belongs to another PeerId"
            );
            swarm.add_external_address(address);
        }
        Ok(Self {
            node,
            swarm,
            config,
            evidence,
            known,
            connected: HashSet::new(),
            pending: HashMap::new(),
            heights: HashMap::new(),
            last_poll: HashMap::new(),
            last_dial: HashMap::new(),
            quota: (Instant::now(), 0),
            peer_quota: HashMap::new(),
            last_produced: Instant::now(),
        })
    }
    pub fn peer_id(&self) -> PeerId {
        *self.swarm.local_peer_id()
    }
    fn remember(&mut self, value: &str) {
        if let Ok((peer, address)) = peer_address(value, self.config.allow_private)
            && peer != self.peer_id()
            && (self.known.len() < MAX_KNOWN || self.known.contains_key(&peer))
        {
            self.known.entry(peer).or_insert(address);
        }
    }
    fn reply(&self, request: &Request) -> Result<Response> {
        ensure!(
            request.chain_id == self.config.expected_chain && request.from > 0,
            "foreign/invalid request"
        );
        let n = locked(&self.node)?;
        let anchor = request.anchor.min(n.ledger.height());
        let response = Response {
            genesis: n.ledger.genesis.clone(),
            status: n.status(),
            head: n.ledger.blocks().last().map(|b| SignedHead {
                header: b.header.clone(),
                signature: b.signature.clone(),
            }),
            anchor: if anchor == 0 {
                None
            } else {
                n.ledger.blocks().get((anchor - 1) as usize).cloned()
            },
            blocks: n
                .ledger
                .blocks()
                .iter()
                .filter(|b| b.header.height >= request.from)
                .take(BLOCK_PAGE)
                .cloned()
                .collect(),
            pending: n.mempool.clone(),
            peers: self
                .swarm
                .external_addresses()
                .map(ToString::to_string)
                .chain(self.known.values().map(ToString::to_string))
                .take(ADDRESS_PAGE)
                .collect(),
        };
        ensure!(
            serde_json::to_vec(&response)?.len() <= MAX_BODY,
            "response exceeds bound"
        );
        Ok(response)
    }
    fn accept(&mut self, peer: PeerId, request: Request, response: Response) -> Result<()> {
        let height = { locked(&self.node)?.ledger.height() };
        ensure!(
            response.genesis == locked(&self.node)?.ledger.genesis
                && response.status.chain_id == self.config.expected_chain
                && response.status.protocol == VERSION,
            "foreign genesis/status"
        );
        ensure!(
            response.blocks.len() <= BLOCK_PAGE
                && response.pending.len() <= MAX_MEMPOOL
                && response.peers.len() <= ADDRESS_PAGE,
            "oversize response"
        );
        if response.status.height == 0 {
            ensure!(response.head.is_none(), "unexpected genesis head");
        } else {
            let head = response.head.as_ref().context("missing signed head")?;
            ensure!(
                head.header.chain_id == self.config.expected_chain
                    && head.header.version == VERSION
                    && head.header.height == response.status.height,
                "invalid head claim"
            );
            verify(
                response.genesis.authority,
                &head.header.signing_bytes(),
                &head.signature,
            )?;
            let block = Block {
                header: head.header.clone(),
                signature: head.signature.clone(),
                transactions: vec![],
            };
            ensure!(block.id()? == response.status.tip, "head/tip mismatch");
        }
        let common = request.anchor.min(response.status.height);
        if common == 0 {
            ensure!(response.anchor.is_none(), "unexpected anchor");
            if response.status.height == 0 {
                ensure!(
                    response.status.tip == self.config.expected_chain,
                    "invalid genesis tip"
                );
            }
        } else {
            let anchor = response.anchor.as_ref().context("missing anchor")?;
            ensure!(
                anchor.header.height == common && common <= height,
                "invalid anchor height"
            );
            check_overlap(&self.node, anchor, &self.evidence)?;
            if common == response.status.height {
                ensure!(anchor.id()? == response.status.tip, "invalid tip");
            }
        }
        // Only an authority-signed head can advertise a higher height. A plain
        // unauthenticated height must never stall block production.
        ensure!(
            self.heights.len() < MAX_KNOWN || self.heights.contains_key(&peer),
            "peer observation limit reached"
        );
        self.heights
            .entry(peer)
            .and_modify(|h| *h = (*h).max(response.status.height))
            .or_insert(response.status.height);
        ensure!(
            response.status.height < request.from || !response.blocks.is_empty(),
            "missing block page"
        );
        for (offset, block) in response.blocks.into_iter().enumerate() {
            ensure!(
                block.header.height
                    == request
                        .from
                        .checked_add(offset as u64)
                        .context("height overflow")?
                    && block.header.height <= response.status.height,
                "invalid page sequence"
            );
            if block.header.height <= locked(&self.node)?.ledger.height() {
                check_overlap(&self.node, &block, &self.evidence)?;
            } else {
                locked(&self.node)?.commit(block)?;
            }
        }
        if locked(&self.node)?.ledger.height() >= response.status.height {
            for tx in response.pending {
                let mut n = locked(&self.node)?;
                if !n.mempool.contains(&tx) {
                    let _ = n.submit(tx);
                }
            }
        }
        for address in response.peers {
            self.remember(&address);
        }
        Ok(())
    }
    fn tick(&mut self) -> Result<()> {
        let local_height = locked(&self.node)?.ledger.height();
        // Reconnecting/new participants must not consume an observation slot
        // forever. Preserve only connected peers or signed heads still ahead.
        self.heights
            .retain(|peer, height| *height > local_height || self.connected.contains(peer));
        let halted = locked(&self.node)?.runtime.halted;
        if halted {
            ensure!(
                self.evidence.try_exists()?,
                "conflict evidence persistence failed; stop node"
            );
        }
        if !halted {
            let mut peers: Vec<_> = self.connected.iter().copied().collect();
            peers.sort_by_key(|p| self.last_poll.get(p).copied());
            for peer in peers.into_iter().take(8) {
                if self.pending.values().any(|(p, _)| *p == peer) {
                    continue;
                }
                let n = locked(&self.node)?;
                let request = Request {
                    chain_id: self.config.expected_chain,
                    from: n
                        .ledger
                        .height()
                        .checked_add(1)
                        .context("height overflow")?,
                    anchor: n.ledger.height(),
                };
                drop(n);
                let id = self
                    .swarm
                    .behaviour_mut()
                    .exchange
                    .send_request(&peer, request.clone());
                self.pending.insert(id, (peer, request));
                self.last_poll.insert(peer, Instant::now());
            }
            if self.last_produced.elapsed() >= self.config.block_every {
                if let Some(key) = &self.config.authority {
                    let mut n = locked(&self.node)?;
                    if !n.mempool.is_empty()
                        && !self.heights.values().any(|h| *h > n.ledger.height())
                    {
                        let block =
                            orvexa_wallet::sign_block(key, &n.ledger.genesis, n.template()?)?;
                        n.commit(block)?;
                        n.runtime.produced_blocks = n.runtime.produced_blocks.saturating_add(1);
                    }
                }
                self.last_produced = Instant::now();
            }
        }
        if self.connected.len() < 8 {
            let mut candidates: Vec<_> = self
                .known
                .iter()
                .filter(|(p, _)| !self.connected.contains(p))
                .collect();
            candidates.sort_by_key(|(p, _)| self.last_dial.get(p).copied());
            for (peer, address) in candidates.into_iter().take(4) {
                let _ = self.swarm.dial(address.clone());
                self.last_dial.insert(*peer, Instant::now());
            }
        }
        let mut n = locked(&self.node)?;
        n.runtime.peers = self
            .connected
            .iter()
            .map(|p| PeerStatus {
                url: p.to_string(),
                state: "connected".into(),
                height: self.heights.get(p).copied(),
            })
            .collect();
        Ok(())
    }
    pub async fn run(mut self) -> Result<()> {
        let mut clock = tokio::time::interval(self.config.sync_every);
        clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _=clock.tick()=>self.tick()?,
                event=self.swarm.select_next_some()=>match event {
                    SwarmEvent::NewListenAddr {address,..}=> {
                        let address=address.with(Protocol::P2p(self.peer_id())).to_string();
                        eprintln!("ORVEXA P2P listening: {address}");
                        let mut n=locked(&self.node)?;
                        if n.runtime.listen_addresses.len()<32 { n.runtime.listen_addresses.push(address); }
                    },
                    SwarmEvent::ListenerClosed {..}|SwarmEvent::ListenerError {..}=>anyhow::bail!("P2P listener failed"),
                    SwarmEvent::ConnectionEstablished {peer_id,..}=> {self.connected.insert(peer_id);},
                    SwarmEvent::ConnectionClosed {peer_id,num_established:0,..}=> {
                        self.connected.remove(&peer_id); self.peer_quota.remove(&peer_id); self.last_poll.remove(&peer_id);
                    },
                    SwarmEvent::Behaviour(BehaviourEvent::Identify(identify::Event::Received {peer_id,info,..}))=> {
                        for mut address in info.listen_addrs.into_iter().take(8) {
                            if !matches!(address.iter().last(),Some(Protocol::P2p(_))) {address.push(Protocol::P2p(peer_id));}
                            if peer_address(&address.to_string(),self.config.allow_private).is_ok_and(|(p,_)| p==peer_id) {self.remember(&address.to_string());}
                        }
                    },
                    SwarmEvent::Behaviour(BehaviourEvent::Exchange(request_response::Event::Message {peer,message,..}))=>match message {
                        request_response::Message::Request {request,channel,..}=> {
                            if self.quota.0.elapsed()>=Duration::from_secs(1) {self.quota=(Instant::now(),0);}
                            let quota=self.peer_quota.entry(peer).or_insert((Instant::now(),0));
                            if quota.0.elapsed()>=Duration::from_secs(1) {*quota=(Instant::now(),0);}
                            if self.quota.1<256 && quota.1<4 {
                                self.quota.1+=1;quota.1+=1;
                                if let Ok(response)=self.reply(&request) {let _=self.swarm.behaviour_mut().exchange.send_response(channel,response);}
                            }
                        },
                        request_response::Message::Response {request_id,response}=> {
                            if let Some((expected,request))=self.pending.remove(&request_id) && expected==peer
                                && let Err(error)=self.accept(peer,request,response) {
                                    eprintln!("ORVEXA rejected peer {peer}: {error}");
                                    let _=self.swarm.disconnect_peer_id(peer);
                            }
                        },
                    },
                    SwarmEvent::Behaviour(BehaviourEvent::Exchange(request_response::Event::OutboundFailure {request_id,..}))=>{self.pending.remove(&request_id);},
                    _=>{},
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orvexa_core::Transaction;
    fn key(n: u8) -> SecretKey {
        SecretKey::from_bytes(&[n; 32])
    }
    fn network(path: &Path) -> Network {
        let genesis = Genesis::new(key(1).address(), key(2).address(), Hash32([49; 32]));
        let node = crate::Node::create(&path.join("node.sqlite"), genesis.clone())
            .unwrap()
            .shared();
        Network::new(
            node,
            Config {
                identity: identity::Keypair::generate_ed25519(),
                listen: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
                seeds: vec![],
                announce: vec![],
                allow_private: true,
                expected_chain: genesis.chain_id(),
                evidence_dir: path.join("evidence"),
                authority: Some(key(1)),
                block_every: Duration::from_secs(1),
                sync_every: Duration::from_secs(1),
            },
        )
        .unwrap()
    }
    #[tokio::test]
    async fn disconnected_peer_churn_releases_bounded_observation_slots() {
        let dir = tempfile::tempdir().unwrap();
        let mut network = network(dir.path());
        let request = Request {
            chain_id: network.config.expected_chain,
            from: 1,
            anchor: 0,
        };
        let response = network.reply(&request).unwrap();
        for _ in 0..(MAX_KNOWN + 10) {
            let peer = identity::Keypair::generate_ed25519().public().to_peer_id();
            network
                .accept(peer, request.clone(), response.clone())
                .unwrap();
            network.tick().unwrap();
        }
        assert!(network.heights.is_empty());
    }
    #[tokio::test]
    async fn rejects_foreign_forged_oversized_and_unsigned_height_claims() {
        let dir = tempfile::tempdir().unwrap();
        let mut network = network(dir.path());
        let request = Request {
            chain_id: network.config.expected_chain,
            from: 1,
            anchor: 0,
        };
        let peer = identity::Keypair::generate_ed25519().public().to_peer_id();
        let response = network.reply(&request).unwrap();
        let mut bad = response.clone();
        bad.status.chain_id = Hash32([0; 32]);
        assert!(network.accept(peer, request.clone(), bad).is_err());
        let mut bad = response.clone();
        bad.status.height = u64::MAX;
        assert!(network.accept(peer, request.clone(), bad).is_err());
        assert!(network.heights.is_empty());
        let mut bad = response.clone();
        bad.peers = vec!["invalid".into(); ADDRESS_PAGE + 1];
        assert!(network.accept(peer, request.clone(), bad).is_err());
        let mut block = locked(&network.node)
            .unwrap()
            .template()
            .unwrap()
            .sign(&key(4));
        let mut bad = response.clone();
        bad.status.height = 1;
        bad.status.tip = block.id().unwrap();
        bad.head = Some(SignedHead {
            header: block.header.clone(),
            signature: block.signature.clone(),
        });
        bad.blocks = vec![block.clone()];
        assert!(network.accept(peer, request.clone(), bad).is_err());
        // Valid authority signature still cannot skip parent validation.
        block.header.parent = Hash32([0; 32]);
        block = block.sign(&key(1));
        let mut bad = response;
        bad.status.height = 1;
        bad.status.tip = block.id().unwrap();
        bad.head = Some(SignedHead {
            header: block.header.clone(),
            signature: block.signature.clone(),
        });
        bad.blocks = vec![block];
        assert!(network.accept(peer, request, bad).is_err());
        assert_eq!(locked(&network.node).unwrap().ledger.height(), 0);
        assert!(!locked(&network.node).unwrap().runtime.halted);
    }
    #[tokio::test]
    async fn invalid_pending_cannot_mint_or_double_spend_and_conflicts_latch() {
        let dir = tempfile::tempdir().unwrap();
        let mut network = network(dir.path());
        let peer = identity::Keypair::generate_ed25519().public().to_peer_id();
        let request = Request {
            chain_id: network.config.expected_chain,
            from: 1,
            anchor: 0,
        };
        let tx = Transaction {
            version: VERSION,
            chain_id: request.chain_id,
            sender: key(2).address(),
            recipient: key(3).address(),
            amount: 10,
            fee: 1,
            nonce: 0,
        }
        .sign(&key(2))
        .unwrap();
        let mut response = network.reply(&request).unwrap();
        let mut forged = tx.clone();
        forged.transaction.amount = u64::MAX;
        response.pending = vec![forged, tx.clone(), tx.clone()];
        network.accept(peer, request.clone(), response).unwrap();
        let mut n = locked(&network.node).unwrap();
        assert_eq!(n.mempool.len(), 1);
        let block = n.template().unwrap().sign(&key(1));
        n.commit(block).unwrap();
        drop(n);
        let request = Request {
            from: 2,
            anchor: 1,
            ..request
        };
        let mut response = network.reply(&request).unwrap();
        let mut conflicting = response.anchor.clone().unwrap();
        conflicting.header.timestamp += 1;
        conflicting = conflicting.sign(&key(4));
        response.anchor = Some(conflicting.clone());
        assert!(
            network
                .accept(peer, request.clone(), response.clone())
                .is_err()
        );
        assert!(!locked(&network.node).unwrap().runtime.halted);
        response.anchor = Some(conflicting.sign(&key(1)));
        assert!(network.accept(peer, request, response).is_err());
        assert!(locked(&network.node).unwrap().runtime.halted);
        assert!(network.evidence.exists());
        assert!(locked(&network.node).unwrap().submit(tx).is_err());
        let db = dir.path().join("node.sqlite");
        drop(network);
        let node = crate::Node::open(&db).unwrap();
        assert_eq!(node.ledger.height(), 1);
        assert_eq!(node.ledger.account(key(3).address()).balance, 10);
        let config = Config {
            identity: identity::Keypair::generate_ed25519(),
            listen: "/ip4/127.0.0.1/tcp/0".parse().unwrap(),
            seeds: vec![],
            announce: vec![],
            allow_private: true,
            expected_chain: node.ledger.genesis.chain_id(),
            evidence_dir: dir.path().join("evidence"),
            authority: None,
            block_every: Duration::from_secs(1),
            sync_every: Duration::from_secs(1),
        };
        let restored = Network::new(node.shared(), config).unwrap();
        assert!(locked(&restored.node).unwrap().runtime.halted);
    }
}
