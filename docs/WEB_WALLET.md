# Wallet & Explorer — local ORVEXA v1

Working English browser UI with the actual Rust backend. Transfers are
**transparent**: addresses, amounts and links are public. Only test atoms;
no real funds. The separate privacy-v2 design is not activated.

For separate computers and automatic confirmations, use the opt-in
[friends-testnet guide](FRIENDS_TESTNET.md). It preserves the local workflow below,
with a separate authority key and private-VPN peer listener. Wallet/Explorer
continues to run on each participant's loopback origin.

## Windows PowerShell: build and start

Requirements: Rust 1.93+, Node.js 24+, npm and the native Windows Rust build tools
already used by this project. Internet is needed for build dependencies. The
built app uses no CDN, external fonts, telemetry or remote services.

```powershell
Set-Location 'C:\Users\waxie\Desktop\void'
cargo build --workspace --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-web.ps1 -InstallTools
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -OpenBrowser
```

The build script installs exact Rust 1.93.0 with the Wasm target and
wasm-bindgen CLI 0.2.129. The separate toolchain avoids the missing `rust-lld`
in the original local stable installation. Later builds may omit `-InstallTools`.

On first launch, choose and repeat passwords for **Wallet A and Wallet B**
(at least 12 characters). They are not command arguments, logged, retained by
the script or sent to the node. Open [the app](http://127.0.0.1:4242); the exact
numeric origin is required, not `localhost`.

Three hidden node processes use separate SQLite files on ports 4242–4244.
The first hosts Wallet/Explorer; the launcher pulls its blocks and pending
transfers to the peers every two seconds. This is local replication, not P2P
gossip. Ctrl+C stops only the children created by this invocation.

Files in `data\local-web`: `wallet-a.wallet.json` (genesis recipient/authority),
`wallet-b.wallet.json`, common `genesis.json`, independent `node-0.sqlite` through
`node-2.sqlite`, and node diagnostic logs without wallet secrets. Existing
wallets/chain are never overwritten or reset. Use `-BasePort 4342` for other ports,
`-DataDir data\another-network` for a separate chain, or `-Nodes 1` for one node.
Each browser origin has separate storage; changed ports require wallet imports.

## Check two wallets

1. Open **Wallets → Restore wallet**. Select
   `data\local-web\wallet-a.wallet.json`, name it `Wallet A`, enter A's password,
   download the encrypted backup and continue. Repeat for `wallet-b.wallet.json`
   as `Wallet B`, using B's password. Authentication runs locally in a worker.
2. Select A in the header. Dashboard reads **1,000,000 test atoms**; B has zero.
   The one allowed funding mechanism is genesis. There is no faucet, admin
   balance endpoint or later minting. Creating another browser wallet gives no coins.
3. Select B → **Receive** and copy its full address. Select A → **Send**, paste
   B's address, set amount **250**, fee **2**, review, and sign with A's password.
   Acceptance means **pending**, not confirmed.
4. Open the receipt's **View in Explorer** link. Its real hash has zero
   confirmations. Click **Produce authority block** and use **A's password**.
   Dashboard's **Produce block** works too. No authority secret is on the server.
5. The transfer is now in block 1: **A = 999,748**, **B = 250**, burned fees **2**.
   Search the actual hash in Explorer; positive block heights and public v1
   addresses are searchable too.
6. Select B and send **40** to A with fee **1**, signing with **B's password**.
   Produce the next block with **A's password**, even while B is selected.
   At height 2: **A = 999,788**, **B = 209**, burned fees **3**. Balances plus
   burned fees equal 1,000,000. The peers catch up on the next launcher poll.
7. Press Ctrl+C in the launcher terminal, then restart with the same data:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -OpenBrowser
   ```

   No wallets/genesis are recreated. Confirmed balances, hashes and blocks
   recover from SQLite. Pending transfers are volatile and may disappear;
   a missing hash is never displayed as confirmed.

Read-only checks from another terminal:

```powershell
$a = .\target\debug\orvexa-cli.exe address --wallet .\data\local-web\wallet-a.wallet.json
$b = .\target\debug\orvexa-cli.exe address --wallet .\data\local-web\wallet-b.wallet.json
Invoke-RestMethod "http://127.0.0.1:4242/v1/balance/$a"
Invoke-RestMethod "http://127.0.0.1:4242/v1/balance/$b"
4242..4244 | ForEach-Object { Invoke-RestMethod "http://127.0.0.1:$_/v1/status" } |
    Select-Object chain_id,height,tip,burned_fees,pending
```

For an existing manual database:

```powershell
.\target\debug\orvexa-node.exe run --db .\data\node-a.sqlite --listen 127.0.0.1:4242 --web-dir .\web\dist
```

Omit `--web-dir` to keep the original CLI-only origin policy. Canonical v1
transactions, legacy domains/addresses, consensus, database schema and economics
remain unchanged. The launcher expects A to be both genesis recipient/authority;
other configurations use the manual commands.

## Wallet and browser security

`orvexa-web-wallet` reuses Rust core/wallet through Wasm. A fresh worker performs
each operation and terminates afterwards; no unlocked key/session is retained.
Only signed transaction/block bytes are posted. No key/seed/password endpoint
exists. Browser storage contains encrypted v1 containers, public addresses and
local labels; **it is not a backup**. Password fields are cleared after use, but
erasure of all JavaScript/browser copies cannot be guaranteed. Malware,
extensions and modified app code can compromise an active wallet.

The native encrypted format is unchanged: Argon2id (64 MiB / 3 passes / 1 lane),
XChaCha20-Poly1305 and authenticated public metadata. Restore verifies the
password before storing a file. **Backup & Restore** downloads encrypted files
and tests them locally. Browser-created files work with the native CLI:

```powershell
.\target\debug\orvexa-cli.exe wallet-verify --wallet .\data\local-web\wallet-a.wallet.json
```

There is **no mnemonic seed phrase** in v1. The encrypted file plus password
restores its key/address; labels are browser-local and history comes from the
selected chain. Verify backups before deleting any browser copy.

The first validated genesis pins the network. A changed chain disables signing
until reviewed in Settings. Host/Origin/fetch-site checks, restrictive CSP,
no CORS, no framing/referrer and no-store responses protect the optional browser
host. It does not authenticate hostile local programs. The default API still
rejects all browser Origins. This is not an audited wallet or an independently
validating light client; the served app and local validating node are trusted.

## Pages and integration

Dashboard, Send, Receive/QR, Transactions, Wallets, Backup & Restore and Settings
share tokens/components. Explorer reads newest/older blocks, block details,
actual IDs, pending/confirmed states and public v1 addresses. Confirmations count
accepted blocks, not Byzantine finality. Logical slots are not calendar dates.
The QR contains the raw address, explicitly not a network-bound payment invoice.

`/v1/ui-state?address=…` reads balance/history/mempool/tip under one lock, avoiding
inconsistent observations during block acceptance. Integers remain exact decimal
strings/BigInt in the adapter. History is the latest 256 confirmed transfers;
block pages contain eight. See [API.md](API.md).

Views use `TransparentV1Adapter`. Address lookup is explicitly a v1 capability;
a private ledger needs a separate adapter, scanning and recovery model. No
experimental privacy backend is simulated or integrated here.

## Checks

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
Set-Location web
npm.cmd ci --ignore-scripts
npm.cmd run build
npm.cmd run test:unit
npm.cmd exec -- playwright install chromium
npm.cmd test
```

Browser tests run a real Rust/SQLite node, restore native wallets, sign both
directions through Wasm, check inclusion/restart, open browser backups in the
CLI, and test wrong passwords, unknown hashes, network pinning and mobile
navigation. Desktop/mobile screenshots of every page are in `web/test-results`.
Test wallet passwords are disposable and not retained. Actual results and
implementation inventory: [VALIDATION.md](VALIDATION.md). Brand: [BRAND.md](BRAND.md).
