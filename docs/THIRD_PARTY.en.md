# Third-party software notices

ORVEXA source is MIT licensed; see the root LICENSE. Generated Windows bundles
include `third-party-licenses/` with copied upstream copyright/license files and
an index of the exact installed Cargo/npm package versions. It deliberately
includes build/test dependencies as well as runtime dependencies. Rebuilds use
Cargo.lock and web/package-lock.json; no claim that every indexed dependency is
linked into each binary is made.

Native P2P uses pinned libp2p 0.56.0 (MIT), Noise and Yamux through upstream
libraries. Its runtime requires no external service/account. The collector uses
the enabled Cargo graph (Windows x64 plus Wasm for the Windows bundles, including
build/tests), excluding unselected optional protocols. The Desktop host uses
Wry/Tao and the system Microsoft WebView2 runtime. The runtime is installed
separately; the EXE embeds the application assets and collected license texts.
Published crates sometimes omit license files. Pinned
`docs/third-party-notices/` restores root notices from exact upstream commits;
`source.json` records URLs/hashes and collection verifies them. For packages
whose exact source root supplies no standalone notice (base256emoji, base45,
if-watch), preserve the published Cargo declaration/authors and a clearly marked
SPDX MIT reference with original placeholders; no copyright author is invented.
Yamux 0.13.10 uses its maintained repository's release tag commit, with its full
package manifest verified identical to the published crate. Notice maintenance
is build-time only, never a node runtime network dependency.

Key existing dependencies include RustCrypto Argon2/XChaCha20-Poly1305,
ed25519-dalek, Tokio, Axum, Reqwest, SQLite/rusqlite, wasm-bindgen, React, Vite,
Lucide, qrcode and json-bigint. `subtle` 2.6.1 provides credential comparison;
it is not a new transaction primitive. SQLite itself is public domain; its Rust
bindings have their own licenses. Notices retain upstream terms.

The older, optional friends profile uses Tailscale, installed separately, not
bundled or modified. The native P2P profile does not use it. Its service has its
own account, license and plan terms. Official references checked 2026-10-18:
[pricing](https://tailscale.com/pricing),
[connection encryption](https://tailscale.com/docs/reference/connection-types).
The Personal plan is intended for personal/non-commercial use; verify eligibility
before using this setup commercially. A VPN does not make transparent v1 payments
anonymous or add consensus finality.
