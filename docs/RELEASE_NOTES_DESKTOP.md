# ORVEXA Desktop preview 1

A portable Windows x64 application with a local encrypted wallet, real block
explorer and supervised Rust P2P full node. The English interface, signing Wasm
and open-source notices are embedded in the EXE. Microsoft WebView2 runtime is
required; Rust, Node.js, VPN and a cloud account are not needed to run it.

Download **ORVEXA.exe**, or the complete ZIP with instructions and notices.
Use **SHA256SUMS.txt** to verify the downloaded files:

```powershell
Get-FileHash .\ORVEXA.exe -Algorithm SHA256
.\ORVEXA.exe
```

## What works

- Create and back up an encrypted wallet; sign transfers locally using Rust Wasm.
- Import the same public genesis and network manifest, independently check the
  chain ID and press **Start node**. Verify, store and relay real blocks and
  transactions over native TCP/Noise P2P.
- Inspect balances, transaction history, actual hashes, blocks and confirmations.
- Stop, restart or resume the same node profile with its saved chain and identity.
- Use Wallet and Explorer while the embedded node runs in the app background.
  Keep the app open: closing this preview stops its node.

## Current boundary

**Research software: transparent v1 TESTNET, no real funds.** Addresses, amounts
and sender links are public. A configured single authority produces blocks.
A full node does not automatically become a block-producing validator or earn
rewards. The public network is not live, and this release supplies no hosted
seed or common network files. Obtain verified public configuration from a
network operator. No automatic authority replacement is implemented.

V1 recovery uses an encrypted JSON backup and its password, not a seed phrase.
Private balances/payments with optional disclosure, seed recovery, validator
quorum and rewards remain planned. No independent audit or signed installer is
claimed. ORVEXA is a provisional name.

## Validation

Windows: formatting, workspace check, Clippy with warnings denied, **48 Rust
tests**, **2 adapter tests** and **5 end-to-end tests** passed. These include real
P2P transfers and persistence, two-way wallet transfers, actual native WebView2
wallet creation, and an extracted-release smoke check. The existing 100-node
test uses local TCP processes/tasks, not 100 independent Internet operators.

Read [the Desktop guide](https://github.com/waxiechc/orvexa/blob/main/docs/DESKTOP_APP.md),
[network operator guide](https://github.com/waxiechc/orvexa/blob/main/docs/NATIVE_P2P.md)
and [staged implementation plan](https://github.com/waxiechc/orvexa/blob/main/docs/PLATFORM_PLAN.md).
