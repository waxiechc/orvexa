# ORVEXA: дизайн конфиденциального протокола v2

Дополнение требований 2026-10-18: private по умолчанию, отдельный явный public
режим и выборочное раскрытие платежа/просмотра баланса. Это не переключатель,
скрывающий опубликованную v1 историю. Viewing keys, полнота balance disclosure,
неотзываемость раскрытого и миграция перечислены в [PLATFORM_PLAN.md](PLATFORM_PLAN.md).
Продолжение разработки согласовано по этапам; точный приватный стек всё ещё
требует upstream evidence до включения в работающий узел.

Дата исследования: 2026-10-17. **Редакция 2: пересмотр выбора стека по запросу
основателя; рекомендация, не разрешение на внедрение.** Работающая v1 остаётся
прозрачной. Этот документ описывает предложение; приватные этапы не внедрены.
ORVEXA — предварительное название.

**Текущий статус интерфейса:** отдельно реализованы English Wallet и Explorer
для прозрачной v1, Rust/Wasm-подпись и совместимые зашифрованные backup-файлы.
Это не реализация P1–P6 ниже. Публичные адреса, суммы, комиссии и связи v1
по-прежнему видны; запросы баланса/истории раскрывают локальному узлу интерес к
адресу. `TransparentV1Adapter` изолирует эти возможности от UI; приватному ledger
нужен отдельный адаптер с локальным сканированием и собственной моделью
восстановления. См. [работающий сценарий v1](WEB_WALLET.md).

**Рекомендация для ORVEXA:** продолжать проектирование отдельной shielded-note
сети на согласованном стеке семейства Orchard, рассматривая Ironwood-v3 как
кандидата для проверки. Причина — скрытая принадлежность входа общему дереву
лучше соответствует цели скрывать отправителя, чем выбор публичного кольца
приманок в небольшой новой сети. Это инженерный вывод для ORVEXA, а не оценка
«Monero небезопасен» и не утверждение, что новый проект унаследует приватность
или безопасность действующей Zcash.

Прежнее предложение сразу добавить `orvexa-shielded` с собственным конвертом,
sighash и verifier **снято с первого этапа**. Сначала нужен отдельный стенд
upstream и проверка лицензий, аудитов и затрат. `orchard = "=0.16.0"` —
исследованный кандидат, а не принятая зависимость. Собственный формат, genesis и
активация требуют следующих отдельных решений. Оба варианта подробно сравнены
по семи критериям в разделе 3; кошелёк и Explorer входят в план, разделы 7–9.

## 1. Аудит фактической v1

| Компонент | Что установлено по коду | Последствие для v2 |
| --- | --- | --- |
| orvexa-core::Ledger | `BTreeMap<Address, Account { balance: u64, nonce: u64 }>`; это счета, не UTXO | Нужна другая модель: дерево commitments и множество потраченных nullifiers |
| Transaction | Открытые sender, recipient, amount, fee, nonce, chain_id, version | Нельзя спрятать граф платежей заменой одного amount |
| Подписи | Ed25519 через `verify_strict`; канонические байты с domain separation и chain ID | Сохранить v1; в v2 владение notes и binding проверяются средствами выбранного протокола |
| Блоки | Подписанный авторитетом header, parent, height, логическое время, ordered tx commitment | Сохранить принцип полной проверки; новый формат должен связывать также весь proof/auth payload и состояние v2 |
| Двойной расход | Точный nonce и достаточный баланс; pending pool проверяется последовательно | В v2 нужны уникальные nullifiers в блоке, цепи и pending pool |
| Экономика | Genesis 1 000 000 атомов; fee >= 1, сжигание, эмиссии после genesis нет; checked arithmetic | Сохранить эти тестовые правила; доказать скрытое сохранение стоимости |
| Консенсус | Один явно заданный Ed25519-авторитет, только append к текущему tip | Нет Byzantine finality: авторитет может остановить сеть, цензурировать и подписать конфликтующие ветки |
| SQLite | `genesis` и `blocks`; `user_version=1`; полный replay при open; commit до смены памяти, exclusive lock, synchronous FULL | Можно сохранить транзакционную модель, но новая БД должна атомарно хранить дерево, nullifiers и tip |
| Конфликты | Чужая ветка отвергается; постоянного хранилища доказательств equivocation нет | Отдельная обязательная задача до расширения сети; приватность не устраняет эту угрозу |
| Кошелёк | OS CSPRNG; Argon2id v19, 64 MiB / 3 / 1; XChaCha20-Poly1305; AAD включает публичные metadata | Можно сохранить криптографию контейнера, с отдельной версией формата и новыми секретами |
| Восстановление | Зашифрованный файл + пароль; mnemonic/master-seed recovery нет | Не выдавать существующий backup за seed recovery; v2 нужен проверяемый rescan из резервной копии ключевого материала |
| Запись кошелька | `create_new` защищает от перезаписи, затем write + sync | Это не атомарная публикация: сбой может оставить частичный файл; исправить до кошелька v2 |
| HTTP/peer | Loopback, ограничения размера, ручной pull sync, без аутентификации и сетевой анонимизации | Сохранить v1 API, сделать отдельный интерфейс v2 без запросов баланса по адресу |

Исследованные реализации: [core](../crates/orvexa-core/src/lib.rs),
[crypto](../crates/orvexa-crypto/src/lib.rs),
[node](../crates/orvexa-node/src/lib.rs),
[wallet](../crates/orvexa-wallet/src/lib.rs),
[RPC](../crates/orvexa-rpc/src/lib.rs), [CLI](../crates/orvexa-cli/src/main.rs).
Это аудит исходного прототипа, не независимый криптографический аудит.

### Что видно и что можно восстановить сейчас

- Genesis раскрывает получателя и всю исходную сумму. Каждая транзакция раскрывает
  оба адреса, amount, fee и nonce; балансы всех адресов восстанавливаются из блоков.
  Отсутствие balance endpoint ничего бы не изменило.
- Один sender и последовательный nonce связывают расходы. Повторное использование
  адресов связывает поступления. Уникальные суммы, комиссии, порядок и момент
  распространения дают дополнительные признаки.
- `/v1/blocks`, `/v1/mempool`, `/v1/history/{address}`, `/v1/balance/{address}` и
  SQLite раскрывают данные. Логическое время блока не является стенными часами,
  но наблюдатель самостоятельно записывает время получения.
