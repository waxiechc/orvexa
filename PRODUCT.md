# ORVEXA

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

The founder and invited testers running Rust nodes on Windows, locally or across
native peer connections. They send test atoms, inspect inclusion and restart safely.

## Product Purpose

A working local Wallet and Explorer for the existing ORVEXA v1 chain. Real node
data and explicit pending/confirmed states; no invented balances or history.
The long-term goal is private transfers and balances by default with explicit
public payments and selective disclosure. The staged plan is docs/PLATFORM_PLAN.md.
Current Windows desktop preview adds a native window and background full-node
control without changing v1. Unknown/stale node observations remain explicit.

## Capabilities and Constraints

Transparent account ledger, integer test atoms, fixed genesis allocation and
burned fees. A configured authority signs blocks. No faucet or automatic mining.
Existing v1 keys, canonical transactions and storage remain compatible. Privacy
v2 is unimplemented and requires separate design approval. No real funds.
User spending passwords and keys stay in the browser. Reuse Rust crypto through WebAssembly.
Encrypted file backups are supported; seed phrases are not available in v1.
An opt-in friends profile uses automatic replication and one separate, unfunded
authority key unlocked locally in the node operator's process. It provides no
election, quorum, automatic failover or public remote wallet hosting. Observed
authority conflicts quarantine nodes; the owner's computer can be offline while
the assigned authority and other reachable nodes continue. This does not activate v2.

## Brand Commitments

Preserve the eye/flower mark and ORVEXA wordmark in ORVEXA.png. The user specified
the navy/obsidian palette, restrained geometry and white logo, with no purple,
red, neon or decorative glow. English UI was explicitly confirmed.

## Evidence on Hand

Rust workspace, HTTP API, existing protocol fixtures and three-process demo.
ORVEXA.png is a raster reference, not a vector master. The two attached images
are visual references; their promotional text is not product truth.

## Product Principles

- The wallet signs locally; the node validates independently.
- Report capabilities and failures truthfully.
- A block inclusion is not Byzantine finality.
- Keep backups portable between the native CLI and browser.
