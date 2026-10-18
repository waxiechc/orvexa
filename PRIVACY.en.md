# Privacy research and integration boundary

The detailed [v2 design proposal](docs/PRIVACY_DESIGN.md) and
[test plan](docs/PRIVACY_TEST_PLAN.md) supersede the earlier research priority
below. Revision 2 compares Monero RingCT/CLSAG with Orchard/Ironwood across seven
criteria and recommends the shielded-note architecture conditionally. Orchard
0.16.0 / Ironwood-v3 remains an evaluation candidate. P1 now evaluates upstream
outside this working workspace; custom format/adapter work is deferred to P2.
Wallet recovery and a public Explorer without balances are explicit later steps.
Implementation awaits approval. No private backend or shielded ledger is present.

The delivered English Wallet/Explorer is a **transparent-v1** client. It reuses
the native encrypted container and Rust signing via WebAssembly; encrypted files
and passwords stay in the browser. Its balance/history/address queries still
disclose public v1 metadata to the local node. `TransparentV1Adapter` separates
views from ledger access, but a future private wallet needs local note scanning,
new recovery semantics and a separate adapter. The UI does not implement any
privacy stage or imply anonymity. See [the running guide](docs/WEB_WALLET.md).

The friends-testnet profile adds encrypted private-VPN transport and authenticated
read-only peers. It preserves the same public transaction graph, one authority
and v1 ledger. Transport encryption does not hide amounts/balances from participants
or provide sender/recipient anonymity. [Operator guide](docs/FRIENDS_TESTNET.md).

## Requirements and alternatives

The optional [native P2P profile](docs/NATIVE_P2P.md) removes the VPN/service
requirement using TCP/Noise/Yamux. It preserves the transparent v1 transaction
graph and single authority. Transport encryption does not hide balances or
payment links from validating nodes, and peers can observe IP/PeerId/timing.

Recipient unlinkability, hidden amounts and sender ambiguity are different
properties. They must share compatible transaction algebra, wallet scanning,
double-spend protection, proof verification and consensus encodings.

| Family | Mechanisms | Integration consequences |
| --- | --- | --- |
| Monero-style output ledger | One-time stealth destinations; commitments and range proofs; linkable ring proofs with key images | Decoy selection, view/spend keys and confidential conservation rules; small anonymity sets remain vulnerable to heuristics |
| Shielded note pool (e.g. Zcash Orchard) | Note commitments, nullifiers and zero-knowledge spend/output proofs | Circuit/proving stack, Merkle witness maintenance, wallet scanning and large verification surface; setup assumptions depend on the chosen construction |
| Confidential amounts only | Commitments + range proofs | Hides amounts but leaves the transaction graph; insufficient for ORVEXA's full goal |

Comparison retained for context: Monero-style outputs and shielded pools solve
different integration problems. **An architecture is recommended; no exact backend
is accepted for deployment.** The current account ledger stays available as v1;
a new privacy ledger would be separate. Ed25519 account signatures cannot simply be swapped for ring
signatures without redesigning ownership, balance verification and replay rules.
Do not combine a standalone Bulletproofs crate, stealth key derivation and an
unreviewed ring implementation and infer that the resulting protocol is safe.

`orvexa_crypto::privacy::PrivacyBackend` is an integration contract only. The only
current capability report is Transparent; it never returns a fake privacy proof
or accepts a private transaction. A future implementation needs published audit
scope, license compatibility, maintained upstream, canonical vectors, malformed
proof rejection, amount conservation/range tests, unique nullifiers/key images,
wallet recovery/scanning tests and a migration/version plan.

Network privacy is separate: even a valid shielded transaction can leak its source
through first-seen timing, IP addresses, peer topology or remote wallet queries.
Tor/I2P and Dandelion++ require their own active-adversary and correlation analysis;
none is present in v0.1.

## Primary sources consulted

- [Monero technical overview](https://www.getmonero.org/resources/about/)
- [Stealth addresses](https://web.getmonero.org/resources/moneropedia/stealthaddress.html)
- [RingCT research paper](https://web.getmonero.org/resources/research-lab/pubs/MRL-0005.pdf)
- [Zcash protocol specification](https://zips.z.cash/protocol/protocol.pdf)
- [Dandelion++ paper](https://arxiv.org/abs/1805.11060)
- [ed25519-dalek 2.2 API](https://docs.rs/ed25519-dalek/2.2.0/ed25519_dalek/)
- [RustCrypto Argon2 0.5](https://docs.rs/argon2/0.5.3/argon2/)
- [RustCrypto XChaCha20-Poly1305 0.10](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/)

Dependency choices use the inspected stable API series; newer major releases are
not automatically interchangeable. Cargo.lock records the resolved graph and
local compilation/tests check compatibility with the installed Rust toolchain.
