# orvexa-shielded — прототип правил приватной v2

Изолированный прототип этапов P2/P3 ([план тестов](../../docs/PRIVACY_TEST_PLAN.md)).
Здесь проверяются правила из [спецификации v2](../../docs/PRIVATE_V2_SPEC.md):
канонический конверт, sighash и txid, проверка без состояния, а также модель
реестра. Модель хранит дерево commitments, окно anchors, nullifiers
и сожжённые комиссии, применяет блок атомарно и сохраняет его в SQLite одной
транзакцией.

Это не часть workspace v1: свой `Cargo.toml` с пустым `[workspace]` и свой
`Cargo.lock`. Здесь есть локальный тестовый узел v2 с API и обозревателем
(производитель блоков или ведомый узел), но нет P2P v1 и консенсуса из нескольких валидаторов. Вся криптография — upstream `orchard = "=0.16.0"`
(Ironwood v3, `PostNu6_3`).

```bash
cd research/orvexa-shielded
cargo test --locked      # 47 тестов с настоящими доказательствами, ≈ 75 с на Apple M4
```

Локальная тестовая сеть вручную:

```bash
cargo build --release --locked
# адрес получателя всей тестовой эмиссии — 43 байта raw Orchard address в hex
./target/release/orvexa-v2-node genesis --to <address-hex> --out genesis.bin
./target/release/orvexa-v2-node run --data-dir v2-producer --genesis genesis.bin --produce-every-ms 2000
./target/release/orvexa-v2-node run --data-dir v2-follower --genesis genesis.bin \
    --listen 127.0.0.1:4261 --follow http://127.0.0.1:4260
# обозреватель: http://127.0.0.1:4260/
```

- `src/envelope.rs` — формат конверта, декодер, sighash, txid.
- `src/ledger.rs` — genesis и chain ID, правила транзакции и блока, состояние, replay.
- `src/store.rs` — SQLite: атомарная запись блока, пересчёт и сверка при открытии.
- `src/seed_wallet.rs` — BIP39 (24 слова) → ZIP 32, собственное дерево, скан, сдача, ожидающие траты; режимы трата / полный просмотр / только входящие.
- `src/disclosure.rs` — квитанция одного платежа, ключи просмотра (FVK/IVK), согласие на публичный платёж.
- `src/seed_file.rs` — зашифрованный файл seed (Argon2id + XChaCha20-Poly1305), атомарная запись.
- `src/node.rs`, `src/api.rs`, `src/explorer.html`, `src/bin/orvexa-v2-node.rs` — узел, JSON-API, страница обозревателя, CLI.
- `src/client.rs` — клиент API для кошелька: скан блоков, отправка, поиск по txid.
- `src/wallet.rs` — тестовый кошелёк (скан, witness, перевод) и сборщик
  произвольных bundle для атак.
- `tests/ledger.rs` — сценарии P2-01…P2-15 и P3-01…P3-12.
- `tests/seed_wallet.rs` — P4: векторы BIP39, diversified-адреса, восстановление и пересканирование, файл, секреты, pending.
- `tests/disclosure.rs` — проверка и подделка квитанций, охват ключей просмотра, публичный платёж.
- `tests/acceptance.rs` — этап 7 на настоящих процессах: A → B → C и обратно, перезапуск, восстановление, приватность API и логов.
- `tests/storage.rs` — перезапуск, аварии процесса до и после COMMIT, подделка базы.
