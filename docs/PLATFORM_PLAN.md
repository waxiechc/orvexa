# ORVEXA — план полной сети, монеты и приложения

2026-10-18. План последовательной реализации, не объявление готовой mainnet.
Прозрачная v1 сохраняется: genesis, подписи, кошельки, экономика и базы совместимы.

## Главная цель: приватность с добровольным раскрытием

Суммы, связи обычных адресов с платежами и вычисление баланса скрыты по умолчанию.
Владелец может отдельно выбрать публичный перевод, раскрыть конкретный платёж
или предоставить ключ просмотра. Публичный режим никогда не включается как
автоматический fallback при сбое приватного proving.

| Возможность | Целевое поведение | Сейчас |
| --- | --- | --- |
| Private payment | Commitments, проверенные proofs, одноразовые получатели, отсутствие прямой связи отправителя | Нет, v1 прозрачная |
| Hidden balance | Локальный rescan приватных notes; нет публичного address→balance индекса | Нет |
| Share payment | Проверяемое раскрытие конкретного платежа с chain ID/tx ID без общего viewing key | Проверить возможности точного upstream |
| View wallet | Отдельный ключ просмотра без spending authority; явный охват прошлых/будущих данных | Нет |
| Public payment | Отдельный явно подтверждённый режим, не меняющий остальные платежи | Только прозрачная v1 |
| Balance report | Раскрытие на выбранной высоте; отдельно доказать полноту, а не только владение выбранными notes | Нет |
| Hide on screen | Маска чисел интерфейса | Не криптографическая приватность |

Уже опубликованную сумму нельзя убрать из чужих копий цепочки. Переданный
viewing key или скопированный отчёт нельзя отозвать у узнавшего их человека.
Viewing key может раскрывать историю, будущие платежи и связи адресов; не выдавать
его вместо квитанции одного перевода. Подпись числа сама по себе не доказывает
полный баланс. Переходы public↔private могут раскрывать суммы, тайминги и связи.
Explorer не пытается обойти приватность; раскрытие пользователю не публикует
данные всем автоматически. IP/тайминги и малый anonymity set — отдельные риски.

Подробная криптографическая граница: [PRIVACY_DESIGN.md](PRIVACY_DESIGN.md).

## Порядок работ

### E1 — native приложение и управление узлом (текущий этап)

Windows EXE с существующим English UI, утверждённым логотипом и палитрой.
Node page: импорт публичных genesis/network, проверка chain ID, Start node /
Stop node, реальные peers/height/role. Rust-узел работает в фоне приложения;
Wallet/Explorer используют настоящий локальный API. Данные отдельно от EXE,
повторный запуск восстанавливает состояние, второй экземпляр не открывает тот
же профиль/базу. Контроллер не получает spending passwords/seeds.

Ограниченный restart/backoff при обычном сбое; quarantine не снимается.
Возобновление при открытии приложения по настройке. Трей и автозапуск при входе
в ОС — следующая явная настройка, без скрытой установки службы.
Роль E1 — **verifying full node**, не валидатор кворума и не майнер. Нет наград
на счётчике интерфейса. Сеть без конфигурации не создаётся случайно сама.
WebView2 — локальный системный движок Windows, не сервис/аккаунт; при отсутствии
нужен явный runtime/installer, без скрытой установки.

Приёмка: чистый setup; отказ от подменённой genesis; start/stop/restart;
постоянная база и browser profile; foreign Origin/Host rejection; отсутствие
произвольных файловых/ключевых команд; реальный перевод/Explorer; переносимый EXE.

### E2 — изолированная проверка приватного backend

Выполнить P1 privacy design: точный maintained upstream, лицензии, audit scope,
настоящие proof create/verify, отрицательные vectors и ресурсы на обычном ПК.
Shielded-note подход — кандидат после сравнения Monero/Orchard; точный стек не
объявляется принятым без evidence. Проверить API viewing keys и disclosure
одного платежа. Не комбинировать случайные ring/range/stealth примитивы.
Пакет оценки вне live workspace v1; никаких cosmetic privacy claims.

### E3 — отдельный приватный ledger

Версионированная сеть/форматы, commitments, note tree, anchors, nullifiers,
шифротексты, binding/conservation и fees. Atomic storage tree/nullifiers/blocks,
replay и crash recovery. Public/private переходы имеют точную алгебру стоимости.
Не конвертировать v1 эмиссию автоматически. Приёмка: fake proofs, двойные траты,
лишняя эмиссия, отрицательные/переполненные суммы, invalid anchors/signatures,
corrupt tx, kill посередине commit и отсутствие утечек в API/Explorer.

### E4 — seed-кошелёк и раскрытие данных

Стандартная библиотека mnemonic (BIP39-кандидат), системная энтропия, 24 слова,
проверка выбранных слов, пароль/локальное шифрование/блокировка. HD derivation,
версии/coin profile согласовать с приватным backend: одних слов недостаточно
без стабильного derivation и rescan. Не писать свои KDF/набор слов.
V1 random-key backup импортируется как legacy: случайная фраза не восстановит
его ключ. Seed не попадает RPC, логам, URL, аналитике/clipboard по умолчанию.

Rescan/birthday, witness refresh, payment URI с сетью и режимом, адресная книга,
offline backup verification; отдельные подтверждения Public, Share payment и
Share view-only access. Приёмка: чистая установка из seed восстанавливает те же
адреса/notes/баланс после отправок/restart; wrong words/order/checksum; отказ от
ложного успеха; inter-device restore. Необязательная BIP39 passphrase создаёт
другой кошелёк и не является проверяемым паролем существующего.

