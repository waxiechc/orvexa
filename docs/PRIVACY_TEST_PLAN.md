# ORVEXA: план тестов конфиденциального протокола

Статус на 2026-10-17: **редакция 2, запланировано, не реализовано**. Это не отчёт
о прохождении тестов приватных переводов. [Сравнение и этапы](PRIVACY_DESIGN.md).
Прежний P1 с адаптером перенесён в P2; state — в P3, wallet — в P4, сеть/Explorer —
в P5, hardening — в P6. Ни один этап реализации пока не согласован.

## P1: upstream evidence в отдельном evaluation workspace

Никаких зависимостей или приёма приватных tx в рабочей v1. На согласованном
стенде используются upstream fixtures, а не самодельный ORVEXA конверт/circuit.
Измерения не объявляются аудитом или доказательством приватности сети.

| ID | Проверка | Приёмка |
| --- | --- | --- |
| P1-01 | Источник, pin, lock/checksums, лицензии графа, MSRV и Windows build | Воспроизводимый manifest/lock и отчёт; нет изменений Cargo.lock v1 |
| P1-02 | Upstream advisories/RustSec, audit scope и diff до выбранного release | Исправления сопоставлены с версией; неизвестный scope явно отмечен; нет fallback на insecure suite |
| P1-03 | Настоящий bundle/proof и authorization upstream candidate | Полная проверка; изменение proof/подписей отвергается, mock-verifier отсутствует |
| P1-04 | Canonical bounds, malformed vectors и suite/circuit variants | Зафиксированы фактические accept/reject и границы API; это ещё не ORVEXA decoder |
| P1-05 | Key generation, proving/verification 2/4/8 actions, scan, bytes и peak RAM | Release, CPU/RAM/OS/features, cold/warm runs и одинаковый объём проверок; согласован desktop budget |
| P1-06 | Monero source/fixtures как сравнительная база | Зафиксированы input/output/ring shape, данные decoy corpus и реально проверяемые APIs; FCMP++ не приписан CLSAG |
| P1-07 | Сопоставление двух путей и решение go/no-go | Пробелы и стоимость адаптации учтены; следующий P2 не начинается автоматически |

**Состояние на 2026-10-18** ([стенд и отчёт](../research/orchard-eval/README.md),
`orchard =0.16.0`, Ironwood v3 / PostNu6_3, Apple M4):
P1-03 выполнен; P1-01 выполнен, кроме запуска на Windows (сборка есть);
P1-05 выполнен для Mac, нет Windows-ПК и Intel; P1-02 — RustSec чист, покрытие
аудитом не установлено; P1-04 — частично (границы builder/verifier, без
сериализации); P1-06 не начат. P1-07: владелец проекта принял `orchard 0.16.0`
(Ironwood v3 / PostNu6_3) для тестовой сети v2 — см. [спецификацию, §10](PRIVATE_V2_SPEC.md). Стенд показал, что повтор nullifier
и отрицательный value balance библиотека принимает — их обязан отвергать узел.

Настоящие proofs в upstream fixtures ещё не проверяют genesis/chain binding,
экономику или double spend ORVEXA. В P1 нет собственного sighash, адаптера,
кошелька, nullifier database, RPC или Explorer. Неисполненная сравнительная
проверка Monero отмечается как неизвестная, а не как преимущество Orchard.

## Проверенная исходная база

В исходном аудите прошли 29 тестов прозрачной v1: криптография 2, storage failure 1,
protocol 12, network 6, wallet/storage 4, compatibility 4. Они проверяют подписи,
сумму/nonce/fee, overflow, pending overspend, corrupt block, SQLite replay и
сохранение исходных форматов. Приватных proofs, nullifier state и seed-based
recovery в этих тестах нет. u64 не принимает отрицательное число в схеме v1,
но отдельного теста отрицательного proof/witness нет. Это не покрытие v2.

После добавления прозрачного Web Wallet набор содержит **32 Rust-теста**, ещё
2 unit-теста точного разбора чисел и 3 браузерных сценария с реальным узлом.
Проверены двухсторонняя Wasm-подпись, backup/restore, Explorer и restart;
это не тесты приватных proofs. Команды и результаты — в
[VALIDATION.md](VALIDATION.md), запуск — в [WEB_WALLET.md](WEB_WALLET.md).

## P2: изолированный адаптер и настоящие proofs

После выбора стека P1 и review спецификации, в новом изолированном
`orvexa-shielded`, без работающего RPC v1. Таблица ниже относится к рекомендованному
Orchard/Ironwood пути; при выборе Monero её нужно пересмотреть целиком, а не
переименовать nullifier в key image. Используются upstream
builder/prover, синтетические ключи и независимые upstream vectors, где доступны.
Proof и подписи настоящие; mock-verifier не допускается. Нельзя ограничиваться
отказом «добропорядочного» builder: атакующий может передать готовые bytes.

