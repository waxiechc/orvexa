# ORVEXA native P2P testnet — implementation boundary, 2026-10-18

The requested network is one common chain, joined by volunteer computers and
servers. No Tailscale, cloud account, subscription, shared access credential or
external relay service is required. Node software is free; operators provide
their own machines, power and connectivity. At least one reachable bootstrap
address is needed; two independent reachable servers are recommended. Machines
behind NAT can dial out, but being behind CGNAT does not magically create inbound
reachability. No router/firewall/account configuration will be changed silently.

This stage replaces the private-overlay requirement with an **optional native
peer transport**, preserving loopback wallet APIs, v1 crypto/consensus/accounting
and SQLite. It is not the proposed private ledger or a mainnet release. Existing
local and friends modes remain usable. No mining, rewards, stake or emissions
are added. Full nodes independently verify/replicate; exactly one configured
authority creates blocks until a separately designed consensus upgrade.

## Concrete transport and messages

Pinned Rust libp2p 0.56.0, TCP + Noise + Yamux, identify and bounded JSON
request/response. New 0.57.1 was evaluated but its swarm 0.48.0 pins
wasm-bindgen-futures 0.4.58, which conflicts with the working wallet's pinned
wasm-bindgen 0.2.129. Do not downgrade that wallet stack or patch cryptographic
sources to force resolution. The 0.56.0 graph resolves and stays independently
pinned in Cargo.lock. Its MIT licenses/notices must accompany binaries.
Upstream references: [builder](https://docs.rs/libp2p/0.56.0/libp2p/struct.SwarmBuilder.html),
[bounded JSON codec](https://docs.rs/libp2p/0.56.0/libp2p/request_response/json/codec/struct.Codec.html).
Using these libraries does not constitute an ORVEXA security audit.

Each node generates a separate persisted libp2p identity/PeerId. It is not a
spending or block-authority key. Seed addresses include the authenticated PeerId.
No DNS, external IP-discovery API, discovery service or default third-party
bootstrap list is used. Configured operators supply actual reachable numeric
addresses. Discovered addresses are bounded and validated; default Internet
mode does not dial loopback/private/link-local/metadata addresses. An explicit
private-address option supports LAN and local integration tests.

Peers exchange one chain-pinned request: requested next height and common-prefix
anchor height. Responses include actual genesis/status, the requested bounded
prefix anchor, an authority-signed tip header, at most eight signed blocks, at most 512 pending transfers, and
a small page of dialable peer addresses. Requests are capped at 4 KiB and
responses at 2 MiB. No balance mutation, wallet file, password, authority signing
or public web-wallet endpoint exists on the P2P listener. Each accepted block
still passes native v1 validation and durable commit before changing memory.

Anti-entropy polling propagates transactions and accepted blocks, backfills
offline nodes and retries configured seeds. Connection/stream/address/request
limits bound resources; these are prototype limits, not demonstrated public
Internet DoS protection. Wrong chains, forged blocks and double spends are
rejected. Conflicting authority signatures reuse the existing durable
quarantine/evidence behavior without selecting a branch. Only validated signed
data can change ledger state; remote height advertisements are not finality.
An unsigned/spoofed high status cannot stall the authority: the advertised tip
must match a verified authority-signed header before its height is remembered.

## Delivery and acceptance

1. Native transport, strict addresses, separate identity, pinned genesis,
   bounded sync/peer discovery and automatic authority operation.
2. Real TCP/Noise tests: transfer dissemination, offline/restart catch-up,
   foreign/invalid data rejection, default local compatibility and a local
   100-node exercise. A local exercise is not an Internet or capacity guarantee.
3. Windows launch file and Linux server commands using the **same supplied
   genesis/network manifest**. Joining must never silently create an independent
   chain. Code-only ZIPs contain no operator wallets/identity/authority keys.
4. Deployment acceptance with actual volunteer server addresses, NAT/firewall
   checks, independent operators and prolonged operation. No invented live seeds.

Multi-authority consensus, signer anti-equivocation hardening, reorg rules,
permissionless Sybil/eclipsing resistance, peer-store recovery, automatic NAT
hole punching/participant-operated relays and private transaction proofs remain
separate work. An offline sole authority still stops confirmations. A meaningful
100-person test requires actual participants and reachable volunteer infrastructure.
