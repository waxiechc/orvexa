# Contributing

ORVEXA is an experimental transparent testnet. Discuss protocol, monetary,
privacy and validator-set changes before implementation. Preserve v1 fixtures
and distinguish delivered behavior from proposals.

Include a concrete failure, a coherent change and relevant validation. Never
commit real wallet files, keys, passwords, peer identities, databases or private
network credentials. Fixed vectors must be clearly identified as public tests.
Report security issues using SECURITY.md.

Run `cargo fmt --all -- --check`, `cargo check --workspace --locked`,
`cargo test --workspace --locked` and
`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`.
For UI, build the pinned Wasm/web app, run unit/browser tests, inspect desktop
and mobile, and preserve the approved logo/navy palette.

No custom crypto primitives or privacy claims for v1. Rewards/fees/issuance are
consensus rules, not UI balances. Contributions use MIT; preserve upstream notices.
