# ORVEXA validation — latest 2026-10-18

## Desktop preview 1 and download site — 2026-10-18 (current delivery)

Release: `v0.1.0-desktop-preview.1`. Windows x64 standalone `ORVEXA.exe`
(12,990,464 bytes) embeds the English UI, Rust signing Wasm and open-source
notices. ZIP: `ORVEXA-Desktop-Windows-x64.zip` (5,535,705 bytes).
The app uses the installed Microsoft WebView2 runtime.

SHA-256:

```text
4b6f3a16c44a40ad9741c4ac0adb022034d104c51fac57b7b2d8578dfc6dde9e  ORVEXA.exe
fea4bc77c51ab9d5d1fc66b9c117065eccf89eb6f91353ae394e21e57c7ac3a5  ORVEXA-Desktop-Windows-x64.zip
```

Executed on Windows x64:

- `cargo fmt --all -- --check`, `cargo check --workspace --locked`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`:
  PASS. `cargo test --workspace --locked`: **48 passed**, including the existing
  real 100-local-node P2P test and four new desktop-controller tests.
- Pinned Rust 1.93.0 signing Wasm and TypeScript/Vite production build: PASS.
  `npm run test:unit`: **2 passed**; `npm test` with Release executables and
  `ORVEXA_NATIVE_GUI_TEST=1`, `ORVEXA_TEST_EMBEDDED=1`: **5 passed**.
- Actual desktop controller imports a common genesis/manifest, connects a real
  TCP/Noise peer, signs/submits a transfer locally, observes authority confirmation,
  finds it in Explorer, stops and resumes the same durable chain. Failed status
  reads disable actions and show unavailable rather than invented zero/stopped
  state. Evidence: `demo-data/desktop-e2e-ddjk6t/result.json`.
- Actual native Windows WebView2 window renders embedded assets and creates an
  encrypted wallet through Rust Wasm. Evidence:
  `demo-data/desktop-e2e-ddjk6t/native-window-result.json`.
- Existing two-wallet browser test: A → B → A, actual transaction hashes,
  balances, fees and restart: PASS; `demo-data/web-e2e-rivaXy/result.json`.
- `node scripts/verify-desktop-release.mjs release/desktop-20261018-022941-763257`:
  PASS. Extracted ZIP executable runs outside source `web/dist`, serves embedded
  Wasm and license text, matches checksums, opens its actual UI, and correctly
  returns 503 for stopped-ledger queries. Download site desktop/mobile captures,
  keyboard skip link, metadata, no script/asset errors or horizontal overflow:
  PASS. Evidence: `demo-data/desktop-release-JExB1k/result.json`.

The new `orvexa-desktop` binary and node controller supervise a verifying full
node, not an automatic block-producing validator. Existing v1 formats, keys,
economics and consensus remain unchanged. A typed-route forwarding issue found
by real transfer testing was fixed and covered by regression checks.

Limits: no live public seeds/common public network, Internet deployment test,
permissionless consensus, automatic authority failover, rewards, seed recovery,
private ledger, independent audit, installer/code signing, tray, or OS-login
autostart is claimed. Closing the preview stops its embedded node. Linux GUI
is not implemented; Linux headless build is covered separately by GitHub CI.
The E1–E8 implementation plan is in [PLATFORM_PLAN.md](PLATFORM_PLAN.md).

The older testnet artifacts below remain historical, separate deliverables.

## Native P2P — 2026-10-18

Tested Windows archive:
`release/ORVEXA-native-p2p-testnet-windows-x64-20261018-020251-343458.zip`.
SHA-256: `527d1d73fb84a1d81fbbb607b5b4c5b52e07f8051efae5116d704cb1fb6a23e1`.
Final extracted-bundle result:
`demo-data/p2p-bundle-check-adc0798fea854c6caedee1f5898abb8d/result.json`.
Earlier native timestamped ZIPs are superseded by this tested archive.

No external service/account/VPN/token is required in this new optional mode.
Pinned libp2p 0.56.0, TCP/Noise/Yamux, separate persisted transport PeerId,
numeric public seed addresses, genesis pin, bounded anti-entropy/peer discovery,
signed tip claims and durable conflict quarantine. No transaction, monetary,
wallet encryption or consensus encoding was changed. Existing local v1 and
private-overlay modes remain available. No live public seed was supplied.

Executed on Windows x64:

- `cargo fmt --all -- --check`, `cargo check --workspace --locked`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: PASS.
- `cargo test --workspace --locked`: **44 passed**, including 100 real local
  TCP/Noise nodes (one authority plus 99 replicas), transfers/offline/restart,
  wrong genesis, strict address/identity limits, forged headers, unsigned height
  spoofing, invalid parent, oversized pages, invalid/duplicate pending transfers,
  conflict evidence/restart latch and released observation slots after peer churn.
- Release node/CLI build with `-C target-feature=+crt-static` into `target/friends`:
  PASS. Original user node processes were preserved. Node DLL string inventory
  contains Windows system DLLs and no external vcruntime/msvcp DLL name.
- Pinned Rust 1.93.0 Wasm wallet/wasm-bindgen and `npm ci --ignore-scripts`,
  TypeScript/Vite production build: PASS. No wallet dependency downgrade.
- `npm run test:unit`: **2 passed**. Chromium `npm test` using new Release
  executables: **3 passed**, including local signing, two-way transfer, native
  backup compatibility, QR, actual Explorer results, network pin and restart.
- `scripts/demo-p2p.ps1`: PASS on three separate Windows OS processes over
  native TCP/Noise. Final height 3, B balance 198, burned fees 4; first actual
  transaction has 3 Explorer confirmations. Owner offline transfer, replica
  catch-up, authority offline pending and authority restart confirmation pass.
  Evidence: `demo-data/p2p-20261018-015742-d6e3568f/result.json`.
- Legacy `scripts/demo.ps1 -SkipBuild -BinaryDir target/friends/release -BasePort 58242`:
  PASS. Evidence: `demo-data/20261018-020117-05eabb88/result.json`.
- Extracted native Windows bundle: launcher, native peer listener, stable separate
  identity, CLI, HTML and signing Wasm pass. A PowerShell 5.1 single-seed scalar
  conversion bug was found and corrected. Supplemental upstream notices are
  pinned with provenance/hashes; 364 selected Cargo/npm packages are included.

Files changed/added in this stage: root `Cargo.toml`, `Cargo.lock`, `.gitignore`,
`START-ORVEXA.cmd`, `README.md`, `ARCHITECTURE.md`, `THREAT_MODEL.md`, `PRIVACY.md`,
`ROADMAP.md`; `.github/workflows/ci.yml`; `crates/orvexa-node/Cargo.toml`,
`crates/orvexa-node/src/{lib,main,mesh,p2p}.rs`, `crates/orvexa-rpc/src/lib.rs`,
`crates/orvexa-tests/Cargo.toml`, `crates/orvexa-tests/tests/p2p.rs`;
`scripts/{start-p2p,new-p2p-network,demo-p2p,verify-p2p-bundle,testnet-common,package-testnet,collect-licenses,update-upstream-notices}.ps1`;
`docs/{P2P_DESIGN,NATIVE_P2P,THIRD_PARTY,VALIDATION}.md` and pinned
`docs/third-party-notices/` files. UI source and transaction/crypto/wallet/core
rules were not rewritten.

Limits: local tests are not 100 independent Internet operators. Actual reachable
volunteer seed(s), router/firewall checks and prolonged multi-network acceptance
are still needed. No Linux execution on this Windows host, automatic authority
service unlock, permissionless validator consensus/rewards, Sybil/eclipse
resistance, private ledger or independent audit is claimed.

The older VPN artifact/results below are retained as history; the native P2P
delivery and operating instructions are in [NATIVE_P2P.md](NATIVE_P2P.md).

## Friends testnet — 2026-10-18

Prepared an opt-in version for separate computers connected through an encrypted
private VPN. Each machine keeps its own chain database and loopback Wallet/Explorer.
A separate read-only, authenticated peer listener exchanges signed v1 data;
automatic pull replication and a single separate-key authority confirm transfers.
No election, validator quorum, authority failover, public Internet listener or
privacy-v2 backend was introduced. Existing local workflows and canonical
transaction/block/address encodings, supply, fees and SQLite schema remain intact.

### Windows artifact

`release/ORVEXA-friends-testnet-windows-x64-20261018-014000-fa6af8.zip`
(5,136,164 bytes), SHA-256:

```text
c1a3cbe426cbe0a5cc89d4a8c2bdc45858fdfb14bfdf100984c7a2c68f6b88c4
```

Contains Release x64 node/CLI executables with static MSVC CRT, tested compiled
web/Wasm assets, startup/network-creation scripts, guide and copied upstream
license notices for 244 installed packages (including build/test dependencies).
PE import inspection found no external vcruntime/msvcp dependency in the node.
No wallet, node database, genesis or join credential is in this application ZIP.
The packager creates a separate `.sha256` sidecar. Earlier timestamped archives
are superseded; use the exact tested artifact above.

Standalone extraction/launcher verification passed:
`demo-data/bundle-check-ab857721716c4cb48c2bb2b5ab06e2b3/result.json`.
It starts the copied script/node/CLI, serves HTML and signing Wasm, reports the
correct chain/replica role, rejects unauthenticated mesh requests, checks notice
inventory and stops only its own children. Every bundled web asset was compared
byte-for-byte with the tested build. Recipients need Windows x64 and a connected
private VPN; Rust/Node.js/Python are build-time tools, not recipient requirements.

### Executed checks

- `cargo fmt --all -- --check`, `cargo check --workspace --locked` and
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  passed. Clippy has no warnings.
- `cargo test --workspace --locked`: **38 tests passed** (32 previous + 6 mesh).
  New coverage includes credential/URL/chain-pin boundaries, narrow read-only
  routes, authentication/browser-origin rejection, request caps, automatic
  three-node propagation, owner offline/rejoin, authority offline/restart,
  preserved conflict evidence/quarantine across restart, forged/foreign data,
  separate authority keys and no stale-prefix signing after newer-peer fetch failure.
- `cargo build --workspace --locked --target-dir target/friends` passed. The
  separate target avoided overwriting the already-running original local node.
  Release native build with `-C target-feature=+crt-static` and the pinned Rust
  1.93.0 Wasm/wasm-bindgen build passed.
- `npm ci --ignore-scripts`, `npm run build` and `npm run test:unit` passed;
  **2 unit tests** retain exact integer parsing. npm reported zero vulnerabilities
  in its installed dependency audit; this is not an independent security audit.
- **3 Chromium E2E tests passed using extracted Release binaries**, with real
  signing/transfers, backup interop, Explorer and restart. A faster Release run
  exposed a first-load network-pin race; it was fixed by re-reading the pin after
  worker verification. The regression now deliberately gates Wasm loading and
  also tests a chain change from another tab. Final result:
  `demo-data/web-e2e-TNaqTm/result.json` (height 2, A 999788, B 209, burned 3).
- Original three-process demo passed with the new binaries:
  `scripts/demo.ps1 -SkipBuild -BinaryDir target/friends/debug -BasePort 51242`.
  Evidence: `demo-data/20261018-013031-04dae981/result.json`.
- New `scripts/demo-testnet.ps1` passed with both Debug and Release executables.
  Latest Release evidence: `demo-data/friends-20261018-013516-9aa3e7b7/result.json`.
  Three distinct OS processes gossip automatically; a second transfer confirms
  while the owner's process is stopped, the owner catches up after restart,
  authority downtime leaves a third transfer pending, and authority restart
  confirms it. At height 3, Bob has 198 and burned fees are 4; all tips match.
  The join ZIP contains exactly genesis.json, network.json and mesh.secret.
- All PowerShell sources parse. Local document links resolve. Existing user
  local node processes were left running; disposable test children were stopped.

### Scope and remaining acceptance

The network tests use loopback transports between actual OS processes. **Actual
different computers, Tailscale accounts, NAT traversal, router/firewall rules and
long-duration availability have not been tested here.** The account/VPN/firewall
settings were not changed. The operator guide explains these final device checks;
the artifact is prepared for that private test, not certified as production-ready.
GitHub Actions was extended with the new Windows demonstration; remote CI was
not executed during this local session.

One authority must remain online for confirmations; replicas never elect a
replacement. A dishonest invited peer advertising a higher chain can stall
production. Conflict quarantine depends on observing the alternate signature;
there is no independent signer journal, safe standby, authority rotation or
automatic branch recovery. Do not clone/revert an active authority database.
Authority key is held in local process memory; no password file/autostart service
was installed. Pending transactions remain volatile. Full chain in memory,
bounded history, transparent amounts/addresses and no independent audit remain.
No real money, exchange listing or anonymity claim is made.

### Files changed in this stage

- `Cargo.toml`, `Cargo.lock`, `crates/orvexa-node/Cargo.toml`.
- `crates/orvexa-rpc/src/lib.rs`, `crates/orvexa-node/src/lib.rs`,
  `crates/orvexa-node/src/main.rs`, `crates/orvexa-cli/src/main.rs`.
- New `crates/orvexa-node/src/mesh.rs` and `crates/orvexa-tests/tests/mesh.rs`.
- New `scripts/testnet-common.ps1`, `new-testnet.ps1`, `start-testnet.ps1`,
  `package-testnet.ps1`, `collect-licenses.ps1`, `demo-testnet.ps1`,
  `verify-testnet-bundle.ps1`; existing `scripts/demo.ps1` gained an optional
  binary-directory argument (default behavior retained).
- `web/src/App.tsx`, `web/tests/wallet.spec.ts` (pin-race fix/regression,
  cross-tab notification, testnet copy and alternate binary path for tests).
- `.gitignore`, `.github/workflows/ci.yml`, `README.md`, `ARCHITECTURE.md`,
  `THREAT_MODEL.md`, `PRIVACY.md`, `ROADMAP.md`, `PRODUCT.md`, `docs/API.md`,
  `docs/WEB_WALLET.md`, this log; new `docs/FRIENDS_TESTNET.md` and
  `docs/THIRD_PARTY.md`.

Core/crypto and wallet formats were preserved. SVG/PNG brand sources and the
privacy-v2 proposal were not changed. Build/test artifacts and private test data
remain ignored by Git. [Operator instructions](FRIENDS_TESTNET.md).

## Delivered English Wallet / Explorer — transparent v1 (2026-10-17)

Implemented the requested running UI, not a static mockup. Dashboard, Send,
Receive/QR, Transactions, Wallets, Backup & Restore, Settings and Explorer use
the actual loopback Rust node. A new Rust/Wasm crate reuses the native v1 wallet
container and canonical signing in disposable browser workers. The node receives
signed data, not wallet files or passwords. Opt-in `--web-dir` enables the exact
same-origin browser host; default CLI Origin rejection remains in place.
New read-only routes return real IDs, block details, transaction status and a
coherent UI snapshot. Existing transaction bytes, signature domains, address
prefix, account rules, block consensus, SQLite schema and economics are unchanged.
No privacy-v2 backend was added or activated.

### Executed locally on Windows

- `cargo fmt --all -- --check` — passed.
- `cargo check --workspace --locked` — passed.
- `cargo test --workspace --locked` — **32 passed** (29 existing + 3 web/API tests).
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — passed without warnings.
- `cargo build --workspace --locked` — passed.
- `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-web.ps1`
  — passed: Rust 1.93.0 Wasm release build, pinned wasm-bindgen 0.2.129,
  `npm ci --ignore-scripts`, TypeScript and Vite production build.
- In `web`: `npm.cmd run test:unit` — **2 passed**: exact small/u64/above-2^53
  integer parsing and rejection of fractional/prototype-pollution inputs.
- In `web`: `npm.cmd test` — **3 passed** using installed Playwright Chromium:
  native wallet import, actual Wasm signing in both directions, real pending and
  confirmed hashes, block production, two confirmations, SQLite restart,
  browser-created backup authenticated by native CLI, wrong password rejection,
  block/address detail views, sent/received history, unknown-hash 404 and explicit
  adoption after network-pin mismatch.
- Browser request capture found no test password or encrypted wallet posted to
  HTTP; no browser runtime errors occurred. This is scoped test evidence, not a
  guarantee against a compromised browser, extensions or malicious served code.
- Desktop 1440×1000 and mobile 390×844 screenshots cover Dashboard, Send,
  Receive, Transactions, Wallets, Backup, Settings and Explorer. Mobile menu keyboard focus,
  Escape, closed-menu `inert`, same-route close and page overflow checks passed.
  Screenshots: `web/test-results/wallet-native-backups-brow-55221-nsfers-Explorer-and-restart/`.
- Original three-process demonstration:
  `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1 -SkipBuild -BasePort 48242`
  — **PASS**, including signature/replay rejection, sync and restart.
  Evidence: `demo-data/20261017-234531-294730e3/result.json`.
- New launcher smoke test with existing disposable test data:
  `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -BasePort 47242 -DataDir demo-data\launcher-smoke-c05fcff3dd1c4139985485b1fed4b3ea -RunForSeconds 8`
  — passed. All three separate SQLite histories match at height 2; spawned node
  processes were stopped. The smoke test used copied test data, not user wallets.

Final browser evidence: `demo-data/web-e2e-GAHYpQ/result.json`:

```text
height = 2; A = 999788; B = 209; burned fees = 3
chain = 3faa3ae66c95210df4b31459d8082b0c0ed3d74b4d804fc4df9b329ad876148c
tip   = d47c18535641a9a811eef25a5c20e21a61e276869e26008b5dd6b6aa02781512
A→B   = 0eab067b109483b3bc80e3c2573776569e46ff0edb3112ae30fa7bf581502473
B→A   = cf35c22bac5f9b724d0cc00845042a04b6703a6d5e1c3fbec49cb898750e7ded
```

GitHub Actions now includes a pinned Wasm/Node/Chromium build and browser tests
alongside the existing Windows/Linux Rust checks. The remote workflow has not
been executed here. Test data, compiled artifacts and screenshots are ignored
by Git; no reusable test passwords are retained.

### Implementation inventory

Changed existing files:

- `Cargo.toml`, `Cargo.lock`, `.gitignore`, `.github/workflows/ci.yml`.
- `crates/orvexa-wallet/src/lib.rs`, `crates/orvexa-cli/src/main.rs`.
- `crates/orvexa-node/src/lib.rs`, `crates/orvexa-node/src/main.rs`,
  `crates/orvexa-rpc/src/lib.rs`.
- `README.md`, `ARCHITECTURE.md`, `PRIVACY.md`, `THREAT_MODEL.md`, `ROADMAP.md`,
  `docs/API.md`, `docs/PRIVACY_DESIGN.md`, `docs/PRIVACY_TEST_PLAN.md`, this log.

Added source files:

- `crates/orvexa-web-wallet/Cargo.toml`, `crates/orvexa-web-wallet/src/lib.rs`.
- `crates/orvexa-node/src/web.rs`, `crates/orvexa-tests/tests/web.rs`.
- `scripts/build-web.ps1`, `scripts/start-local.ps1`, `scripts/trace-brand.py`.
- `web/package.json`, `web/package-lock.json`, `web/index.html`,
  `web/tsconfig.json`, `web/vite.config.ts`, `web/playwright.config.ts`.
- `web/src/main.tsx`, `App.tsx`, `adapter.ts`, `lossless.ts`, `vault.ts`,
  `signer.worker.ts`, `tokens.css`, `styles.css`.
- `web/tests/adapter.test.mjs`, `web/tests/wallet.spec.ts`.
- `web/public/brand/orvexa-symbol.svg`, `orvexa-wordmark.svg`, `orvexa-logo.svg`.
- `PRODUCT.md`, `DESIGN.md`, `.impeccable/design.json`,
  `docs/WEB_WALLET.md`, `docs/BRAND.md`.

The original `ORVEXA.png`, core/crypto protocol code, legacy fixtures and
`scripts/demo.ps1` were preserved. SVG paths reconstruct the selected raster
front face; they are not the unavailable original vector master. No bitmap is
embedded in the SVGs. [Brand provenance](BRAND.md),
[PowerShell startup and two-wallet checks](WEB_WALLET.md).

### Remaining limits

Transparent test network only; one block authority, no Byzantine finality,
faucet, post-genesis issuance, private transfers or mnemonic recovery. Pending
transactions are volatile. UI history is bounded to 256 confirmed transfers;
block pages contain eight blocks. Receive QR is an address, not a network-bound
invoice. Browser storage requires a separate encrypted backup. JavaScript secret
erasure cannot be guaranteed. The original interrupted native file-publication
limitation remains. Local origin controls are not authentication of local
programs. No independent audit or real-funds readiness claim is made.

## Privacy design revision 2 — Monero comparison (historical; documentation only)

At the founder's request, [PRIVACY_DESIGN.md](PRIVACY_DESIGN.md) now compares
Monero RingCT/CLSAG and Orchard/Ironwood across the seven requested criteria,
records reusable library boundaries, audit provenance, wallet/Explorer behavior,
and a conditional shielded-note recommendation. The former adapter-first P1 is
superseded: P1 is now upstream evaluation outside the working v1 workspace;
custom protocol/adapter work is deferred to a separately reviewed P2. No stage
of implementation was approved or started.

Research rechecked official pinned sources: monero-oxide at
`731657ae3385be667abb556266369a497bc86f13`, its manifests and wallet caveats,
the Cypher Stack audit's scope and provenance, Orchard 0.16.0 sources and the
official soundness advisory. Audit remediation/diffs, candidate builds, proof
benchmarks and end-to-end private wallet behavior are not yet verified.
FCMP++ is distinguished from the CLSAG library examined here.

SHA-256 comparison against a pre-edit inventory confirms no changes to Rust,
Cargo manifests/lock, tests/fixtures, CI or scripts. All local Markdown file links
were checked and resolve. Only these eight documentation files changed in this
revision: README.md, ARCHITECTURE.md, PRIVACY.md, THREAT_MODEL.md, ROADMAP.md,
docs/PRIVACY_DESIGN.md, docs/PRIVACY_TEST_PLAN.md and this validation log.
No dependency was added and no experimental cryptography was executed.

Cargo checks and the three-process demo were not rerun for this documentation
revision. The following original-design and rename sections preserve the actual
earlier successful checks of the unchanged transparent v1; they do not certify
the proposed private protocol.

## Original privacy design review (historical; documentation only)

Added [the v2 design proposal](PRIVACY_DESIGN.md) and
[the staged test matrix](PRIVACY_TEST_PLAN.md). Current code was audited before
the proposal. The original 29 tests passed; fmt/check/all-targets-all-features
Clippy/build also passed. No private-proof tests were added or run in this turn:
P1 implementation explicitly awaits founder approval.

`cargo info orchard@0.16.0 --registry crates-io` confirmed the published version,
license and declared Rust requirement, and its downloaded source was inspected.
It is not a workspace dependency; its full build/proving performance has not
been tested. Existing source hashes confirm no Rust, Cargo manifest/lock, script
or CI changes before approval. Only documentation was edited.

The existing three-process demo passed again:
`demo-data/20261017-230015-6d835b35/result.json`. All nodes had height 1,
recipient balance 250, burned fees 2, and tip
`6986df5d137cb217cae28f24ac1b9fc277dfeae42f735210eaf094db02ef2a81`.
Invalid signatures/replays were rejected and restart restored the same chain.
This verifies the unchanged transparent v1, not the proposed private protocol.

## Rename audit

See [the audit report](RENAME_AUDIT.md) for the complete remaining-name
classification, fixes and exact three-node startup commands. All requested checks
were executed again locally: `cargo fmt --check`, `cargo check --workspace`,
`cargo test --workspace` (29 passing tests), and
`cargo clippy --workspace --all-targets --all-features -- -D warnings`.
`cargo build --workspace --locked` and the strengthened three-process demo passed.
CI now includes all workspace features in Clippy.

Latest demo: `demo-data/20261017-224714-025c1fae/result.json`; three distinct
`orvexa-node` PIDs (19548, 19104, 2728), correct UTF-8 ORVEXA network labels,
height 1, recipient balance 250, burned fees 2, identical recovered tip
`ebba95163a3c8718f0b5f81cdbcaf1cc0d775fa8d2674e403cbf38a95ed41349`.
No protocol, consensus or economic changes were made.

## Rename validation

The existing project was renamed to the provisional working name ORVEXA.
The [rename inventory](RENAME_ORVEXA.md) records all affected files and intentional
v1 compatibility identifiers. The baseline suite passed 25 tests before changes.

| Executed check after rename | Result |
| --- | --- |
| cargo fmt --all | PASS |
| cargo check --workspace --locked | PASS |
| cargo test --workspace --locked | PASS: 29 tests, 0 failures |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS |
| cargo build --workspace --locked | PASS: orvexa-node and orvexa-cli |
| powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1 -SkipBuild | PASS: three ORVEXA node processes, transfer and restart |
| orvexa-cli.exe --help / orvexa-node.exe --help | PASS: new product and binary names |

The four added compatibility tests verify pre-rename canonical addresses, chain
ID, signing bytes, transaction/block signatures and IDs, economics, decryption of
an original wallet, the original SQLite schema, and replay of an unchanged actual
database from the initial three-process demo. Public test fixtures only; the
fixture wallet password is intentionally known and never used for real funds.
All 36 pre-existing demo artifacts retain their original SHA-256 hashes.
Cargo.lock differs only in local package names; dependency versions are unchanged.

Renamed demo evidence: `demo-data/20261017-224113-100ca9db/result.json`.
All three nodes: height 1, recipient balance 250, burned fees 2,
tip `e76cd15d10243563696032cb3c1fe3b918e8dde2f9d35131012fbbaead9a8b5d`.

## Initial prototype validation before rename

Environment: Windows x86_64 MSVC, rustc 1.93.0, cargo 1.93.0.
Initial workspace was empty; no existing user files were removed.

| Executed check | Result |
| --- | --- |
| cargo fmt --all -- --check | PASS |
| cargo check --workspace --locked | PASS |
| cargo test --workspace --locked | PASS: 25 tests, 0 failures |
| cargo clippy --workspace --all-targets --locked -- -D warnings | PASS |
| cargo build --workspace --locked | PASS: node and CLI binaries built |
| powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1 -SkipBuild | PASS: three independent node processes, transfer, rejection checks and restart |

Test coverage: 2 crypto tests, 1 storage failure unit test, 12 protocol tests,
4 wallet/storage integration tests, 6 HTTP/network integration tests.
Tests cover strict signatures and checksums, network/version separation,
nonces and pending overspend, zero amounts/fees and overflow, self-transfers,
exact balance spending, unauthorized blocks, wrong parents/heights/timestamps,
body commitments and atomic rejection, immutable test supply, JSON encoding,
password/ciphertext/metadata tampering, bounded KDF parameters, exclusive database
access, corruption on replay, volatile pending transactions, database write
failure, peer pagination and malformed pages, foreign genesis and conflicting
histories, browser Origin and body size rejection.

## Three-process evidence

Final successful demo run: `demo-data/20261017-222734-2339dc91/result.json`.
The script generated a fresh random network and encrypted test wallets, then
stopped all processes. Test passwords were not persisted.

- Chain ID: `d015a69e0e397a2a257f76261003d638985b67ef5ade9bf0d17170df3e08bb84`
- Transaction ID: `d8f5f2fc2b60596ddaaa9d7a395380ddbeb309119d3e5b2fd46802b530953f11`
- All three tips: `f5c2e974811d89fa9caed18d2404a77cf23f3323e63472932f28f1503497e205`
- All three heights: 1; recipient balance: 250; burned fees: 2.
- Invalid signature, pending duplicate and confirmed replay: rejected.
- Forced process termination and restart: same validated chain and balance.

GitHub Actions workflow is supplied for Windows/Linux but has not been run on
GitHub; only the local Windows results above are verified. These checks establish
a working devnet prototype, not privacy, adversarial consensus safety or an
independent security audit.
