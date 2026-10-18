# ORVEXA friends testnet — Windows, 2026-10-18

This version connects separate computers through an encrypted private VPN.
Wallet/Explorer stays on **each computer's loopback address**. Peers exchange
signed public v1 data on a separate authenticated, read-only port. No website is
published to the Internet. This is a transparent test network, one authority,
not a decentralized production cryptocurrency. The existing local network stays
available unchanged.

## What stays online when your PC is off

| Computer | Role | Keys it needs | Can it confirm transfers alone? |
| --- | --- | --- | --- |
| Your PC | Replica + your Wallet | Your spending wallet, in your browser | No |
| Friend 1 | **The single authority** + verifying node | A separate, unfunded authority wallet | Yes, for valid transfers it receives |
| Friend 2 | Replica + their Wallet | Only their own spending wallet | No |

Every node validates signatures, nonces, balances and supply and stores its own
SQLite chain. A replica going offline does not stop the authority and reachable
peers. A returning replica catches up automatically. If the authority is off,
new payments wait pending until it returns. **Do not run two authority processes
or copy its key to every participant.** There is no election, quorum, automatic
failover or Byzantine finality. Multiple replicas are not multiple block producers.
The authority cannot spend other users' coins or mint beyond genesis, but can
censor, halt or sign conflicting histories.

## 1. Get the app and connect the computers

Unzip the generated `ORVEXA-friends-testnet-windows-x64-*.zip` into a new folder on
each Windows x64 computer. It contains `bin/`, the compiled Wallet/Explorer,
scripts and dependency license notices. Recipients do not need Rust, Node.js or
Python. It contains **no wallet, chain database or join credential**. Verify the
SHA-256 against a value obtained from the sender through a trusted channel; a hash
next to an untrusted download alone does not authenticate it.

