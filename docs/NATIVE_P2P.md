# ORVEXA: узлы без внешних сервисов

Нативная **тестовая** сеть: Rust libp2p, TCP/Noise/Yamux, автоматическая
синхронизация, локальный Wallet & Explorer. VPN, аккаунт, подписка, Docker,
Rust/Node.js на компьютере получателя Windows ZIP не требуются. Криптографические
библиотеки входят в программу; во время работы нет обращений к GitHub, SPDX,
сервисам обнаружения IP или чужим bootstrap-серверам.

Программа бесплатна. Электричество, интернет и постоянно работающие машины
предоставляют участники. Платный VPS не обязателен. Нужен хотя бы один стартовый
узел с доступным **TCP 4243**: публичный IP сервера или домашнего ПК с пробросом
порта. Желательны два независимых доступных узла. Обычный участник за NAT может
подключаться исходящим соединением; при закрытом входящем порте он не станет
доступным bootstrap для остальных. CGNAT требует другого доступного участника
или решения со стороны провайдера; программа не обходит его автоматически.
Проверьте входящую доступность со второй внешней сети. Автоматически менять
роутер, системный firewall или устанавливать службу launcher не будет.

**На 2026-10-18 публичный IP/сервер не предоставлен, живые seeds отсутствуют.**
Собранный архив содержит программу, а не работающую в интернете инфраструктуру.
Сто компьютеров должны присоединяться к одной genesis; сто отдельно созданных
genesis дадут сто разных сетей. Программа присоединения никогда сама не создаёт
новую genesis. Общие параметры выбираются один раз оператором сети.

## Что делают узлы

Каждый проверяет подписи, nonce, баланс, комиссию, supply, parent и порядок
блоков; хранит проверенную цепочку в SQLite; распространяет подписанные переводы
и блоки, догоняет цепочку после отключения. Транспорт зашифрован и сверяет PeerId.
HTTP API и браузерный кошелёк доступны только на `127.0.0.1:4242`.

Блоки по-прежнему подписывает **один отдельный авторитет**, определённый genesis.
Уход компьютера владельца средств не мешает переводам, пока авторитет доступен.
Уход единственного авторитета останавливает подтверждения: остальные узлы не
становятся авторами блоков автоматически. Вознаграждений за запуск узла, PoW,
PoS, кворума и защиты от Sybil/eclipse здесь нет. При двух конфликтующих
подписанных заголовках узел сохраняет `evidence/authority-conflict.json` и
останавливает приём/создание блоков даже после перезапуска. Не удаляйте evidence
для продолжения работы без расследования; ветка автоматически не выбирается.

Суммы, адреса, комиссии и связи отправителя **публичны**. Это прозрачная v1 без
реальных денег и независимого аудита. IP/PeerId видны сетевым участникам.
Не запускайте одновременно две копии authority на разных базах. Один authority
не делает сеть децентрализованным консенсусом; это отдельный следующий этап.

## Участнику: Windows, запуск одним файлом

1. Распаковать `ORVEXA-native-p2p-testnet-windows-x64-*.zip` в доступную для записи
   папку. Получить от оператора два **публичных** файла `genesis.json` и
   `network.json`, положить их в `network/` рядом с `bin/`, `scripts/` и
   `START-ORVEXA.cmd`. Если оператор собрал ZIP с `-NetworkDir`, они уже внутри.
   Сверить chain ID с оператором через независимый канал.
2. Дважды нажать `START-ORVEXA.cmd`. Он запускает один узел, создаёт отдельный
   транспортный ключ и базу в `data/p2p-node/`, открывает Wallet & Explorer.
   При отсутствии сетевых файлов появится объяснение, а не новая случайная сеть.
3. В Wallets создать свой кошелёк и сохранить зашифрованный backup и пароль.
   Он начинает с нулём. Для тестовых монет передать оператору **только адрес**:
   владелец genesis может перевести часть своей фиксированной тестовой эмиссии.
   Автоматического крана и майнинга нет. Ключи/пароли не передаются серверу.
4. Оставить окно работающим. Перезапуск того же файла проверяет сохранённые блоки
   и восстанавливает соединения. При остановке всех узлов цепочка сохранится на
   дисках, но обработки переводов не будет. Pending находится в памяти: если
   отключить все хранящие его узлы, подписанную транзакцию нужно отправить снова.

Точная команда вместо двойного клика, из папки распакованного приложения:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-p2p.ps1 -OpenBrowser
.\bin\orvexa-cli.exe network
.\bin\orvexa-cli.exe status
```

Входящий публичный TCP-порт нужен, чтобы принимать новых соседей. Исходящее
подключение к доступному seed уже позволяет получать/проверять цепочку и
передавать свои переводы. Никогда не пробрасывайте UI-порт 4242 в интернет.

## Оператор: создать общую тестовую сеть один раз

Выполнять только когда есть **ваш настоящий** публичный IP, назначенный вашей
машине/роутеру. Следующая команда намеренно создаёт новую отдельную testnet,
существующие кошельки/цепочки v1 не изменяет. Она запросит пароли authority и
funded owner; не хранит пароли. Не используйте owner как authority.

```powershell
$publicIp = Read-Host 'Actual public IP of your reachable node'
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\new-p2p-network.ps1 -PublicAddress $publicIp
$manifest = Get-Content .\data\p2p-operator\network.json -Raw | ConvertFrom-Json
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-p2p.ps1 `
  -NetworkDir .\data\p2p-operator -DataDir .\data\p2p-operator `
  -AuthorityWallet .\data\p2p-operator\authority.wallet.json `
  -Announce $manifest.seeds[0] -OpenBrowser
```

