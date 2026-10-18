# Переименование проекта в ORVEXA

ORVEXA — предварительное рабочее название. Товарные знаки, домены и названия
криптовалютных проектов ещё не проверены. Этот этап меняет название существующего
проекта; задачи нового приватного протокола и консенсуса остаются в дорожной карте.

## Инвентаризация до изменений

Регистронезависимый поиск `rg -n --hidden -i void` по исходному проекту выявил
следующие **27 файлов**. Дополнительный поиск с `--no-ignore`, исключающий `.git`
и `target`, выявил старые демо-данные и логи; они сохраняются без изменений.

| Исходный файл | Новый путь / действие |
| --- | --- |
| Cargo.toml | Переименовать workspace members и внутренние зависимости |
| Cargo.lock | Обновить только имена локальных пакетов и зависимостей |
| ARCHITECTURE.md | Новое название, модули и пояснение совместимости v1 |
| LICENSE | ORVEXA contributors |
| PRIVACY.md | Новое название и путь Rust API |
| README.md | Название, команды запуска и предварительный статус названия |
| ROADMAP.md | Новое название |
| THREAT_MODEL.md | Новое название |
| docs/API.md | Название API, прежние пути/схемы и адреса v1 |
| scripts/demo.ps1 | Новые бинарники и имена функций |
| crates/void-cli/Cargo.toml | crates/orvexa-cli/Cargo.toml |
| crates/void-cli/src/main.rs | crates/orvexa-cli/src/main.rs |
| crates/void-core/Cargo.toml | crates/orvexa-core/Cargo.toml |
| crates/void-core/src/lib.rs | crates/orvexa-core/src/lib.rs |
| crates/void-crypto/Cargo.toml | crates/orvexa-crypto/Cargo.toml |
| crates/void-crypto/src/lib.rs | crates/orvexa-crypto/src/lib.rs |
| crates/void-node/Cargo.toml | crates/orvexa-node/Cargo.toml |
| crates/void-node/src/lib.rs | crates/orvexa-node/src/lib.rs |
| crates/void-node/src/main.rs | crates/orvexa-node/src/main.rs |
| crates/void-rpc/Cargo.toml | crates/orvexa-rpc/Cargo.toml |
| crates/void-rpc/src/lib.rs | crates/orvexa-rpc/src/lib.rs |
| crates/void-tests/Cargo.toml | crates/orvexa-tests/Cargo.toml |
| crates/void-tests/tests/network.rs | crates/orvexa-tests/tests/network.rs |
| crates/void-tests/tests/protocol.rs | crates/orvexa-tests/tests/protocol.rs |
| crates/void-tests/tests/storage_wallet.rs | crates/orvexa-tests/tests/storage_wallet.rs |
| crates/void-wallet/Cargo.toml | crates/orvexa-wallet/Cargo.toml |
| crates/void-wallet/src/lib.rs | crates/orvexa-wallet/src/lib.rs |

`crates/void-tests/src/lib.rs` не содержит старого названия; файл перемещается
в `crates/orvexa-tests/src/lib.rs` без изменения содержимого.
Также обновляется docs/VALIDATION.md и добавляются этот отчёт и тесты совместимости
с фиксированными данными, выпущенными исходной сборкой до переименования.

Перед правками сохранена копия исходных файлов во временном каталоге и SHA-256
всех существующих демо-артефактов. Исходные 25 тестов прошли. Все исходные файлы
в Git пока untracked; этот отчёт сравнивает состояние с сохранённой копией,
а не выдаёт `git diff` за полный список изменений.

## Совместимость

Остаются прежними домены подписей и хешей `VOID/address/v1`, `VOID/genesis/v1`,
`VOID/transaction/v1`, `VOID/tx-id/v1`, `VOID/ordered-transactions/v1`,
`VOID/block-header/v1`, `VOID/block-id/v1` и канонический префикс `void_test_`.
Это идентификаторы протокола, а не текущее название продукта. Изменение префикса
также изменило бы аутентифицируемые метаданные AEAD старых кошельков.

Формат JSON, HTTP-маршруты `/v1`, версия протокола/кошелька/базы, chain ID,
подписи, алгоритмы и параметры шифрования, эмиссия 1 000 000 тестовых атомов,
минимальная комиссия 1 атом и правила сжигания комиссий сохраняются.
В `/v1/status` меняется только отображаемое название сети.

Корневая рабочая папка `C:\Users\waxie\Desktop\void` сохраняет свой путь, чтобы
не перемещать открытый проект. Старые данные и логи `demo-data`, а также
генерируемый Cargo каталог `target` не проходят текстовую замену.

## Итоговые проверки

Все 27 файлов из таблицы изменены по указанным новым путям. Дополнительно:

| Файл | Изменение |
| --- | --- |
| docs/VALIDATION.md | Результаты проверок ORVEXA и сохранённый исходный отчёт |
| docs/RENAME_ORVEXA.md | Этот список, границы переименования и совместимость |
| crates/orvexa-tests/tests/rename_compatibility.rs | Четыре теста совместимости |
| crates/orvexa-tests/tests/fixtures/v1-protocol.json | Контрольные данные исходной сборки до переименования |
| crates/orvexa-tests/tests/fixtures/v1-encrypted-wallet.json | Зашифрованный тестовый кошелёк исходной сборки |
| crates/orvexa-tests/tests/fixtures/v1-node.db | Неизменённая копия публичной базы первого демо |

Итог: 28 существующих файлов с изменённым содержимым, 5 новых файлов и один
файл (`crates/orvexa-tests/src/lib.rs`), перемещённый без изменения содержимого.
Все семь каталогов `crates/void-*` переименованы в `crates/orvexa-*`.
Файлы `.gitignore` и `.github/workflows/ci.yml` не потребовали изменений:
старое название в них отсутствовало, команды CI используют весь workspace.

Успешно выполнены `cargo fmt --all`, `cargo check --workspace --locked`,
`cargo test --workspace --locked` (**29 тестов**),
`cargo clippy --workspace --all-targets --locked -- -D warnings` и
`cargo build --workspace --locked`. Проверены `--help` новых программ.
Три независимых процесса `orvexa-node.exe` прошли демонстрацию перевода,
отклонения неверной подписи/повторного расходования, синхронизации и перезапуска.
Результат: `demo-data/20261017-224113-100ca9db/result.json`.

Совместимость подтверждена данными, созданными до переименования: адресами,
genesis, байтами подписания, подписями, ID транзакции/блока, зашифрованным
кошельком и реальной SQLite-базой прежнего демо. Тесты также фиксируют прежние
экономические константы и JSON-представления. Пароль кошелька фикстуры намеренно
известен тестам; в фикстурах нет пользовательских секретов или реальных средств.

Версии внешних зависимостей не менялись: Cargo.lock совпадает с исходным после
замены только `void-` на `orvexa-` в именах локальных пакетов. SHA-256 всех 36
старых демо-файлов совпали с исходными. Оставшиеся упоминания старого названия
в исходном проекте относятся к формату v1, тестовым данным и этому описанию
переименования; старая папка проекта, исторические логи и кэш Cargo сохранены.
