# ORVEXA roadmap

Active product sequence: [PLATFORM_PLAN.md](docs/PLATFORM_PLAN.md), E1–E8.
The founder authorized continued staged development. Main requirement: private
payments/balances by default, explicit public mode and selective disclosure.
E1 starts native app/node management; privacy backend, seed recovery, BFT
admission/failover and consensus rewards have separate implementation gates.

1. **Delivered:** buildable workspace, transparent signed ledger, encrypted CLI wallet,
   loopback API, transactional persistence and executable end-to-end tests.
2. **Delivered for local testing:** explicit pull sync, bounded block pagination,
   three independent database processes and rejection of conflicting tips.
   Add scheduled gossip, rate limits, durable equivocation evidence, adversarial network tests
   and an explicit strategy for authority equivocation before expanding networking.
   **Delivered transparent-v1 UI:** English Wallet/Explorer, browser Rust/Wasm
   signing, encrypted native-compatible backup/restore, real transaction/block
   queries and a three-node PowerShell launcher. Two-way transfers, Explorer,
   mobile navigation and persisted restart are covered by real-browser tests.
   See [WEB_WALLET.md](docs/WEB_WALLET.md). This does not implement private v2.
   **Delivered friends-testnet profile:** explicit authenticated read-only peers
   over an encrypted private VPN, automatic pull replication, separate-key
   authority production, observed-conflict evidence/quarantine and a Windows
   app/join packaging workflow. Ordinary replicas may disconnect/rejoin; the
   authority must stay online for confirmations. Actual multi-computer/VPN
   acceptance still needs operators' devices. This is not validator quorum,
   public gossip, fork choice, production hosting or privacy-v2 implementation.
   See [FRIENDS_TESTNET.md](docs/FRIENDS_TESTNET.md).
   **Delivered native P2P prototype:** TCP/Noise/Yamux without external service,
   separate persisted transport identity, pinned public seeds/genesis, bounded
   automatic block/transaction exchange, signed tip claims, peer discovery and
   reused conflict quarantine. Windows one-file launcher and public join pack;
   local 100-node TCP/Noise test plus three real Windows-process transfer/restart
   demonstration. Actual Internet acceptance awaits reachable volunteer nodes;
   no live seed has been supplied. Multi-authority consensus, node incentives,
   anti-Sybil defenses and privacy are separate work. See [NATIVE_P2P.md](docs/NATIVE_P2P.md).
3. **Design revision 2; implementation awaits approval:**
   [privacy v2](docs/PRIVACY_DESIGN.md) compares Monero RingCT/CLSAG and
   Orchard/Ironwood across seven criteria. It recommends shielded notes, with
   the exact stack conditional on evidence. P1 evaluates upstream in a separate
   workspace; P2 reviews the protocol and offline adapter; P3 adds atomic
   note-tree/nullifier state; P4 desktop wallet transfers/recovery; P5 separate
   local v2 network and read-only Explorer; P6 metadata analysis and independent
   audit. See [acceptance tests](docs/PRIVACY_TEST_PLAN.md). Existing v1 remains
   available. No privacy claims before implementation, validation and audit.
4. Revisit permissionless consensus, emission and bootstrapping with simulation,
   reorg/property/fuzz tests and independent security audits. No mainnet timeline.
5. Stabilize SDK, network-bound payment links and the delivered v1 wallet UX. Then consider Pay,
   Names and Gateway separately. Business API/support revenue must not require a
   subscription for ordinary base transfers or promise token price appreciation.

The next proposed privacy task, **after separate founder approval**, is P1 in
PRIVACY_DESIGN.md: upstream evidence, genuine fixtures and resource measurements
in a separate evaluation workspace. No custom ORVEXA format, genesis or adapter
is included in that stage. Existing consensus and economics remain unchanged. Durable
equivocation handling and independent review remain required before public networking; atomic wallet
publication and recovery tests are required before the v2 wallet. The delivered
browser wallet uses the existing transparent v1 container and ledger; it does not
implement those privacy tasks. See docs/VALIDATION.md for executed checks.
