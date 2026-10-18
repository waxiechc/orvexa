# ORVEXA: architecture decision record 001

Status: experimental transparent local devnet. PRIVATE BY DESIGN is a target,
not a description of this version. No real funds. No public deployment.

ORVEXA is a provisional working name pending trademark, domain and cryptocurrency
project checks. The workspace and binaries use `orvexa-*`. The rebrand preserves
all v1 wire/storage encodings, address strings, signing bytes and economics;
legacy `VOID/...` domains and `void_test_` addresses are protocol identifiers.
See [the rename inventory and compatibility checks](docs/RENAME_ORVEXA.md).

## Proposed privacy architecture (not activated)

Optional native P2P is implemented in `orvexa-node::p2p`: pinned libp2p 0.56.0,
TCP/Noise/Yamux, bounded request/response and identify. No hosted discovery,
VPN or shared token. Separate persistent PeerId, numeric pinned seeds,
authority-signed height claims, append-only verified sync and durable conflict
quarantine. Local wallet API remains loopback-only. See [protocol boundary](docs/P2P_DESIGN.md)
and [operator instructions](docs/NATIVE_P2P.md). Public deployment still requires
actual reachable volunteer nodes; sole authority consensus and public v1 ledger
are unchanged. No automatic promotion, mining or node rewards.

[PRIVACY_DESIGN.md](docs/PRIVACY_DESIGN.md), revision 2, compares Monero
RingCT/CLSAG with Orchard/Ironwood for a wallet, transfers and Explorer.
It recommends a separate shielded-note v2 test network, subject to review of the
exact upstream version, audit coverage and resource costs. Orchard 0.16.0 in
Ironwood-v3 mode is an evaluation candidate, not an accepted dependency.
P1 is now upstream evaluation in a separate workspace; the ORVEXA format/adapter,
state, wallet, network and Explorer follow through separate gates P2–P6.
The earlier proposal to start with an adapter in this workspace is superseded.

The existing account ledger, Ed25519 v1 encodings, fees/supply, database schema,
API and demonstration remain intact. A new ledger is necessary because every v1
balance is reconstructible from its public genesis and transaction history.
See [the test matrix](docs/PRIVACY_TEST_PLAN.md) for the distinction between
existing v1 coverage and planned private-protocol tests.

## Consensus choice (recorded before implementation)

| Candidate | Benefit | Cost / trust assumption |
| --- | --- | --- |
| Nakamoto PoW | Open validator participation; objective cumulative work | Energy, difficulty adjustment, fork choice and reorg implementation; a tiny network is easily attacked |
| PoS / BFT | Fast finality with a defined validator set | Stake bootstrap, validator membership, slashing, quorum and network assumptions; substantial attack surface |
| Signed single sequencer | Small deterministic validation surface; no mining hardware | One explicitly configured authority can censor, halt or equivocate; no decentralized consensus |

