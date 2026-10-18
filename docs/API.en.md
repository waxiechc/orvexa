# ORVEXA v1 loopback HTTP JSON API

ORVEXA is the provisional product name. The rename only updates the human-readable
network label in `/v1/status`; HTTP routes, schemas and protocol version remain
unchanged. Canonical v1 addresses retain the legacy `void_test_` prefix so existing
transactions, genesis files and encrypted wallet metadata remain compatible.

Origin: `http://127.0.0.1:4242`. No JSON-RPC envelope; responses are ordinary JSON.
The default client and wallet listener accept only numeric loopback addresses.
That local API has no authentication and is intended for a trusted local user.
By default browser Origin headers are rejected. Opt-in `run --web-dir web/dist`
serves Wallet/Explorer on the node's exact numeric loopback origin, accepting
only that Origin. Host/fetch-site checks and strict CSP apply. No permissive
CORS or key/password endpoints are added.

| Method | Path | Request | Response |
| --- | --- | --- | --- |
| GET | /v1/status | none | protocol, network, privacy, consensus, chain_id, height, tip, burned_fees, pending |
| GET | /v1/runtime | none | enabled, role, halted, peers [{url,state,height}], produced_blocks |
| GET | /v1/genesis | none | immutable Genesis |
| GET | /v1/balance/{address} | checksummed test address | address, confirmed: {balance, nonce}, pending_nonce |
| GET | /v1/history/{address} | address | newest 256 confirmed entries; {height, id, transaction}; excludes genesis allocation |
| GET | /v1/mempool | none | ordered array of SignedTransaction; maximum 512 |
| POST | /v1/transactions | SignedTransaction | {id} on acceptance to volatile pending pool |
| GET | /v1/block-template | none | unsigned Block with first 128 pending transactions |
| POST | /v1/blocks | signed Block | {id} after validation and durable SQLite commit |
| GET | /v1/blocks?from=1 | inclusive height, default 1 | up to 8 signed blocks |
| GET | /v1/transactions/{id} | canonical transaction hash | TransactionDetails; HTTP 404 when unknown |
| GET | /v1/blocks/{height} | positive block height | real ID, ordered transaction_ids, confirmations, full block |
| GET | /v1/explorer/blocks?before=9 | optional exclusive upper height | newest-first page of up to 8 BlockDetails |
| GET | /v1/explorer/mempool | none | pending TransactionDetails with actual IDs |
| GET | /v1/ui-state?address=... | optional v1 address | status, genesis, recent blocks, pending, optional balance/history; one lock |

TransactionDetails is `{id,status,block_height,block_id,confirmations,transaction}`.
Pending has no block and zero confirmations; confirmed has its actual accepted
block and `tip_height - block_height + 1` confirmations, not Byzantine finality.
Unknown/dropped pending transactions return 404. Genesis is `/v1/genesis`, not
a signed block at `/v1/blocks/0`. History remains the latest 256 confirmed entries.

All amounts/nonces/heights are unsigned integers; use a lossless u64 JSON parser
in future JavaScript SDKs. IDs are 64 lowercase hex characters, signatures 128.
The delivered browser adapter uses exact decimal strings/BigInt, including u64
values beyond JavaScript Number precision.
Private keys, passwords and seed material are never valid API parameters.

SignedTransaction:

```json
{
  "transaction": {
    "version": 1,
    "chain_id": "<64 lowercase hex characters from genesis>",
    "sender": "<void_test_ address>",
    "recipient": "<void_test_ address>",
    "amount": 250,
    "fee": 2,
    "nonce": 0
  },
  "signature": "<128 lowercase hex characters>"
}
```

Placeholders above are schema illustrations, not valid transaction data. Use the
Rust wallet to sign canonical bytes; signing serialized JSON will not verify.
Block is `{header, transactions, signature}`; header is `{version, chain_id,
height, parent, timestamp, transactions_hash}`. Timestamp is a strictly increasing
logical counter, not wall-clock time. See ARCHITECTURE.md for signing encodings.

Validation/storage failures currently return HTTP 400 and `{ "error": "..." }`.
Malformed JSON/schema errors use Axum's HTTP 400/422 text responses; oversized
requests use HTTP 413, browser-origin requests HTTP 403, missing paths HTTP 404.
Maximum request and client response size is 2 MiB; client timeout is 10 seconds,
redirects and environment proxies are disabled. The history endpoint has no
pagination yet. HTTP connections have no production-grade rate limits/timeouts
on the server side. Peer sync is partial-progress: blocks already durably accepted
remain accepted if a later block or pending item is invalid.

See [WEB_WALLET.md](WEB_WALLET.md) for browser setup, local signing,
native-compatible encrypted backups and the two-wallet acceptance scenario.

## Opt-in friends-testnet peer API

`run --mesh-listen <Tailscale-IP>:4243 --mesh-token-file <file> --expect-chain <id>`
adds a separate authenticated **read-only** listener. Only GET `/v1/status`,
`/v1/genesis`, `/v1/blocks?from=...` and `/v1/mempool` are available. No wallet,
balance, block template, UI or mutation endpoints are exposed there. Every
request needs `Authorization: Bearer <mesh credential>`; never put it in a URL.
Origin headers are rejected even with a valid credential. Bad/missing credentials
return 401; unavailable routes 404; disallowed methods 405; request/concurrency
limits 429 (64 requests/second, 16 in flight). Native peer clients use a 3-second
overall/2-second connect timeout and the same 2 MiB response bound.

Peer addresses must be numeric Tailscale addresses or loopback for local tests;
arbitrary remote hosts, DNS, LAN/public/wildcard addresses and redirects/proxies
remain forbidden. HTTP here requires an actually encrypted private VPN; the
server cannot infer encryption from an IP range. Default `Client::new` is unchanged;
`Client::new_mesh` is explicit opt-in. Local Wallet/Explorer remains loopback-only.

Runtime `role` is authority/replica in mesh mode; `enabled=false` in default mode.
Peer height is the highest advertised height observed after checking its genesis;
it is an observation, not a signed/finalized checkpoint. Production pauses until
caught up with known newer peers, including after a failed block fetch. A dishonest
invited peer can therefore stall production; remove it only after reviewing the
network/evidence. Runtime counters reset on restart. Observed conflicting authority
signatures set `halted=true` and retain evidence; submit/template/commit then reject
with HTTP 400. There is no automatic fork selection or authority election.
[Setup and operational limits](FRIENDS_TESTNET.md).