| ID | Проверка | Ожидаемый результат |
| --- | --- | --- |
| P2-01 | Валидный Ironwood-v3 bundle, PostNu6_3, известные note/witness и fee | Проверены proof, SpendAuth и binding; результат не раскрывает openings |
| P2-02 | Изменённый либо усечённый proof | Decoder/verifier отказывает без panic |
| P2-03 | Дополнительные байты в proof/envelope | Отказ canonical length/trailing bytes, даже если proof мог бы оставаться математически допустимым |
| P2-04 | Изменить cmx, cv_net, nullifier, rk или anchor | Полная проверка отказывает; point/field encodings проверяет библиотека |
| P2-05 | Подменить SpendAuth либо binding signature | Отказ; одного успешного ZK proof недостаточно |
| P2-06 | Изменить ciphertext, ephemeral key, fee, expiry, chain ID или suite после подписи | Mutation test каждого public field вызывает отказ |
| P2-07 | Исторический insecure circuit, другой pool/version, неподдерживаемые flags | Отказ до verifier; VK не берётся из входа |
| P2-08 | Пустая очередь, нет actions, некорректная/огромная длина | Отказ до дорогой проверки, несмотря на допустимость empty batch upstream |
| P2-09 | Отрицательные/дробные значения, u64::MAX, i64::MAX+1, выход за тестовую эмиссию | Checked Amount/TryFrom отказывает до modular cast; accept/reject пары на границах |
| P2-10 | Валидный библиотечный proof с negative/zero value_balance либо fee mismatch | Ordinary transaction policy отказывает |
| P2-11 | Spends-disabled mint flags либо fee < 1 | Ordinary verifier отказывает; genesis verifier в P2 отсутствует |
| P2-12 | Выходы больше входов минус fee | Builder/binding либо политика конверта отказывает; есть проверка подготовленного атакующим bundle |
| P2-13 | Опубликованный upstream malformed/soundness regression vector | Fixed verifier отвергает; недоступный закрытый exploit не имитируется и отмечается как пробел |
| P2-14 | Debug/error/log capture с помеченными секретами | Нет spending/view keys, note plaintext, masks и witnesses в выводе адаптера |
| P2-15 | Канонические векторы конверта | Anchor связан подписью; изменение любого auth/proof байта меняет full tx ID; неоднозначная кодировка отклоняется |

**Состояние на 2026-10-18.** Правила из [спецификации v2](PRIVATE_V2_SPEC.md)
реализованы в изолированном прототипе [research/orvexa-shielded](../research/orvexa-shielded/).
Выполнены P2-01…P2-08, P2-10…P2-12, P2-15, P3-01…P3-05, P3-07 и P3-12:
22 теста с настоящими доказательствами. Хранение в SQLite: P3-03 на диске,
P3-09, P3-10 (настоящий `abort` процесса внутри записи) и P3-11 (шесть видов
подделки базы) — ещё 6 тестов. Не выполнены P2-09 (только проверки кошелька),
P2-13 (нет публичного вектора exploit), P2-14 и P3-08 (честно непостроимо). Спецификация ждёт review;
в рабочий узел ничего не включено.

`-1 as u64` даёт большое положительное значение, а не отрицательный witness.
Нужно проверять запрещённый пользовательский ввод, границы и настоящие bad-proof
vectors, а не объявлять такую конверсию тестом отрицательной скрытой суммы.
Circuit ради теста не переписывается. Zero-value dummy notes допустимы и не
являются инфляцией.

Один valid bundle может проходить stateless verifier многократно. P2 **не
обещает** chain-level double-spend protection; она появляется в P3.
Приёмка P2: спецификация конверта/genesis и suite прошла review до реализации,
pin/checksums/licenses зафиксированы, применимые тесты исполняются, 29 v1 тестов
и demo сохранены. Метрики P1 повторяются, если адаптер или выбранный stack
существенно меняют измеряемую работу. Чужие benchmark numbers не выдаются за
результаты этого PC.

## P3: state, двойные траты, инфляция и restart

