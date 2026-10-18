# HTTP JSON API ORVEXA v1

[English](API.en.md). Схемы, версии и маршруты не переводятся: локализуется
представление. Адреса v1 сохраняют `void_test_` для совместимости с существующими
подписями, genesis и encrypted wallets.

Обычный узел: `http://127.0.0.1:4242`. Desktop: `http://127.0.0.1:4252`.
Это обычный JSON без оболочки JSON-RPC. HTTP API предназначен для числового
loopback, не публичного wallet-сервера. По умолчанию браузерные Origin запрещены;
опциональный web host разрешает только свой точный Host/Origin с CSP.

| Метод | Маршрут | Ввод и результат |
| --- | --- | --- |
| GET | /v1/status | protocol, network, privacy, consensus, chain_id, height, tip, burned_fees, pending |
| GET | /v1/runtime | enabled, role, halted, peers [{url,state,height}], produced_blocks; транспортные наблюдения |
| GET | /v1/genesis | Неизменяемая Genesis |
| GET | /v1/balance/{address} | address, confirmed: {balance, nonce}, pending_nonce; проверяется checksum |
| GET | /v1/history/{address} | Последние 256 подтверждённых {height,id,transaction}; начальное распределение отдельно |
| GET | /v1/mempool | Упорядоченные SignedTransaction, максимум 512 |
| POST | /v1/transactions | SignedTransaction → {id}; принятие во временный mempool |
| GET | /v1/block-template | Неподписанный Block с первыми 128 pending tx |
| POST | /v1/blocks | Подписанный Block → {id} после проверки и durable commit |
| GET | /v1/blocks?from=1 | Высота включительно, по умолчанию 1; до 8 подписанных блоков |
| GET | /v1/transactions/{id} | TransactionDetails по настоящему хешу; 404, если неизвестна |
| GET | /v1/blocks/{height} | Положительная высота; ID, transaction_ids, confirmations, полный блок |
| GET | /v1/explorer/blocks?before=9 | Верхняя высота исключительно, до 8 блоков, сначала новые |
| GET | /v1/explorer/mempool | Pending TransactionDetails с реальными ID |
| GET | /v1/ui-state?address=... | Согласованный status/genesis/blocks/pending и optional balance/history под одной блокировкой |

TransactionDetails: `{id,status,block_height,block_id,confirmations,transaction}`.
Pending не включена в блок и имеет 0 подтверждений; confirmed содержит настоящий
блок и `tip_height - block_height + 1` подтверждений. Это не BFT финальность.
Исчезнувший pending возвращает 404, не успех. Genesis доступна через `/v1/genesis`,
а не как подписанный `/v1/blocks/0`. Пагинации истории пока нет.

Суммы, nonce и высоты — беззнаковые целые. В JavaScript нужен lossless u64 JSON
parser; браузерный адаптер использует десятичные строки/BigInt, включая значения
за пределом точности Number. ID — 64 lowercase hex символа, подпись — 128.
Пароли, seed и закрытые ключи не являются параметрами API.

Подписываются канонические байты Rust, не сериализованный JSON. Структуры:

```text
SignedTransaction = {transaction: {version, chain_id, sender, recipient, amount, fee, nonce}, signature}
Block = {header: {version, chain_id, height, parent, timestamp, transactions_hash}, transactions, signature}
```

Timestamp — строго растущий логический счётчик, не время по часам.
[Канонические кодировки](../ARCHITECTURE.md).

Ошибки проверки/хранения: HTTP 400 с `{ "error": "..." }`. Неверный JSON/schema:
400/422 Axum; большой запрос: 413; запрещённый Origin: 403; отсутствующий путь:
404. Размер запроса/ответа клиента до 2 МиБ, таймаут клиента 10 секунд,
redirect/proxy отключены. Нет production-grade server rate limit/timeouts.
Уже durable принятые блоки сохраняются, если последующая страница/tx ошибочна.

## Локальный контроллер Desktop

Доступен только в desktop host с теми же Host/Origin/CSP, без CORS и удалённых
wallet secrets. Пока узел остановлен, ledger API возвращает 503.

| Метод | Маршрут | Действие |
| --- | --- | --- |
| GET | /v1/desktop/status | Наблюдаемые phase/config/runtime/ledger, ошибки и ограничения роли |
| POST | /v1/desktop/configure | Публичные genesis/network, peer_port, allow_private_peers, resume_on_launch; импорт только при остановленном узле |
| POST | /v1/desktop/start | Пустой объект; запуск проверяющего full node без authority-ключа |
| POST | /v1/desktop/stop | Пустой объект; остановка сетевой задачи и освобождение базы |

В status `validator_active=false`, `rewards_supported=false`: это реальные
границы v1, не ошибка кнопки. Карантин нельзя убрать restart/resume.
Локализация не меняет enum/JSON ключи, а переводит их в интерфейсе.

## Read-only friends mesh

`run --mesh-listen <Tailscale-IP>:4243 --mesh-token-file <file> --expect-chain <id>`
добавляет отдельный аутентифицированный listener. Доступны только GET
`/v1/status`, `/v1/genesis`, `/v1/blocks?from=...`, `/v1/mempool`.
Нет wallet/balance/template/UI/mutation endpoints. Credential передаётся в
`Authorization: Bearer <mesh credential>`, никогда в URL. Любой Origin отвергается.
401 — credential, 404 — маршрут, 405 — метод, 429 — лимит 64 req/s и 16 in-flight.
Peer client: общий таймаут 3 секунды, connect 2 секунды, ответ до 2 МиБ.

Разрешены числовые Tailscale IP и loopback для тестов; произвольные remote
hosts/DNS/LAN/public/wildcard/redirect/proxy запрещены. HTTP требует настоящего
шифрованного VPN — диапазон адреса этого не гарантирует. `Client::new_mesh`
включается явно, default `Client::new` не изменён. Локальный Wallet остаётся loopback.

Mesh role — authority/replica, в default режиме enabled=false. Высота peer —
наблюдение после проверки genesis, не подписанный checkpoint. Подпись ждёт
известную более новую цепочку даже после неудачной загрузки; приглашённый
злонамеренный peer может остановить производство. Runtime counters временные.
Наблюдаемые конфликтующие authority-подписи сохраняют evidence и halted=true;
submit/template/commit отвергаются с 400. Нет fork choice или выборов authority.
[Ограничения](FRIENDS_TESTNET.md).

Нативный P2P — отдельный бинарный транспорт TCP/Noise с ограниченными JSON
сообщениями, а не этот публичный HTTP API. [Дизайн](P2P_DESIGN.md).