Decision: manual, Ed25519-signed single sequencer for **local testing only**.
Genesis fixes its public address. Every node independently checks all rules.
The authority cannot spend other wallets' coins or mint after genesis, but can
sign conflicting histories. Nodes only append to their current tip; divergent
forks are rejected, never silently replaced. There is no fork choice or Byzantine
finality. The word `produce` means signing a test block, not proof-of-work mining.
Sources: [Bitcoin paper](https://bitcoin.org/bitcoin.pdf),
[Tendermint consensus specification](https://github.com/tendermint/tendermint/blob/main/spec/consensus/consensus.md).

## Modules

- orvexa-crypto: SHA-256, strict Ed25519 verification, checksummed test addresses,
  OS randomness; no custom cryptographic algorithms.
- orvexa-core: consensus data, canonical signing bytes, account state transitions,
  nonce replay prevention and supply invariants. No I/O.
- orvexa-wallet: encrypted key files and local transaction/block signing.
- orvexa-rpc: typed loopback HTTP JSON API and bounded client.
- orvexa-node: SQLite block journal, complete replay validation, pending pool,
  HTTP server and explicit peer synchronization.
- orvexa-cli: wallet, genesis, transfer, production, history and synchronization.
- orvexa-tests: cryptographic, ledger, persistence and real HTTP integration tests.
- orvexa-web-wallet: browser Wasm adapter reusing native encrypted v1 wallets
  and canonical signing; no new primitives or private backend.
- web/: English React/TypeScript Wallet/Explorer, exact-integer v1 adapter,
  encrypted browser storage and short-lived signing workers.

The optional `--web-dir` host serves built assets on the numeric loopback origin
with exact Host/Origin checks and CSP. Default CLI mode still rejects browser
Origins. Only signed public payloads are posted; no password/key endpoints exist.
`/v1/ui-state` reads a coherent state under one lock. `start-local.ps1` manages
three local processes with pull replication. See [workflow](docs/WEB_WALLET.md).

## Temporary economics

All amounts are integer **test atoms**, with no exchange rate or promised value.
Genesis v1 allocates exactly 1,000,000 atoms to one explicit test recipient. No
subsidy, faucet, premine hidden in code, mutable supply configuration or admin
balance endpoint exists. Minimum fee is 1 atom; fees are burned, not paid to the
developer or authority. `sum(balances) + burned_fees == 1_000_000` at every commit.

Production alternatives remain undecided: capped issuance with a declining block
subsidy (eventual fee security), perpetual tail emission (persistent security
budget and dilution), or fully distributed genesis (allocation/bootstrap trust).
Economic modelling and consensus selection must precede a public specification.

## Wire format and state

JSON is transport/storage only. SHA-256 hashes and signatures use domain-separated
fixed-width bytes: version u32 LE; unsigned integers u64 LE; hashes/keys 32 raw
bytes. Genesis includes a random 32-byte network salt, authority, allocation
recipient and fixed supply. Its canonical digest is the chain ID and genesis tip.
Transactions bind version, chain ID, sender key, recipient key, amount, fee and
nonce; tx ID additionally binds the 64-byte signature. Block headers bind version,
chain ID, height, parent, strictly increasing logical timestamp and a SHA-256
commitment to the ordered transaction IDs and u64 count. Block IDs bind the signed
header plus authority signature. Signatures never depend on JSON key order.

Addresses are `void_test_` + lowercase Ed25519 public key hex + first 4 bytes of
SHA-256(`VOID/address/v1` || public key). Weak public keys, bad checksums and
noncanonical encodings are rejected. Account nonce starts at 0 and must match
exactly. Positive transfers, checked arithmetic, minimum fees, valid signatures,
correct network/version, at most 128 transactions/block and exact parent/height
are mandatory. Self-transfers are allowed and only burn the fee.

SQLite stores immutable genesis and ordered signed blocks, not a trusted balance
snapshot. Startup replays and verifies every block. A write commits to SQLite
before memory changes. Full synchronous rollback journaling and an exclusive
database lock prevent two processes sharing a database. Pending transactions
are volatile (maximum 512); they are validated against an ordered projection of
the pending pool and revalidated after every accepted block. RPC balance is the
confirmed balance; pending nonce is reported separately.

## Networking and future services

Default CLI and browser APIs permit only numeric loopback HTTP endpoints, with no proxies/redirects,
timeouts and bounded request/response sizes. No public listener, discovery,
telemetry, TLS, background gossip or anonymity transport exists. `sync --peer`
explicitly pulls a bounded block suffix and pending transactions; each is locally
verified. This supports controlled local devnets, not an adversarial P2P network.
Re-run manual sync to catch changes after its initial peer snapshot.

An **opt-in friends testnet profile** now adds a separate read-only peer listener
bound to an explicitly assigned Tailscale IP (`100.64.0.0/10` or
`fd7a:115c:a1e0::/48`; loopback also allowed for process tests). It requires a
shared 256-bit access credential, a pinned chain ID, a live encrypted VPN and up
to eight explicit peers. It has no wallet/signing/static-file or mutation routes.
No public/wildcard/LAN listeners or arbitrary remote DNS/URLs are enabled.
Browsers continue using each machine's local UI. Automatic pull sync validates
prefixes, bounded block pages and pending data; invalid data cannot mint funds.
An optional separate unfunded authority key signs nonempty blocks on a schedule.
No authority is elected and no Byzantine finality or failover is introduced.
Observed conflicting authority signatures are saved outside the unchanged v1
SQLite schema; operational quarantine survives restart and selects no fork.
The authority database/key must not be cloned/reverted. This profile is a private
cross-computer test, not the public-network consensus or privacy-v2 stage.
See [setup and limits](docs/FRIENDS_TESTNET.md).

The v1 Wallet/Explorer now consumes this API without changing ledger or consensus.
A future private ledger needs a separate adapter; transparent address lookup
must not become private wallet scanning. Pay, Names, SDK and Gateway remain
future work. Names need a threat model; payment links must bind network/address/
amount. The current receive QR is a raw v1 address and is labelled accordingly.