Одинаковый DataDir сохраняет PeerId, указанный в seeds. Ввести пароль authority
локально при запуске. Для IPv6 поддержан native CLI `/ip6/.../tcp/4243`; Windows
launcher по умолчанию слушает IPv4, поэтому для IPv6 используйте native команду
с `--p2p-listen /ip6/::/tcp/4243` и соответствующим announce.

Раздать участникам только `data/p2p-operator/join-network.zip` (ровно два публичных
файла) и архив программы. Никогда не раздавать весь каталог оператора: в нём
authority, funded owner, transport identity и база. В кошельке владельца можно
локально импортировать `owner.wallet.json` и распределять тестовые атомы.

Второй доступный узел генерирует **свою** identity, узнаёт PeerId из `network`,
публикует свой numeric multiaddr `/ip4/REAL_IP/tcp/4243/p2p/REAL_PEER_ID` через
`-Announce` и добавляется в `network.json` seeds для новых участников.
Существующие соединённые узлы обмениваются ограниченной страницей таких адресов.
Адреса не являются голосами или авторитетами; блоки всё равно проверяются.

Существующая v1 genesis также поддерживается: скопировать её без изменений,
указать **её** chain ID и нужные seeds в манифесте `orvexa-p2p-v1`, инициализировать
новую реплику через `init` или использовать уже существующую базу. Автоматическому
authority нужен отдельный от funded recipient ключ; для старой genesis, где они
совпадают, сохраняется прежний ручной режим, автоматический запуск отклоняется.

## Linux: самостоятельный сервер без VPN

В Linux-бинарник Windows ZIP не превращается. Сборка исходников требует Rust
1.93+ и C toolchain для SQLite/ring; Linux здесь описан, на этой Windows-машине
не был исполнен. CI уже содержит проверку Ubuntu.

```bash
cargo build --release --locked -p orvexa-node -p orvexa-cli
mkdir -p data/p2p-node
target/release/orvexa-node init --db data/p2p-node/node.sqlite --genesis network/genesis.json
# Set these to the ACTUAL public values from the common network manifest.
read -r -p 'Chain ID: ' CHAIN_ID
read -r -p 'Seed multiaddr: ' SEED
target/release/orvexa-node run --db data/p2p-node/node.sqlite \
  --listen 127.0.0.1:4242 --expect-chain "$CHAIN_ID" \
  --p2p-listen /ip4/0.0.0.0/tcp/4243 \
  --p2p-identity data/p2p-node/identity.peerkey --p2p-seed "$SEED"
```

При повторном запуске пропустить `init`. Для authority добавить
`--authority-wallet /absolute/path/authority.wallet.json`; ввод пароля идёт в
локальный терминал, не в аргументы/URL. Автозапуск authority после reboot без
ручного ввода и безопасное хранение секретов для службы пока не реализованы.
Для обычной реплики systemd может перезапускать ту же команду `run`: фиксированные
абсолютные пути, отдельный непривилегированный пользователь, Restart=on-failure,
WorkingDirectory проекта. Секреты кошельков реплике не нужны.

## Проверки и сборка архива

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\demo-p2p.ps1
# Windows x64 static CRT, compiled Wallet/Explorer, licenses, no user keys:
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-testnet.ps1 -NativeP2P
# Once YOUR common public network exists, bake its two public files into a joinable app:
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\package-testnet.ps1 `
  -NativeP2P -SkipBuild -NetworkDir .\data\p2p-operator
```

`cargo test` включает 100 настоящих локальных TCP/Noise узлов (один authority,
99 реплик): перевод с последней реплики достигает authority, все получают
одинаковый tip/баланс. Это не 100 реальных интернет-участников и не гарантия
публичной DoS-устойчивости. `demo-p2p.ps1` запускает три отдельных Windows-процесса,
проверяет A→B→A, недоступность владельца, catch-up, отключение/перезапуск authority,
баланс B=198, burned=4 и поиск первого tx в Explorer API (3 подтверждения).
Одноразовые пароли существуют только в памяти/stdin; demo-data не включается в ZIP.

Следующие этапы: внешние операторы и длительное сетевое испытание; безопасная
эксплуатация authority; проектирование нескольких производителей блоков и
кворума отдельно от транспорта; приватный ledger отдельно от прозрачной v1.
Транспортное шифрование не скрывает публичную историю монет.