| ID | Атака / сбой | Ожидаемое свойство |
| --- | --- | --- |
| P3-01 | Два valid spends одной note, разные proofs/tx IDs | Второй nullifier отклонён в pool, одном блоке, последовательных блоках |
| P3-02 | Дубликат nullifier внутри bundle/блока | Блок отвергнут атомарно |
| P3-03 | Повтор после restart/resync | Set восстановлен replay, повтор отклонён |
| P3-04 | Корректный proof относительно дерева атакующего | Ledger отвергает неизвестный anchor, несмотря на успешный stateless verifier |
| P3-05 | Чужая ветка, stale root, cross-chain context | Отказ по закреплённой политике сети/anchors |
| P3-06 | Лишние outputs, fee mismatch, negative value balance, повтор mint | Нет новых commitments/nullifiers/burn в принятом state |
| P3-07 | Изменение supply, bootstrap digest, genesis proof/auth | Genesis отвергнут; 1 000 000 только через единственную разрешённую bootstrap-операцию |
| P3-08 | Burn overflow/underflow, fee > remaining supply | Checked arithmetic отказывает, SQL transaction не публикуется |
| P3-09 | Сбой на записи блока/notes/nullifiers/root до commit | Полный rollback и один согласованный tip после restart |
| P3-10 | Сбой после commit, до смены памяти | Replay восстанавливает новый state |
| P3-11 | Повреждён proof, блок, snapshot, nullifier set либо tip | Replay/consistency check обнаруживает повреждение; snapshot не источник доверия |
| P3-12 | Частично невалидный блок после валидных tx | Ни один эффект блока не принят |
| P3-13 | Дубликат nullifier dummy action | Нет unchecked обхода через dummy flag; уникальность применяется ко всем опубликованным nullifiers |

В тестах известны openings: сумма unspent test notes плюс burned fee равна supply
после каждой последовательности. Узел эти openings не получает. Property tests
сравнивают принятые операции с независимой простой моделью тестовых сумм.

## P4: получатель, backup и локальные секреты

| ID | Сценарий | Ожидаемое свойство |
| --- | --- | --- |
| P4-01 | Несколько платежей через разные diversifiers | Получатель обнаруживает notes, static address не публикуется |
| P4-02 | Неверный IVK, ciphertext tamper, чужой receiver | Неверная note не зачисляется |
| P4-03 | Backup seed/key container → пустой cache → полный rescan | Совпадают incoming, spent nullifiers, баланс и доступная outgoing history |
| P4-04 | Раскрыт IVK / FVK / OVK | Подтверждается ровно заявленный объём раскрытия; privacy от владельца viewing key не обещается |
| P4-05 | Wrong password, AAD tamper, truncated/oversized wallet | Отказ без потери единственной исправной копии |
| P4-06 | Сбой до/после temp sync и atomic publication | Есть хотя бы одна проверяемая encrypted copy на Windows/Linux |
| P4-07 | CLI argv, logs/errors, node requests и backups | Нет seed, plaintext notes, masks/view keys вне разрешённого локального отображения |
| P4-08 | Потеря pending, повторный rescan, разные wallet caches | Нет двойного зачисления; pending отдельно от confirmed |

Scan перебирает outputs и восстанавливает witness/notes, а не запрашивает у узла
открытый баланс. Тест с помеченными секретами не доказывает отсутствие всех
side channels и не заменяет анализ хранения и утечек.

## P5/P6: сеть, Explorer, metadata и независимый review

- Три узла одинаково принимают valid block и отвергают bad-proof/state cases.
- Alice -> Bob -> Charlie через разные кошельки; restart/rescan сохраняет spent
  balance и включение tx. Это тест интеграции, не измерение анонимности.
- Explorer индексирует только проверенные блоки, ищет height/hash/tx ID, показывает
  fee/размер/actions/roots и inclusion; нет адресного balance/history, view keys,
  plaintext amount или выдуманного sender/receiver.
- Индекс Explorer перестраивается из цепи и возобновляется после сбоя; другой
  chain ID и повреждённый checkpoint отвергаются. Число подтверждений не
  выдаётся за Byzantine finality.
- Access logs и запросы tx ID Explorer учитываются как возможный источник связи
  посетителя с платежом; сторонние trackers не нужны локальному MVP.
- Header/tx ID связывают proofs, signatures, ciphertexts, порядок и state roots;
  wrong authority/parent/height и смешанные v1/v2 отвергаются.
- Конфликтующие подписанные headers сохраняются и перепроверяются после restart.
  Это свидетельство equivocation, не решение Byzantine finality.
- Network capture подтверждает отсутствие адресных balance/history-запросов и
  передачи viewing keys. Оставшиеся timing/size/fee leaks документируются.
- Dump блоков не содержит открытых сумм/static addresses; отдельно анализируются
  вывод из side information и singleton/маленький anonymity set.
- Fuzz parsers: points/fields, flags, bounded lengths, trailing bytes, proof sizes,
  empty queues; проверяются лимиты ресурсов до дорогого verifier.
- Внешний review охватывает circuit selection, auth digest, genesis, nullifiers,
  conservation, crash consistency и recovery. Тесты его не заменяют.

## Проверки после каждого согласованного этапа

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo.ps1 -SkipBuild
```

Release-оценка P1 фиксирует характеристики машины, время и память. При смене
утверждённого стека требуется review; proof нельзя подменить заглушкой.
