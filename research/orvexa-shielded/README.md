# orvexa-shielded — прототип правил приватной v2

Изолированный прототип этапов P2/P3 ([план тестов](../../docs/PRIVACY_TEST_PLAN.md)).
Здесь проверяются правила из [спецификации v2](../../docs/PRIVATE_V2_SPEC.md):
канонический конверт, sighash и txid, проверка без состояния, а также модель
реестра в памяти. Модель хранит дерево commitments, окно anchors, nullifiers
и сожжённые комиссии и применяет блок атомарно.

Это **не узел** и не часть workspace v1: свой `Cargo.toml` с пустым
`[workspace]` и свой `Cargo.lock`. Сети, mempool, консенсуса, хранения на диске
и RPC здесь нет. Вся криптография — upstream `orchard = "=0.16.0"`
(Ironwood v3, `PostNu6_3`).

```bash
cd research/orvexa-shielded
cargo test --locked      # 22 теста с настоящими доказательствами, ≈ 30 с на Apple M4
```

- `src/envelope.rs` — формат конверта, декодер, sighash, txid.
- `src/ledger.rs` — genesis и chain ID, правила транзакции и блока, состояние, replay.
- `src/wallet.rs` — тестовый кошелёк (скан, witness, перевод) и сборщик
  произвольных bundle для атак.
- `tests/ledger.rs` — сценарии P2-01…P2-15 и P3-01…P3-12.
