# ORVEXA threat model

This v0.1 prototype is transparent and unaudited. Use test atoms only.

| Threat | Current control | Remaining limitation |
| --- | --- | --- |
| Chain observer | None for transaction privacy | Addresses, sender links, amount, fee, nonce and history are public |
| Network observer | Numeric loopback binding; no telemetry | A local process can correlate HTTP requests, timing and wallet lookups; no Tor/I2P or Dandelion++ |
| Key file theft | Argon2id + XChaCha20-Poly1305; random salt/nonce; authenticated public metadata | Weak passwords permit offline guessing; inherited filesystem permissions and backups need OS protection |
| Compromised device | Secrets never sent to the node; sensitive buffers zeroized where feasible | Malware, terminal capture, swap and crash dumps can expose unlocked keys; zeroization is not a hardware boundary |
| Replay/double spend | Signed chain ID and exact account nonce; ordered pending projection | Authority can equivocate; alternate histories do not have Byzantine finality |
| Minting / overflow | Fixed genesis rules, checked arithmetic, burn accounting | No external audit or formal verification |
| Malicious authority | All signatures and accounting independently verified | Censorship, halt and conflicting valid chains remain possible; no automatic recovery |
| Disk corruption / crash | Transactional SQLite journal, full sync, complete replay on startup | Storage hardware/OS failure and rollback to an older valid database are not detected externally |
| Untrusted RPC / peer | Input bounds, timeouts, loopback restrictions, local validation of blocks | Local users can spam the API, fill the pending pool, submit already signed data or observe all balances; no authentication/rate limiting |
| Browser wallet | Opt-in same-origin host; exact numeric Host/Origin, fetch-site checks, restrictive CSP, no framing/referrer/cache; Rust signing in temporary Wasm workers | Served code, extensions and the local validating node are trusted; origin checks do not authenticate local programs or provide light-client validation |
| Friends-testnet transport | Explicit VPN address binding, pinned genesis, shared peer credential compared with `subtle`, no redirects/proxies/DNS; read-only peer surface, request/concurrency caps | Requires an actually encrypted VPN; address range is not encryption. Operators and VPN control plane can observe metadata; this is not public Internet DoS hardening |
| Native P2P transport | TCP/Noise with separate PeerId, numeric addresses, pinned genesis, signed tip claims, bounded messages/streams/connections, verified append-only blocks and persisted conflict quarantine | Peers see IPs, timings and public transfers. No Sybil/eclipse resistance, automatic NAT traversal, peer-store recovery or demonstrated Internet DoS resilience. A malicious authority can still halt/equivocate; an unsigned advertised height alone cannot stall production |

Genesis pins the network and authority. There is no key rotation, hidden fee,
remote configuration update, arbitrary balance mutation or post-genesis mint.
The authority key and spending key should be separate in a realistic test setup.

Wallet files contain version, public address, fixed KDF parameters, salt, nonce
and ciphertext. AEAD authenticates metadata; decrypted key must match the public
address. Files are created without overwriting existing files. No mnemonic or
seed-export endpoint is implemented; back up the encrypted wallet and remember
the password. There is no password recovery service.

The optional English browser wallet stores encrypted v1 containers and public
labels/addresses in origin-local storage. Restore authenticates before saving;
native/browser encrypted backups interoperate. Only signed data is posted to the
node. Password fields are cleared and workers terminated after use; JavaScript,
browser and OS copies cannot be reliably erased. Browser storage is not a backup.
The first validated genesis is pinned; changing it requires explicit adoption in
Settings. The default CLI-only router still rejects every browser Origin; only
`--web-dir` enables the exact same-origin UI policy. A remote UI server is not supported.
Explorer and address lookups expose public v1 payment metadata, including query
timing, to the local node. History is bounded, pending transfers are volatile,
and confirmation counts do not confer Byzantine finality. See
[WEB_WALLET.md](docs/WEB_WALLET.md) for operation and limits.

Privacy requires a new transaction/state design. Encrypting wallet files does not
hide transactions. See [PRIVACY.md](PRIVACY.md) for the research decision and gate.

## Privacy design review (proposal only)

[PRIVACY_DESIGN.md](docs/PRIVACY_DESIGN.md), revision 2, records the current audit,
public-data leaks, Monero/Orchard tradeoffs and the proposed shielded-note v2
threat model. A new chain inherits neither upstream users nor its anonymity set;
public Explorer access can itself reveal metadata. No private
mechanism has been activated. Every v1 balance is reconstructible from genesis
and public transfers even if the balance endpoint were removed. Repeated sender
keys/nonces, address reuse, public fees and observation timing correlate payments.

Additional audit limits: the encrypted wallet exposes its public address; CLI
payment arguments reveal recipient/amount to shell history and process observers.
The file writer uses create_new followed by write/sync, so interrupted creation
can leave a partial file; atomic backup publication is not implemented. Seed-based
recovery is absent. Legacy manual sync does not retain authority-equivocation evidence.
These are tracked explicitly; none is fixed by hiding amounts alone.

The friends-testnet profile now records **observed** conflicting authority-signed
headers during automatic peer ingestion and quarantines the node without choosing
a branch. An evidence marker survives restart; missing observations remain a
limitation. Legacy manual sync does not persist this evidence. There is no
separate durable signer anti-equivocation journal, key rotation, election or safe
standby. Only one authority process/key/database lineage may operate; reverting
or cloning that database can fork the chain. Automatic authority mode refuses the
funded genesis key. Authority/spending keys remain separate; running nodes retains
the unlocked authority key in memory until shutdown. No password file, service
credential or autostart job is installed. See [operator guide](docs/FRIENDS_TESTNET.md).
An invited peer advertising a newer chain can stall automatic production until
catch-up; this favors avoiding stale-prefix signing over Byzantine liveness.

The proposed v2 must address proof/circuit soundness, binding signatures, known
anchors, nullifier uniqueness, encrypted local notes and rescan. Fees, action
counts and network timing remain visible. Small anonymity sets, malware, viewing
key disclosure and an equivocating authority remain threats. Existing library
audits do not audit ORVEXA or guarantee a later circuit version; external review
is required before considering real funds.