### E5 — автоматический многовалидаторный консенсус

Направление integration spike: готовый BFT engine, например **CometBFT + Rust
ABCI-приложение**, с ротацией proposer, кворумом, WAL и durable signing state.
Это self-hosted софт внутри дистрибутива, не чужая облачная услуга.
Exact version/API/license выбираются spike; самодельный алгоритм «кто первый
проснулся — тот авторитет» не принимается. Нужна новая спецификация commit,
validator-set updates и application state; это не замена одной Ed25519-подписи
в неизменённом v1 блоке. Транспорт BFT engine может отличаться от libp2p.

Проверочная сеть: четыре независимых валидатора равной силы, commit требует
**более 2/3 voting power**. При отказе proposer следующий раунд меняет его
автоматически. При 3/4 работа продолжается; при 2/4 commit останавливается.
Два равных валидатора требуют обоих: безопасное BFT failover на одном из двух
не обещается. 100 процессов на одном ПК не дают независимости операторов.

Admission: любой запускает full node → синхронизация → заявка/отдельный consensus
key → проверяемый bond/stake/validator-set membership → активация на границе
эпохи. Кнопка автоматизирует процедуру, не создаёт voting power из числа EXE.
Wallet/consensus/transport keys разделены; seed не заменяет signing state.

Приёмка: proposer offline, 3/4 продолжает, 2/4 останавливается; partitions 3+1
и 2+2, задержки/reordered votes, rejoin, все crash→quorum recovery;
anti-double-sign после kill, запрет rollback/клонирования подписывающей базы.

### E6 — экономика, Sybil-защита и награды

Спецификация до кода: supply curve/genesis allocation, источник наград,
fee burn/redistribution, bond/unbonding, downtime/double-sign, делегирование,
лимиты и attack model. Без обещаний цены/дохода/оплаты за открытый EXE.
Награды определяются консенсусом и проверяются узлами, не endpoint mint или
локальным счётчиком. 1000 копий программы не создают 1000 прав на доход.
Публичная validator identity/bond может раскрывать связи с приватными notes:
эту границу проверять отдельно. V1 issuance=0 после genesis/fee burn сохраняются.
Приёмка: supply с issuance/burn, deterministic payout, no duplicate reward после
restart/replay, Sybil/split-stake/downtime, validator updates и reward budget.

### E7 — инфраструктура и эксплуатация

2–3 reachable seed операторов, Linux headless и Windows app, долгие испытания,
peer store/discovery, verified snapshots/state sync, pruning/archive/rescan,
лимиты диска/трафика и DoS-защита, метрики без секретов, backup/disaster recovery.
Подписанные обновления/rollback, hashes/SBOM/licenses, installer/uninstaller,
трей и opt-in автозапуск/diagnostics. Публичный Explorer может жить у участников;
spending wallet не превращается в публичный backend. NAT/CGNAT: самостоятельные
relays/hole punching отдельно, без обязательного сервиса. Сейчас live seeds нет.

### E8 — выпуск и интеграции

Fuzz/property/recovery/partition tests, независимый крипто/консенсусный аудит,
исправления, воспроизводимая сборка и длительная public testnet до решения о
mainnet. Никакого тихого превращения testnet в реальные деньги.
Merchant API/SDK после стабилизации. Отправка ORVEXA→ORVEXA — обычный перевод;
вывод в банк/другую монету/на биржу требует внешней поддержки/ликвидности и не
появляется от наличия блокчейна. Собственный мост — отдельная модель и аудит.

## Если компьютеры отключаются

| Сценарий | V1 | Целевой BFT после E5 |
| --- | --- | --- |
| Упала обычная реплика | Другие продолжают; нужен доступный authority для подтверждений | Продолжает при кворуме |
| Упал sole authority | Переводы ждут; реплика не наследует его ключ/полномочия | Автоматическая ротация proposer при кворуме |
| Все offline, вернулась одна реплика | Replay сохранённой цепочки, без производства | Синхронизация, commit ждёт >2/3 power |
| Вернулся один validator из четырёх | Не применимо | Одного недостаточно |
| Новый ПК без диска/backup | Общая genesis + история у сохранившего её узла | Плюс verified snapshots/commits; membership себе не выдаёт |
| Потеряны все копии истории | Нужен backup; seed восстанавливает ключ, а не исчезнувшие блоки | То же; потеря quorum keys требует явной recovery/fork координации, не новой самоназначенной власти |

## Первоисточники

- [CometBFT consensus](https://github.com/cometbft/cometbft/blob/main/spec/consensus/consensus.md): proposer rotation, >2/3 power, сетевые/Byzantine assumptions.
- [WAL](https://github.com/cometbft/cometbft/blob/main/spec/consensus/wal.md): recovery; signing state также обязателен.
- [BIP39](https://github.com/bitcoin/bips/blob/master/bip-0039.mediawiki): mnemonic/seed, не полный ORVEXA wallet derivation.
- [Zcash protocol](https://zips.z.cash/protocol/protocol.pdf), [ZIP310](https://zips.z.cash/zip-0310): disclosure анализируется для точного протокола; ZIP310 про Sapling, не доказательство свойств Orchard/Ironwood.
- [Wry](https://docs.rs/wry/0.57.0/wry/): native WebView, Apache-2.0 OR MIT, системный WebView2 в Windows.