- В файле кошелька открыт адрес, хотя spend key зашифрован. CLI `send --to ...
  --amount ...` оставляет сведения о платеже в argv/истории оболочки. Это не утечка
  seed, но такой интерфейс недостаточен для будущего приватного кошелька.
- Loopback ограничивает доступ из сети, но локальный процесс/узел видит запросы
  адресов и момент отправки. Tor/I2P, Dandelion++, traffic padding не реализованы.

## 2. Модель угроз и границы обещаний

Цель v2 после реализации и проверки: наблюдатель цепи не видит значения notes,
статический адрес получателя, исходный commitment реального входа и полный
баланс кошелька. Владелец вычисляет свой баланс локально из расшифрованных notes
и их nullifiers. Приватность рассматривается при условии корректности выбранной
системы доказательств, генерации случайности и клиентского ПО.

| Противник | Требуемая защита | Что остаётся |
| --- | --- | --- |
| Пассивный наблюдатель блоков | Скрытые суммы/получатели, proof membership без открытого input reference | Публичны fee, число actions, размер, anchor, nullifiers, commitments и время наблюдения |
| Отправитель или получатель | Не раскрывать ему чужие ключи и прочие notes | Стороны платежа знают его сумму и контрагента; они могут сообщить это третьим лицам |
| Активный peer/узел | Не получать view keys или запросы конкретных адресов; проверять весь полученный поток | Сетевые корреляции, задержки, eclipse и выборочное обслуживание не устранены одной ZK-схемой |
| Недобросовестный авторитет | Независимая проверка proofs, conservation и double spend | Forks, censorship, halt, манипуляция составом маленького anonymity set; finality не появляется |
| Вор файла кошелька | Шифрование ключей, notes, меток и локальной истории | Публичный размер/даты файлов; слабый пароль; откат backup; взлом работающего устройства |
| Supply-chain / ошибка circuit | Pin зависимостей, circuit/version allowlist, regression vectors, аудит адаптера | Отсутствие найденной уязвимости не доказывает soundness; скрытая инфляция может быть не наблюдаема постфактум |

Не обещаются защита от глобального наблюдателя трафика, скомпрометированного
устройства, раскрытых viewing keys, singleton anonymity set и всех корреляций.
Особенно слабая приватность при первом распределении, малом обороте и известном
единственном владельце genesis. Одни лишь dummy actions не создают независимых
участников или реального anonymity set.

## 3. Monero и Orchard/Ironwood: разные архитектуры

### 3.1. Что именно сравнивается