For a personal, non-commercial test with a few friends, install
[Tailscale](https://tailscale.com/docs/how-to/quickstart), invite the participants
to the same private network, approve their devices and leave it connected.
Use separate user accounts; do not share account passwords. Its current Personal
plan allows up to six users at $0, subject to its terms:
[official pricing](https://tailscale.com/pricing), checked 2026-10-18. This does
not promise free commercial hosting or availability. Your computers, Internet
connections and power still have to work.

Tailscale assigns IPv4 addresses from `100.64.0.0/10`; get each computer's address
with `tailscale ip -4`. Connectivity uses WireGuard encryption even when relayed:
[addresses](https://tailscale.com/docs/concepts/tailscale-ip-addresses),
[connections](https://tailscale.com/docs/reference/connection-types).
The node cannot verify that an address really belongs to an encrypted interface:
**connect the VPN first**. Do not expose the mesh port through a router, public
proxy, Tailscale Funnel or plain unencrypted network. No domain/card/server rental
is needed for this private setup.

The addresses below are **examples to replace with actual Tailscale addresses**:

```text
You:     100.90.0.1
Friend1: 100.90.0.2   authority
Friend2: 100.90.0.3   replica
```

## 2. Friend 1 creates a separate authority key

In PowerShell, inside the unzipped app folder:

```powershell
New-Item -ItemType Directory -Path .\data -Force | Out-Null
.\bin\orvexa-cli.exe wallet-new --wallet .\data\authority.wallet.json
.\bin\orvexa-cli.exe address --wallet .\data\authority.wallet.json
```

Choose a password of at least 12 characters. Friend 1 sends you **only the public
address** printed by the last command. The encrypted file and password stay with
Friend 1. Back up that file and password separately. It receives no genesis funds.
There is no authority key rotation or recovery without the original key in v1.

## 3. You create the network and private join bundle

In your app folder:

```powershell
$authorityAddress = Read-Host 'Paste the public authority address from Friend 1'
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\new-testnet.ps1 -AuthorityAddress $authorityAddress
```

Choose a password for your new funded owner wallet. The script creates a **new**
network in `data/friends-testnet`; it refuses nonempty directories. Supply stays
exactly **1,000,000 test atoms**, allocated to `owner.wallet.json`; fees remain
burned and minimum fee is 1. No faucet, admin balance mutation or later minting.
Existing `data/local-web` wallets and databases are not migrated or reset.

Send `data/friends-testnet/join-network.zip` **privately** to invited operators.
It contains exactly `genesis.json`, `network.json`, `mesh.secret`. Verify the
chain ID through a separate trusted conversation. The access credential is not
a wallet key; it allows reading the private peer API. Anyone with access can
see all transparent chain data. Never send `owner.wallet.json`, your password
or the entire data directory. The script can optionally reuse an existing
encrypted wallet via `-OwnerWallet`, but a fresh test wallet is recommended.

Friends extract the join archive into `data/friends-testnet` under their app
folder. Friend 1 keeps their authority wallet separately at the path above.

## 4. Start the nodes

Friend 1 (the only block producer; password is prompted once, held only in memory):

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-testnet.ps1 -MeshAddress 100.90.0.2 -Peers 'http://100.90.0.1:4243,http://100.90.0.3:4243' -AuthorityWallet .\data\authority.wallet.json -UiPort 4342 -OpenBrowser
```

You:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-testnet.ps1 -MeshAddress 100.90.0.1 -Peers 'http://100.90.0.2:4243,http://100.90.0.3:4243' -UiPort 4342 -OpenBrowser
```

Friend 2:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-testnet.ps1 -MeshAddress 100.90.0.3 -Peers 'http://100.90.0.2:4243,http://100.90.0.1:4243' -UiPort 4342 -OpenBrowser
```

Use all-to-all peers for this small group, maximum eight explicit peers per node.
The script can discover the local Tailscale IPv4 address if `-MeshAddress` is
omitted. Each node uses its own `data/friends-node/node.sqlite`; never share or
sync a live SQLite file between machines. `-NetworkDir` and `-DataDir` allow
different folders. Port 4342 avoids the earlier local demo on 4242–4244.

Open **http://127.0.0.1:4342 on your own computer**. `100.x:4243` is a peer API,
not the website. HTTP on loopback remains a browser secure context for Wasm,
clipboard and local keys; inter-computer traffic relies on VPN encryption.
There is no remote wallet custody endpoint. Mobile/web-only access without a
local node is not delivered in this stage.

Peers pull bounded pages and pending transactions every two seconds by default.
The authority signs nonempty blocks on its five-second production schedule once
caught up. Offline peers/timeouts can increase latency. Empty blocks are not
generated automatically; confirmation counts increase with accepted blocks,
not elapsed time. Keep the terminal, computer and VPN running, with sleep
disabled while operating a node. Windows reboot requires relaunch; the authority
must enter its password again. No plaintext password file or autorun job is installed.

If the firewall blocks peers, an administrator can add a narrow inbound rule
on each computer, replacing the example with **that computer's actual VPN IP**:

```powershell
$meshIp = '100.90.0.2'
$nodeProgram = (Resolve-Path .\bin\orvexa-node.exe).Path
New-NetFirewallRule -DisplayName 'ORVEXA private peers' -Direction Inbound -Action Allow -Protocol TCP -LocalAddress $meshIp -LocalPort 4243 -RemoteAddress 100.64.0.0/10 -Program $nodeProgram -Profile Any
```

In a source checkout use `target/friends/debug/orvexa-node.exe` for the firewall
program path. Limit Tailscale access rules to invited devices and this peer port.
The project does not change firewall/VPN/account settings automatically.

## 5. Check real transfers and an offline computer

1. On your PC, import `data/friends-testnet/owner.wallet.json` in **Wallets →
   Restore wallet** and save an encrypted backup. It shows 1,000,000 test atoms.
2. Friends create their own wallets in **Wallets → Create wallet** and save their
   backups. Those keys stay in their browsers; they begin with zero funds.
3. Friend 2 copies their Receive address to you. Send 250, fee 2. No manual
   **Produce block** is needed: Friend 1's authority confirms it automatically.
   Explorer on each node shows the same actual hash and block. Balances:
   you 999,748, Friend 2 250, burned 2.
4. Turn **your** PC off. Friend 2 can send 40 back to your public address, fee 1,
   while Friend 1's authority is running. A recipient need not be online.
5. Start your node with the same command. It catches up: you 999,788, Friend 2
   209, burned 3. Wallet restoration is unnecessary if browser storage remains;
   the encrypted backup recovers it otherwise.
6. Stop the authority: another valid transfer may enter pending, but no replica
   signs a replacement block. Restart that same authority with its original
   database/key; reachable pending transfers can confirm. If every copy of a
   pending transaction disappears in restarts, inspect Explorer before resubmitting.

Read operational state in another terminal:

```powershell
.\bin\orvexa-cli.exe --node http://127.0.0.1:4342 network
Invoke-RestMethod http://127.0.0.1:4342/v1/status
```

`network` reports actual role, peer reachability, last observed heights,
in-process produced-block count and `halted`. A failed peer is retried; no
automatic admission/discovery or key distribution occurs.
Peer heights retain the highest advertisement observed after genesis checks;
they are not signed checkpoints. Known newer peers pause authority production
until catch-up, even when a subsequent fetch fails. A dishonest invited peer can
stall this small trusted-group setup; investigate before changing the peer list.

## Conflicts, recovery and limits

Automatic peer ingestion compares overlapping blocks. Two different,
authority-signed headers at the same height cause quarantine and write both
blocks to `data/friends-node/evidence/authority-conflict.json`. The existing tip
is preserved; block acceptance stops. This is evidence of conflicting signatures,
not a claim that the foreign branch's entire state is valid. A forged signature
or foreign chain cannot create this evidence. An existing evidence file, including
an incomplete marker, keeps the node halted after restart. Do not delete it or
select a branch automatically: stop and preserve all nodes' databases/evidence
for investigation. Detection depends on observing the conflicting branch.

Back up the authority's **current** database together with its encrypted key;
never run a stale copy concurrently or revert it to produce an alternate history.
There is no separate anti-equivocation signer journal or safe hot standby.
Backup SQLite only while its node is stopped. Replicas can rebuild from the
trusted genesis and an honest available peer. Wallet backups contain no chain
history and the node database contains no user private spending keys.

The shared mesh credential can be rotated by stopping nodes, generating a new
`mesh-token` file and privately redistributing it. This does not rotate the
authority key, change chain ID or erase chain data. OS permissions/backups must
protect credential and authority files. The VPN control plane is an additional
availability/metadata dependency. Peer access is capped at 64 requests/second and
16 in-flight requests per node; this is not public-Internet DoS hardening.

Single authority, transparent addresses/amounts, volatile mempool, full chain in
memory, bounded history and lack of independent audit remain. No real funds,
exchange listing, token price or decentralized/private-network readiness is implied.

## Build and verify from source

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked --target-dir target/friends
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-web.ps1 -InstallTools
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo-testnet.ps1
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1 -SkipBuild -BinaryDir target/friends/debug -BasePort 51242
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-testnet.ps1
```

`target/friends` avoids overwriting an already-running original debug node. The
packager defaults to Release and includes pinned native binaries, existing web
assets and upstream notices. `-Configuration Debug -SkipBuild` packages a prior
debug build. Only the build machine needs Rust/Node.js. The demo uses three real
local OS processes and disposable random passwords retained only in memory;
it does **not** exercise actual different computers, VPN accounts or routers.
The source checkout's `docs/VALIDATION.md` records results and remaining
verification gaps. The bundled app runs without the source checkout.