**Monero RingCT/CLSAG:** каждый выход содержит одноразовый публичный ключ,
commitment суммы и данные для её восстановления получателем. При расходе
публикуется кольцо ссылок на выходы: один настоящий, остальные — decoys.
CLSAG связывает право расхода и commitment настоящего входа с pseudo-output,
не раскрывая его индекс. Bulletproofs+ ограничивают значения выходов;
баланс commitments и открытая комиссия проверяются отдельно. Уникальный key
image запрещает повторный расход, не указывая напрямую на реальный выход.
Кольцо — публичное множество кандидатов, а не доказательство принадлежности
любому выходу цепи. Это согласованные части RingCT, не независимые плагины.
[Описание RingCT](https://www.getmonero.org/resources/moneropedia/ringCT.html),
[Zero to Monero, 2-е издание](https://web.getmonero.org/library/Zero-to-Monero-2-0-0.pdf),
[переход к Bulletproofs+ в таблице обновлений](https://github.com/monero-project/monero/blob/f8aba51d2d50ebe7c96a0f4d1f2d6e88add61710/README.md#scheduled-softwarenetwork-upgrades).

**Orchard:** зашифрованные notes, commitments в дереве, публичные nullifiers,
ZK proof скрытой принадлежности note дереву и корректности расхода/выхода.
Явного кольца ссылок на выбранные старые выходы нет; anchor задаёт дерево,
относительно которого доказывается membership. Randomized spend authorization
и binding signature дополняют proof. Halo2 в этом стеке использует IPA и
Pasta curves без церемонии trusted setup с секретной «токсичной» частью.
Это не означает отсутствия proving/verifying keys или ошибок в circuit.
[ZIP 224](https://zips.z.cash/zip-0224),
[проверка bundle и подписей](https://github.com/zcash/orchard/blob/0.16.0/src/bundle/batch.rs).

**Ironwood** — новый value pool и версия notes внутри развивающегося стека
Orchard/Zcash, а не альтернативная библиотека «анонимных переводов». В
исследованном `orchard` есть разные pool/protocol/circuit combinations.
`ironwood_v3()` использует PostNu6_3 и V3 note plaintext; `orchard_v3()` имеет
другую политику cross-address transfers. Замена имени pool не задаёт безопасный
режим. Подготовка quantum-recoverable notes не делает нынешние подписи, proof
или сеть постквантово защищёнными.
[Версии в 0.16.0](https://github.com/zcash/orchard/blob/0.16.0/CHANGELOG.md),
[ZIP 2005 и его ограничения](https://zips.z.cash/zip-2005).

**FCMP++ рассматривается отдельно.** Monero развивает full-chain membership
proofs и CARROT; это существенная смена модели относительно колец CLSAG.
На дату исследования официальный roadmap отмечает их как in progress, а
проверенный `monero-wallet` явно ограничивает создание транзакций CLSAG.
Слитый PR с FFI-компонентом не доказывает активацию полного протокола или его
поддержку этим кошельком. Поэтому свойства FCMP++ не приписываются RingCT/CLSAG
в таблице ниже. Для выбора FCMP++ ORVEXA понадобился бы новый review точной
спецификации, реализации, релиза и аудита; сейчас он не предлагается к внедрению.
[Roadmap Monero](https://beta.getmonero.org/resources/roadmap/),
[границы monero-wallet](https://github.com/monero-oxide/monero-oxide/blob/731657ae3385be667abb556266369a497bc86f13/monero-oxide/wallet/README.md),
[PR 10359, компонент FFI](https://github.com/monero-project/monero/pull/10359).

### 3.2. Сравнение по семи критериям

Ниже — оценка переноса в **собственную малую сеть ORVEXA**, а не рейтинг
действующих сетей. Затраты на этом PC пока не измерялись.

| Критерий | Monero RingCT/CLSAG | Orchard/Ironwood | Следствие для ORVEXA |
| --- | --- | --- | --- |
| 1. Суммы и балансы | Суммы скрыты commitments и range proofs; кошелёк суммирует принадлежащие ему непотраченные выходы. Публичный адрес не является открытым счётом | Суммы скрыты в notes/commitments; диапазоны входят в circuit; баланс считается локально по notes и nullifiers | Оба удовлетворяют сокрытию значений при корректной реализации. Шифрование числа в JSON или один range proof этого не обеспечивают |
| 2. Отправитель и получатель | Stealth outputs/subaddresses скрывают связь получателя с выходами. Отправитель скрыт среди выбранных decoys; известные выходы, возраст и пересечение колец уменьшают неопределённость | Получатель скрыт внутри note; диверсифицированные адреса и note encryption. Membership скрывает выбранную note среди допустимых notes anchor, без публичного списка приманок | Преимущество shielded tree для цели ORVEXA: не нужно проектировать decoy sampling. Номинальное число notes не равно числу независимых владельцев |
| 3. Double spend / отсутствие инфляции | CLSAG + key-image set, проверка ring members, pseudo-output balance, range proofs, правил эмиссии | Proof + SpendAuth + binding, известный anchor, nullifier set, ограничения value balance, правил эмиссии | У обоих stateless verifier недостаточен. Состояние и вся экономическая проверка должны атомарно применяться каждым узлом |
| 4. Rust и текущий проект | Rust-стек monero-oxide доступен; текущая Ed25519 account model не подходит. Нужны output index, key images, decoy interface и новые wallet/RPC | Rust crate orchard доступен; нужны note tree, anchors, nullifiers, witnesses, новый wallet/RPC | Rust поддерживается обоими. Сохранить можно инфраструктуру и v1 отдельно; ни один вариант не является заменой поля amount или подписи |
| 5. Готовые проверяемые реализации | Можно брать согласованные transaction/RingCT и scanning/sending libraries, сверять с reference Monero; нельзя считать произвольную сборку примитивов Monero | Можно брать builder, circuit, verifier, signatures, encryption и key derivation одного совместимого стека | Повторно использовать полный протокол выгоднее, чем собирать Pedersen + случайный Bulletproofs + собственный stealth. Аудит адаптера всё равно нужен |
| 6. Кошелёк и Explorer | Кошелёк: scan, output selection, decoys, fee/change, key images, backup. Explorer: блоки, commitments, кольца-кандидаты; не истинный граф | Кошелёк: trial decryption, note selection, witnesses, proving, fee/change, viewing keys, rescan. Explorer: блоки, actions, roots, nullifiers; нет адресного графа | Оба требуют полноценного нового кошелька. Orchard переносит сложность из decoys в proving/witness state. Публичный Explorer полезен без балансов и сумм |
| 7. Лицензии / сопровождение / риск | Rust crates MIT; reference Monero BSD-3-Clause с отдельными notices. Правила и wallet policy тесно связаны с Monero; перенос на новую сеть уменьшает пользу готовой экосистемы | Исследованный orchard 0.16.0 MIT OR Apache-2.0; API и circuit развиваются, есть история критической soundness-ошибки | Нужны pin, lock, список лицензий всего графа, security response и аудит точной ревизии. Ни один upstream не сопровождает автоматически ORVEXA |

Механизмы получателей подтверждаются описанием
[stealth addresses](https://www.getmonero.org/resources/moneropedia/stealthaddress.html)
и [ключей/адресов Orchard](https://zcash.github.io/orchard/design/keys.html).
Свойства range/commitments — [Monero Pedersen commitments](https://www.getmonero.org/resources/moneropedia/pedersen-commitment.html)
и [value types Orchard](https://github.com/zcash/orchard/blob/0.16.0/src/value.rs).
Остальные сравнительные выводы — инженерная оценка на основе описанной модели
и фактической v1, не результаты benchmark или независимого аудита.

У обоих публичны tx/block IDs, комиссия, размер/структура транзакции, порядок
и наблюдаемое время. У RingCT видны ring references и key images; у shielded
tree — anchor, commitments, nullifiers и ciphertexts. Ни один подход не скрывает
IP отправителя сам по себе. Стороны знают собственный платёж; collusion, адрес
из переписки, merchant logs и выдача viewing keys меняют модель наблюдателя.
При известном единственном владельце genesis отправитель первых расходов
может быть очевиден в обоих вариантах. Искусственные decoys или dummy notes
не создают новых независимых пользователей.

### 3.3. Какие части Monero реально пригодны

| Часть | Реальное повторное использование | Что остаётся ORVEXA |
| --- | --- | --- |
| monero-oxide transaction/RingCT | Парсинг/serialization, CLSAG, Monero-specific Bulletproofs(+), commitment types как согласованное семейство | Правила допустимых tx, индексы ring members, spent images, monetary policy, block admission; differential vectors с reference |
| monero-wallet | Scanner, создание/подписание совместимых tx, механизмы addresses и decoys | Persisted wallet state, выбор собственных outputs, фоновые задачи, UX, backups, privacy-safe data source; библиотека прямо не является готовым кошельком |
| Reference Monero | Спецификация поведения, тестовые векторы, правила canonical encoding и проверки; отдельный полноценный fork — другой возможный проект | C++/CMake/FFI при использовании reference binaries, интеграция RPC/БД и обновлений, независимый аудит отклонений; это не продолжение нынешнего Rust ledger |
| Wallet/network practices | Локальное сканирование, subaddress/invoice UX, анализ decoy selection и сетевых утечек | Использовать подходы, не переносить адресные формулы и сериализацию в Orchard |

Проверенная ревизия Rust-источников:
`731657ae3385be667abb556266369a497bc86f13`. Manifests указывают
`monero-oxide 0.1.0`, Rust 1.81, MIT и `monero-wallet 0.2.0`, Rust 1.89, MIT.
Это ревизия для исследования, не подтверждение публикации этих версий в registry
или готовности их общего dependency graph на ORVEXA/Windows.
[Core manifest](https://github.com/monero-oxide/monero-oxide/blob/731657ae3385be667abb556266369a497bc86f13/monero-oxide/Cargo.toml),
[wallet manifest](https://github.com/monero-oxide/monero-oxide/blob/731657ae3385be667abb556266369a497bc86f13/monero-oxide/wallet/Cargo.toml).

Взять только `monero-bulletproofs` допустимо для отдельного исследования range
proof, но это не даёт приватную валюту. Даже общая кривая с Ed25519 не делает
account signatures v1 совместимыми с CLSAG, key images или Monero commitments.
Генераторы, кодирование точек, subgroup checks и transcripts должны оставаться
upstream. Не заменять Monero Bulletproofs+ на Ristretto `bulletproofs` из другого
семейства. Новый network/address context, защита от межсетевого replay и
подписываемые байты требуют отдельного review: внешний ORVEXA envelope сам по
себе не меняет того, что подписывает готовый Monero signer.

Полный fork Monero дал бы больше готовых node/wallet частей, если приоритетом
стало бы максимальное следование его правилам. Он всё равно не переносит
существующий anonymity set, пользователей или ликвидность. Для нынешней цели
собственной Rust-архитектуры это более крупная смена проекта, а перенос одних
RingCT-примитивов не даёт обещанного сокращения работы.

### 3.4. Какие части Orchard/Ironwood реально пригодны

Повторно использовать согласованный `orchard` builder/prover/verifier, стандартные
note/value commitments, note encryption, randomized authorization/binding,
nullifier derivation, keys и tree hashing. Для wallet derivation/tree storage
подбирать совместимые upstream ZIP32/tree libraries; готовая Zcash wallet DB,
lightwalletd, unified-address UX и Explorer не становятся ORVEXA-продуктами
после подключения crate. Coin type, network encoding, scan sync и storage adapter
остаются нашими задачами. Библиотека не предоставляет consensus, политику fee,
genesis или state-level double-spend validation за узел.

Исследованный кандидат для стенда: `orchard = "=0.16.0"`, `std`, `circuit`,
`multicore`, `zeroize`, без unstable-voting/frost. Для обычных межпользовательских
переводов кандидат — `BundleVersion::ironwood_v3()` и
`OrchardCircuitVersion::PostNu6_3`, с явно разрешёнными spends/outputs/cross-address.
Режимы старой Orchard не выбираются по возрасту аудита. Номер v2 в ORVEXA не
означает Orchard ProtocolVersion V2 и не означает Zcash wire compatibility.

Ранее выполненный `cargo info orchard@0.16.0 --registry crates-io` подтвердил
публикацию, MIT OR Apache-2.0 и MSRV 1.88. Локальный source metadata указывает
commit `8190e4d9b7e9c0140964e88b71f95276d2b50470`. Пакет **не добавлен** в
workspace v1; с 2026-10-18 он собран и измерен только в изолированном
[стенде P1](../research/orchard-eval/README.md) со своим lock-файлом. Его manifest требует семейства Halo2 0.4/gadgets 0.6,
Pasta 0.6, RedDSA 0.6, note encryption 0.5.1, ZIP32 0.3, tree 0.9 и rand 0.10;
это не зафиксированный и проверенный dependency graph ORVEXA.
[Manifest](https://github.com/zcash/orchard/blob/0.16.0/Cargo.toml),
[лицензия и MSRV](https://github.com/zcash/orchard/blob/0.16.0/README.md).

### 3.5. Проверенность, лицензии и сопровождаемость

- **Monero C++ CLSAG:** публичный аудит охватывает реализацию и интеграцию
  на commit `66bcde2` (май 2020). Он не является аудитом Rust-переноса,
  Bulletproofs+ другого выпуска или ORVEXA.
  [Отчёт CLSAG](https://www.getmonero.org/resources/research-lab/audits/clsag.pdf).
- **Rust Monero:** найден опубликованный аудит Cypher Stack / Serai
  `networks/monero`, commit `48db06f901952b24bb38d7c7e256f798f08512cd`,
  с исключением `verify-chain`. Upstream связывает его с происхождением
  monero-oxide. Нужна проверка устранения замечаний и diff до выбранной
  ревизии `731657a`; нынешний код не объявляется целиком покрытым отчётом.
  Формализация FROSTLASS относится к threshold signing, а не ко всему ledger.
  [Происхождение аудита](https://github.com/monero-oxide/monero-oxide/blob/731657ae3385be667abb556266369a497bc86f13/audits/Cypher%20Stack%20May%202025/README.md),
  [текст отчёта, зафиксированный release 1.1](https://github.com/cypherstack/monero_wallet_audit/blob/1931985e68c2c91db367f729c11f9cfbd0c3b298/Misc/Audit.tex).
- **Orchard/Ironwood:** критический дефект under-constrained gadget позволял
  обходить soundness старого circuit, включая инфляцию и повтор notes с разными
  nullifiers. Исправление circuit/key — часть consensus, не косметический
  dependency update. Исторические insecure API необходимо закрыть allowlist.
  Отсутствие trusted setup не предотвращает такие ошибки. В Zcash отдельный
  turnstile ограничивает вывод из pool; ORVEXA без такого учёта не наследует
  эту защиту. После ошибки скрытого witness цепь может не позволять установить
  факт внутренней инфляции задним числом.
  [Официальный advisory](https://github.com/zcash/zcash/security/advisories/GHSA-ghc3-g8w4-whf9).
- **Границы формальной проверки:** проект Ironwood публикует Lean development
  и описание scope; его README говорит о работе над проверкой. Здесь не
  собраны Lean proofs, не доказано соответствие всей orchard 0.16.0 формальной
  модели, не проверена наша интеграция. Список аудитов не заменяет эту цепочку
  соответствия. Страница Zellic была недоступна для полного чтения, поэтому
  её выводы не используются как подтверждение безопасности кандидата.
  [Ironwood formalization](https://github.com/zcash/ironwood).

Лицензии исследованных верхних Rust crates допускают повторное использование
с соблюдением условий MIT либо Apache-2.0. Для reference Monero нужно отдельно
сохранить BSD-3-Clause/MIT notices и проверить встроенные зависимости.
[Reference LICENSE](https://github.com/monero-project/monero/blob/f8aba51d2d50ebe7c96a0f4d1f2d6e88add61710/LICENSE).
Старая лицензия другой версии Orchard не определяет лицензию 0.16.0. До сборки
стенда фиксируются source/registry checksums и lock, а до распространения —
лицензии, notices и ограничения **всего** графа. Верхний manifest не является
таким аудитом. Копирование готового GUI/Explorer потребовало бы отдельной проверки
его лицензии; лицензия crypto crate его не покрывает.

Сопровождение для обоих путей означает регулярную проверку upstream advisories
и RustSec, regression vectors, review обновлений circuit/формата и возможность
экстренно остановить экспериментальный приём tx. Pin защищает от неожиданной
смены версии, но оставляет известную ошибку до осмысленного обновления. Модуль
не должен откатываться на insecure suite ради сборки. У ORVEXA пока нет полного
аудита разрешённого графа, локальных benchmark обоих кандидатов, гарантии
совместимости мобильных/hardware wallets или команды сопровождения upstream fork.

### 3.6. Рекомендация и условия её пересмотра

Рекомендуется **shielded-note архитектура Orchard, с проверкой Ironwood-v3**:

1. Приоритет пользователя — скрывать связи отправителя. Отсутствие публичного
   кольца и необходимости выбирать decoys лучше соответствует этой цели.
   Это преимущество модели, не обещание анонимности в маленькой сети.
2. Есть единый Rust-стек для совместимых proofs, authorization, notes и
   receiving. Он уменьшает необходимость самостоятельно связывать примитивы.
   Monero Rust тоже существует; аргументом выбора является модель, а не язык.
3. ORVEXA v1 ещё не имеет стоимости и зрелой UTXO-инфраструктуры. Можно оставить
   её работающей и построить отдельный v2 ledger, не выдавая несовместимую
   миграцию за обычное обновление пакетов.
4. Продукт «кошелёк + переводы + Explorer» реализуем с этой моделью: личные
   данные расшифровываются кошельком, а публичный Explorer показывает проверяемые
   факты о блоках и включении транзакций. Ему не нужен доступ к balances.

Цена выбора — сложный prover, witnesses, circuit/version discipline и
критическая зависимость от soundness. Поэтому **не рекомендуется сейчас
внедрять orchard в работающий узел или объявлять 0.16.0 окончательным выбором**.
Первый этап P1 проверяет факты на отдельном стенде; P2 рассматривает протокол
адаптации. Ошибка сборки не оправдывает самодельный proof или старый circuit.

Вернуться к Monero следует, если основатель выберет близкий fork его полного
протокола/продуктов и примет decoy model, либо если актуальный безопасный
Orchard/Ironwood не пройдёт аудит ревизии, лицензионную проверку или согласованные
ресурсные требования кошелька. Тогда нужен новый дизайн целого RingCT ledger,
bootstrap/decoy policy и wallet, а не смена одной зависимости. FCMP++ — отдельный
будущий кандидат после проверки зрелости; автоматически смешивать его с
Orchard нельзя.

CT-only варианты не закрывают цель: Elements secp256k1-zkp скрывает значения,
но обычные UTXO references оставляют граф; Ristretto `bulletproofs` 5 даёт range
proof, не ownership/membership/receiving. Они не предлагаются третьим «простым»
способом получить все требуемые свойства.
[Elements manifest](https://github.com/ElementsProject/rust-secp256k1-zkp/blob/master/Cargo.toml),
[Bulletproofs 5](https://docs.rs/crate/bulletproofs/5.0.0).

**Разделы 4–8 — проект адаптации рекомендованного семейства**, не утверждённая
wire-спецификация. Любая точная сериализация, sighash, mint exception и граница
upstream проверяются в P2 до кода узла. Новые примитивы/circuits не предлагаются.

## 4. Конкретная модель notes, стоимости и proof

На узле больше нет `Address -> balance`. Он хранит append-only дерево скрывающих
note commitments, допустимые roots и множество использованных nullifiers.
Note содержит скрытые получателя, неотрицательное значение, случайность и
связанные с протоколом параметры. Commitment фиксирует их; ciphertext позволяет
получателю обнаружить и восстановить note. Употребление commitment не означает,
что из него можно прочитать сумму или что node знает, какой commitment потрачен.

Один action имеет старую и новую note, при необходимости dummy; публичны
`cv_net`, nullifier, randomized spend key, `cmx`, ephemeral key и ciphertexts.
Суммы и witness не входят в публичный конверт. Range constraints встроены в
заимствованный Action circuit; отдельные самодельные Bulletproofs не добавляются.
Концептуально value commitment связывает стоимость с маскирующей случайностью,
а binding signature связывает их баланс для всего bundle. Формулы и генераторы
используются только через библиотеку, не переписываются вручную.

Проверяемый инвариант обычной транзакции:

```text
sum(real input values) = sum(real output values) + public fee
bundle.value_balance = public fee
1 <= fee <= remaining test supply <= 1_000_000
```

Halo2 proof проверяет допустимость скрытых значений (u64), принадлежность входных
notes дереву, связи ownership/nullifier и корректность commitments. SpendAuth и
binding signatures проверяются **в дополнение** к proof. State machine проверяет
уникальность nullifiers и принадлежность anchor текущей принятой истории.
Ни range proof отдельно, ни библиотечный verifier отдельно не решают double spend.

Нормальные транзакции всегда разрешают spends и outputs; отдельное поле типа
транзакции не даёт права mint. Отрицательный value_balance, отличие от fee,
spends-disabled mint вне genesis и произвольные pool/circuit flags отвергаются.
Fee остаётся публичной и сжигается. Остаток эмиссии вычисляется публично как
`genesis_supply - checked_sum(fees)`, а не суммированием открытых балансов.

Кошелёк использует собственный checked Amount в пределах тестовой эмиссии,
отвергает отрицательные/дробные/слишком большие пользовательские значения до
преобразования в библиотечный NoteValue. Расчёты сумм — checked i128, конверсия
fee/value balance в i64 только через TryFrom. Circuit доказывает диапазон u64,
не наш более узкий лимит 1 000 000: меньший глобальный предел следует из genesis,
сохранения стоимости и отсутствия последующей эмиссии. В P2 нужно явно сверить
границы числа actions и суммы значений с алгебраическими ограничениями upstream,
чтобы модульное равенство не принималось за целочисленное conservation. Одних
checked операций кошелька недостаточно против присланных атакующим bytes.

Dummy notes с нулевой стоимостью — часть стандартного доказательства/padding,
не фиктивные деньги. Положительный платёж — правило кошелька; не обещаем, что
узел отличает dummy от реального скрытого нулевого выхода.

## 5. Конверт ORVEXA v2 и порядок проверки

Предложенный envelope: `version=2`, `chain_id`, `suite_id`, fee u64,
`expiry_height` u64 и один authorized Ironwood bundle. Статического sender,
recipient, открытого amount и account nonce нет. Для начального эксперимента
допустимы 2/4/8 padded actions; это ресурсный параметр для тестирования, не
утверждённая политика production. Лимиты байтов/времени P1 измеряет, P5 фиксирует
до включения RPC; лимиты v1 нельзя автоматически перенести на тяжёлые proofs.

Используются канонические encodings библиотечных points/fields. Единственный
публичный decoder адаптера выбирает suite по allowlist, не по присланному VK.
Он применяет `Bundle::try_from_parts`, точную `Proof::expected_proof_size(n)`,
ограниченные lengths и запрет trailing bytes. JSON может переносить canonical
bytes, но никогда не определяет signing representation.

Sighash конверта — SHA-256 стандартными средствами с доменом
`ORVEXA/shielded-sighash/v2`, chain ID, version/suite, fee, expiry, числом actions,
anchor, flags и библиотечным `Bundle::commitment(TxVersion::V6)` в фиксированном
каноническом порядке. Anchor включается явно: в библиотечном v6 digest он не
входит в ту же часть commitment. Proof и signatures не включаются в подписываемую
часть, чтобы не создать цикл. Полный tx ID и commitment блока связывают **все**
байты authorized envelope, включая proof, signatures и ciphertexts.
Это предложение адаптера, требующее отдельных векторов и аудита, а не заявление
побайтной совместимости с транзакцией сети Zcash.

Проверка узлом:

1. Bounds/canonical decode, версия/сеть/suite, expiry, flags и fee; reject empty.
   Для ordinary bundle требуются точные default flags Ironwood-v3: spends,
   outputs и cross-address включены; genesis проверяется отдельным путём.
2. Anchor из принятой истории; одинаковый anchor для bundle. На первом сетевом
   эксперименте — tip state root, затем отдельно спроектировать окно старых roots.
3. Нет повторов nullifier внутри bundle/блока/цепи и конфликтов pending pool.
4. `value_balance == fee` и checked burn accounting.
5. VK построен только для PostNu6_3. Через `BatchValidator::new(vk)`,
   `add_bundle(bundle, sighash)` и `validate(OS CSPRNG)` проверяются все proofs,
   SpendAuth и binding signatures. Проверка одного `verify_proof` недостаточна.
6. Только после всех проверок атомарно добавить cmx в дерево, nullifiers в set,
   блок, roots и burned fees. Ошибка/сбой не меняет принятого состояния.

У batch validator пустая очередь допустима в upstream; наш адаптер обязан
отвергать отсутствие bundle/actions до этого вызова.
[API проверки](https://github.com/zcash/orchard/blob/0.16.0/src/bundle/batch.rs),
[конструкторы и commitments](https://github.com/zcash/orchard/blob/0.16.0/src/bundle.rs).

## 6. Получатели, stealth addresses и сокрытие отправителя

Классический Monero stealth destination использует view/spend keys, ephemeral
key agreement и одноразовый output key. Его нельзя просто пристыковать к другой
кривой и note circuit. Для рекомендованного семейства предлагается его аналог:
diversified receiving addresses, скрытый receiver внутри note commitment и
in-band note encryption с ephemeral key. Адрес не публикуется в action; для
каждого платежа нужны новые note randomness/ephemeral secret, для invoices
кошелёк выдаёт новые diversifiers. Самостоятельная реализация DH/KDF не нужна.

Способ выдачи нового адреса — wallet policy, не доказательство сетевой анонимности.
Получатель сканирует все outputs локальным incoming viewing key, проверяет
расшифрованную note и commitment, сохраняет её и witness. Full viewing key
позволяет отслеживать nullifiers; outgoing viewing key позволяет восстановить
собственные исходящие сведения. Viewing keys не передаются узлу.
[Note encryption](https://github.com/zcash/orchard/blob/0.16.0/src/note_encryption.rs).

Отправитель скрыт через ZK membership/ownership для дерева, а не через кольцо
явных UTXO или публикацию постоянного Ed25519 sender. Randomized authorization
key не является account address. Nullifier показывает повтор расхода, но не
явную ссылку на исходный commitment. Защита ограничена реальным anonymity set,
известными контрагентами, временем, платёжными шаблонами и захватом view keys.

В v2 API нет глобального balance/history по адресу. Узел выдаёт блоки/compact
outputs по высоте; кошелёк считает баланс локально. Кошелёк загружает одинаковый
поток для сканирования, не делает адресные фильтры или запросы собственных
nullifiers. Данные о witness берутся локально из дерева. Открытая fee и размер
bundle остаются признаками; padding ограничивает, но не устраняет их.
Первая версия кошелька использует пустые memos и фиксированные стандартные
ciphertext sizes. Транспортная приватность — отдельный этап P6.

## 7. Хранение, восстановление и целостность

Отдельная `orvexa-private-test-v2.sqlite`, без изменений `user_version=1` v1.
Логические таблицы v2: genesis/config, ordered blocks, note commitments/positions,
spent nullifiers с UNIQUE, tree checkpoints/anchors, tip и burn counter.
Блок связывает parent, ordered full tx IDs и новые state roots. Состояние
строится из полностью проверенного replay; snapshots — только ускорение,
сверяемое с replay. Commit всех записей одной SQLite transaction предшествует
изменению памяти. Pending reservations не считаются потраченными на диске;
после принятого блока pool перепроверяется, после рестарта его можно отправить
снова. Reorg не добавляется скрыто под видом приватности.

Кошелёк v2 — отдельный encrypted format, с master seed, chain/network context,
ZIP32 account parameters, scan birthday, diversifier counter, viewing/spend keys
и зашифрованным локальным состоянием notes. Старый Ed25519 key не считается
совместимым spending key. Новый network-bound address envelope не называется
`void_test_`; его checksum/отображение согласуются в P4 без нового криптоалгоритма.
В открытом заголовке v2 остаются только необходимые format/KDF/salt/nonce
параметры; адреса, viewing keys и метки должны быть внутри encrypted payload,
а не наследовать открытое поле address контейнера v1.

Резервная копия seed/ключевого контейнера и пароль должны позволять rescan всей
принятой цепочки, восстановление входящих notes, outgoing recovery по OVK и
определение spent через nullifiers. Сначала backup encrypted container; формат
мнемоники — отдельное последующее решение на основе стандарта, не самодельные
слова. Кошелёк сверяет подписанную историю/roots, не доверяет ответу удалённого
баланса. Scan birthday — ускорение, не обязательный секрет: восстановление должно
работать и от высоты 0. Метки и внешние invoice references из seed не выводятся,
для них нужна отдельная encrypted metadata backup. Запись нового backup/cache —
temp + sync + атомарная публикация с
проверенным Windows/Linux поведением, без перезаписи единственной рабочей копии.

Баланс, notes, witnesses, seed, masks и viewing keys не попадают в node API,
логи/Debug/errors. CLI приватного платежа должен получать чувствительные поля
интерактивно либо через локальный защищённый ввод, а не открытые argv.
Зашифрованный диск не защищает от malware в работающем процессе.

### Кошельки и переводы как конечный продукт

Первый целевой кошелёк v2 — desktop CLI с локальным сканированием и proving,
без сервера, которому передаются seed/view keys. До начала реализации фиксируется
ресурсный бюджет целевого компьютера. Mobile, hardware signing и удалённый prover
не обещаются: для них нужны отдельные измерения и модель доверия.

Минимальная проверяемая пользовательская цепочка: Alice и Bob создают разные
encrypted wallets; Bob выдаёт network-bound receiving address; Alice выбирает
notes, проверяет адрес/сумму/fee/change и строит tx локально; узлы независимо
проверяют её; Bob обнаруживает note сканированием. Оба видят pending/confirmed
и идентификатор включения. После Alice -> Bob -> Charlie выполняется удаление
только тестовых caches и rescan из backups; подтверждённые суммы и spent state
должны совпасть. Метки контактов восстанавливаются из metadata backup, не seed.
Повтор расхода и неподтверждённая сдача обрабатываются явно; unconfirmed chaining
не входит в первый продукт.

Для Monero-пути понадобились бы scan/output selection/decoys и key-image state.
В классической модели view-only ключ показывает входящие, но для достоверного
spent balance обычно нужен экспорт key images или spend-capable wallet.
Это отличается от полномочий Orchard FVK/IVK/OVK; нельзя обещать одинаковый
watch-only UX при смене backend.
[View keys Monero](https://www.getmonero.org/resources/moneropedia/viewkey.html).

### Explorer без раскрытия платежей

Публичный Explorer — индексатор проверенной цепи и read-only UI, не wallet
backend. Он не принимает spend/view keys, не хранит openings и не обещает
баланс адреса, реальный объём переводов, rich list или граф «кто кому заплатил».
Личные квитанции и выборочное раскрытие проектируются отдельно, локально в
кошельке. Tx ID доказывает включение bytes в принятую историю, но сам по себе
не доказывает Explorer, кому и сколько заплачено.

| Функция Explorer | Monero-подобная ORVEXA | Shielded-note ORVEXA |
| --- | --- | --- |
| Блок/поиск по height/hash/tx ID | Header, ordered tx IDs, размер, fee, состояние проверки | То же, плюс проверенные roots и число actions |
| Детали публичной tx | Commitments, output keys, key images, ring candidates; кандидаты не отмечаются как реальные отправители | Commitments, nullifiers, anchor, proof/auth bytes; не выдумываются входные адреса |
| Supply | Проверенные правила mint/fee, не сумма открытых балансов | Проверенный genesis и накопленный burn; integrity зависит от proof soundness |
| Личная история/баланс | Только в кошельке | Только в кошельке |

Для рекомендуемого пути: отдельный индекс с checkpoint block ID, возобновление
после сбоя и сверка с валидирующим узлом; индекс можно перестроить из блоков.
Не отдавать непроверенный proof как «валидный» по одному факту включения у peer.
Показывать число подтверждений и доверие к авторитету: оно не является
Byzantine finality. Запрос tx ID, access logs и внешняя аналитика Explorer тоже
могут связать посетителя с платежом; на локальном MVP сторонние trackers не
нужны. Публичный сервис требует собственного анализа metadata/DoS в P6.

## 8. Genesis, миграция и отсутствие скрытой эмиссии

Рекомендуется **отдельная новая тестовая сеть**, без конвертации v1-балансов,
bridge или скрытого смешанного режима. Это предложение, не уже выполненная
миграция. v1 данные/keys сохраняются, его demo продолжает работать. Новый chain ID,
genesis, каталоги и wallet format не позволяют принять v2 за прежнюю сеть.
Публичная история v1 не станет приватной задним числом. Значение 1 000 000 и
сжигание fee >= 1 предлагается явно сохранить как тестовые параметры; сеть
не создаёт экономического права на старые атомы. Ни одна сеть не предназначена
для реальных денег.

Кандидат конструкции genesis для review P2 — один специальный bootstrap bundle:
spends disabled, outputs enabled, `value_balance = -1_000_000`; размер эмиссии
закреплён в config. Проверяются proof, signatures и его выходные commitments.
После genesis подобный режим запрещён, включая отрицательный value balance.
Первый эксперимент может иметь одного получателя genesis; это раскрывает
исходное распределение и не обеспечивает anonymity set.

Чтобы избежать цикла chain ID/signatures, network config фиксирует соль,
authority, suite/circuit, экономику и **неавторизованный digest bootstrap bundle**.
Chain ID вычисляется из config, затем подписывается bootstrap context; полный
genesis block ID связывает уже авторизованные байты. В P2 нужен отдельный review
и векторы этого порядка; до него genesis v2 не принимается ни одним рабочим узлом.
При развёртывании фиксируется также точный полный genesis block ID из общего
genesis-файла: одинаковый config не разрешает принимать другой authorized genesis.

Если позже потребуется перенос v1 test atoms с сохранением права владения,
нужен отдельный snapshot/claim либо burn/deposit/turnstile дизайн: один старый
остаток нельзя одновременно потратить в v1 и получить в v2. Требуются решение
о freeze/finality старой сети, уникальность claim, rollback policy, conservation
между сетями и анализ публичных сумм/времени claim. Это не автоматическая
«обёртка» кошелька; **bridge и claims не включены в этот план и не выполняются**.
Смена консенсуса, наград и денежной политики не является условием выбора privacy
stack и не реализуется под видом переименования или приватности.

## 9. Этапы внедрения и точка согласования

| Этап | Изменения | Критерий завершения |
| --- | --- | --- |
| P0 — эта редакция | Сравнение семи критериев, рекомендация, границы reuse, wallet/Explorer и план | Основатель рассматривает направление; нет изменения работающего протокола |
| **P1 — только проверка upstream, после отдельного согласования** | Отдельный evaluation workspace вне рабочего Cargo workspace v1; source/lock/license/advisory/audit-diff report; upstream fixtures и benchmark кандидата, сравнение доступных Monero fixtures | Проверена совместимость Rust/Windows, настоящие proofs/signatures и отказ malformed данных; измерены затраты, неизвестное отмечено; решение go/no-go по стеку |
| P2 — спецификация и offline адаптер, отдельное согласование | Зафиксировать suite, encoding/sighash/chain binding, fee/conservation, anchor policy, resource limits и genesis; затем один изолированный adapter с векторами | Review формата и bootstrap до реализации; canonical/mutation/over-mint tests, ни одного пути приёма v2 в v1 |
| P3 — state, отдельная БД | Note tree, nullifier set, burn и атомарный replay; genesis только по принятой спецификации P2 | Double spend в pool/блоках/после restart, неизвестный anchor, crash/corruption/overflow отвергаются |
| P4 — desktop wallet | Encrypted keys/notes, receiving addresses, local scan/witnesses/proving, send/change, backup/rescan | Alice -> Bob -> Charlie; независимое восстановление из backup, spent balance и metadata scopes проверены |
| P5 — отдельная локальная v2 сеть и Explorer | Versioned blocks/API, три узла, индексатор и read-only UI; durable equivocation evidence | Все узлы согласованно проверяют tx/state; wallet transfer/restart + Explorer inclusion; индекс перестраивается, личные суммы не раскрываются |
| P6 — hardening и независимый аудит | Metadata/traffic/DoS analysis, fuzz/property/crash tests, review точного circuit/adapter/wallet/consensus integration | Опубликованы измерения, охват аудита и ограничения; дальнейшее развёртывание решается отдельно, без обещания готовности к деньгам |

**Граница небольшого P1:** нет собственного ORVEXA envelope, sighash, genesis,
кошелька, ledger, RPC, bridge или нового circuit. В стенде допускаются только
синтетические upstream notes/keys и настоящие builder/prover/verifier fixtures.
Нельзя заменять proof mock-объектом и считать такой результат проверкой crypto.
Аудиторский diff связывает конкретный release с отчётом; недоступные материалы
отмечаются как пробелы. Зависимости рабочего v1, его Cargo.lock и demo не меняются.
При неудаче стенда работающая версия сохраняется; выбор возвращается на review.

До стенда P1 фиксируются целевое desktop hardware и бюджет времени/памяти/bytes;
в отчёте приводятся CPU/RAM/OS/toolchain, release build, feature set, cold/warm key
generation, proving/verification 2/4/8 actions, scan throughput, размеры и peak RAM.
Для RingCT при сравнении указываются реальные input/output/ring shapes и источник
decoy corpus; неподходящий маленький corpus — ограничение, не повод ослаблять
проверки. Не сравнивать proof-only с полной проверкой чужой tx и не объявлять
придуманные TPS/миллисекунды измерениями. Если безопасный stack, audit evidence
или ресурсный бюджет не подтверждены, P2 автоматически не начинается.

После P1 требуется решение о стеке; до P2 — review security-critical спецификации;
до P5 — готовность восстановления и состояния; до выхода за локальный devnet —
P6 и отдельная стратегия authority equivocation/consensus. Календарные сроки без
первых измерений не обещаются. Публичный UI v1 можно развивать отдельно, но он
будет показывать прозрачную сеть и не считается приватным Explorer v2.

`orvexa_crypto::privacy::capability()` остаётся Transparent для v1 на всех этапах
отдельного исследования. Подробные условия: [PRIVACY_TEST_PLAN.md](PRIVACY_TEST_PLAN.md).
**Сейчас реализована только прозрачная v1; P1–P6 не выполнены. Запрос сравнить
варианты не является согласованием реализации какого-либо из этих этапов.**

## 10. Матрица готовности

| Свойство | Сейчас | После P1 | Требуется дальше |
| --- | --- | --- | --- |
| Подписи, v1 conservation/nonce, SQLite restart | Реализовано; regression tests | Сохраняется | Более широкие adversarial/fuzz tests |
| Скрытые суммы и баланс | Отсутствует | Только upstream offline evidence, без протокола ORVEXA | P2–P6 |
| Unlinkable receiving / sender membership | Отсутствует | Библиотечные механизмы в синтетических fixtures | P4–P6 и реальные ограничения anonymity set |
| Private double-spend protection | Отсутствует | Stateful защита ещё отсутствует | P3 nullifier state и replay |
| Seed-based recovery / private history | Отсутствует | Отсутствует | P4 |
| Приватные переводы пользователей / Explorer | Отсутствует | Отсутствует | P4/P5, аудит P6 |
| Сетевая анонимность / Byzantine finality | Отсутствует | Отсутствует | Отдельные решения, не обещаются этим дизайном |

## Источники и проверка исследования

Использованы официальные исходники/спецификации, не сторонние списки «безопасных
криптобиблиотек». Лицензия верхнего crate не заменяет проверку всего dependency
graph; наличие файла audits в репозитории не объявляется аудитом выбранного commit.
Публикация orchard 0.16.0 ранее проверена локальным Cargo. В этой редакции
дополнительно проверены pinned Monero manifests/README и provenance аудита
Cypher Stack; scope установлен по тексту отчёта, но проверка устранения замечаний
и audit-diff до нынешней ревизии не выполнены.
Сборка кандидатов, скорость proving/scanning и совместимость адаптера пока не
проверены. История прежних запусков v1 — в [VALIDATION.md](VALIDATION.md).

- [Orchard pinned source](https://github.com/zcash/orchard/tree/0.16.0)
- [Bundle version, canonical constructor и digest API](https://github.com/zcash/orchard/blob/0.16.0/src/bundle.rs)
- [PostNu6_3 circuit](https://github.com/zcash/orchard/blob/0.16.0/src/circuit.rs)
- [Value ranges и checked conversions](https://github.com/zcash/orchard/blob/0.16.0/src/value.rs)
- [ZIP 224 / shielded protocol](https://zips.z.cash/zip-0224)
- [Monero project](https://github.com/monero-oxide/monero-oxide)
- [Stealth address mechanism](https://www.getmonero.org/resources/moneropedia/stealthaddress.html)
- [Dandelion++ paper](https://arxiv.org/abs/1805.11060)
