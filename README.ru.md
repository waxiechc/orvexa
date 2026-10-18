# ORVEXA

[Сайт и скачивание Windows-приложения](https://waxiechc.github.io/orvexa/) ·
[Инструкция Desktop](docs/DESKTOP_APP.md) · [Поэтапный план](docs/PLATFORM_PLAN.md)

Новый Desktop preview объединяет Wallet, Explorer и настоящий P2P full node в
одном EXE. Публичная сеть ещё не запущена: для подключения нужны общие сетевые
файлы и доступный узел оператора. Автоматического валидатора и наград пока нет.

**Работающий исследовательский прототип собственной сети на Rust.**
Цель проекта — PRIVATE BY DESIGN. Текущая v0.1 **прозрачная**: адреса, суммы,
комиссии и история видны. Только локальные тестовые атомы, без реальных денег.
ORVEXA — предварительное рабочее название; товарные знаки, домены и названия
криптовалютных проектов ещё не проверены. Тикер также не утверждён.

Добавлены **English Wallet & Explorer** с реальным узлом: Dashboard, Send,
Receive/QR, Transactions, Wallets, Backup & Restore и Settings. Подпись выполняет
Rust WebAssembly в браузере; пароли и ключи узлу не передаются. SVG реконструирован
из выбранного `ORVEXA.png`, концепция сохранена, палитра — холодная тёмно-синяя.

**Основной новый режим — [native P2P](docs/NATIVE_P2P.md):** прямые TCP/Noise
соединения без VPN, регистрации и подписок; Windows ZIP с `START-ORVEXA.cmd`.
Участники используют одну genesis и доступные узлы, которые держат сами.
Публичного bootstrap пока нет: его IP ещё не предоставлен. Сто локальных узлов
проверяются тестом; это не запущенная интернет-сеть. Авторитет пока один.

**Также сохранён friends testnet для отдельных компьютеров:** автоматическая
синхронизация через закрытую VPN, один отдельный авторитет с автосозданием блоков,
зашифрованные пользовательские кошельки на своих устройствах и Windows ZIP без
необходимости устанавливать Rust/Node.js у друзей. Узел владельца может быть
выключен, пока работает назначенный авторитет и доступны другие участники.
Обычные реплики не заменяют отключённого авторитета. Это прозрачная тестовая
сеть, без многовалидаторного консенсуса и публичного веб-хостинга.
[Пошаговое подключение друзей](docs/FRIENDS_TESTNET.md).

### Запуск Wallet & Explorer в PowerShell

Для браузерной сборки нужен Node.js 24+; Rust CLI работает без Node.js.

```powershell
Set-Location 'C:\Users\waxie\Desktop\void'
cargo build --workspace --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-web.ps1 -InstallTools
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -OpenBrowser
```

При первом запуске задайте пароли двух кошельков. В `http://127.0.0.1:4242`
импортируйте через Wallets → Restore файлы `data\local-web\wallet-a.wallet.json`
и `wallet-b.wallet.json`. A получает 1 000 000 атомов в genesis и подписывает
блоки; B начинает с нуля. Launcher запускает три отдельных узла и синхронизирует
пиры. Отправка остаётся pending до **Produce block** с паролем A.
Проверенный сценарий A → B → A, backup и restart — в
[пошаговой инструкции](docs/WEB_WALLET.md). Приватный протокол не активирован.

Реализованы подписанные транзакции и блоки, nonce против повторного расходования,
проверка общей суммы, зашифрованные кошельки, CLI, loopback HTTP API, SQLite с
проверкой всей цепочки при запуске и явная синхронизация нескольких локальных узлов.
Консенсус — **экспериментальный единичный авторитет**, без децентрализованной
финальности, PoW или автоматического разрешения конфликтующих веток.

Подготовлен [дизайн конфиденциальной v2](docs/PRIVACY_DESIGN.md) и
[план её тестирования](docs/PRIVACY_TEST_PLAN.md). Редакция 2 сравнивает Monero
RingCT/CLSAG и Orchard/Ironwood по семи критериям, включая кошелёк и Explorer.
Рекомендуется shielded-note архитектура; точный стек выбирается после отдельной
проверки upstream. Первый предлагаемый этап — изолированный evaluation workspace,
без адаптера в работающем узле. Приватные переводы, nullifier state и новый
кошелёк пока не реализованы, работающая v1 не мигрируется.

Workspace и исполняемые файлы переименованы в `orvexa-*`. Для совместимости v1
сохраняет прежние адреса `void_test_` и домены хешей/подписей `VOID/...`:
существующие кошельки, genesis, транзакции и базы не требуют миграции.
Полный список файлов и проверок — в [отчёте переименования](docs/RENAME_ORVEXA.md).

## Требования и проверки

Rust/Cargo 1.93+, Windows MSVC Build Tools с Windows SDK либо стандартный Linux
C toolchain для bundled SQLite. Системный SQLite, Docker и облака не нужны.
Node.js нужен только для сборки/тестов UI.
Первичная загрузка бесплатных crates требует интернета; дальнейшая работа локальная.

```powershell
rustc --version
cargo --version
rustup component add rustfmt clippy
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --workspace --locked
```

Восемь Rust-модулей находятся в `crates/`, интерфейс — в `web/`. Зависимости сохранены
в `Cargo.lock`. Форматы v1 ещё не стабильны; данные предназначены для небольших
локальных экспериментов. Код распространяется под MIT.

## Автоматическая демонстрация на Windows

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\demo.ps1
```

Скрипт создаёт отдельную папку `demo-data`, два зашифрованных кошелька, genesis и
три независимых процесса на портах 44242–44244. Он передаёт 250 атомов с комиссией
2, проверяет отклонение подменённой подписи и повторных расходов, синхронизирует
транзакцию и блок, перезапускает узел и сравнивает баланс и tip всех узлов.
Проверяются название сети ORVEXA, chain ID и три разных PID `orvexa-node`.
`result.json` содержит результаты. Процессы завершаются в `finally`.
При занятых портах передайте `-BasePort 45242`. Демо-пароль случайный, существует
только в памяти скрипта и не сохраняется: демо-кошельки одноразовые.

## Ручной запуск

Из корня проекта, после сборки (в Linux уберите `.exe` и замените пути):

```powershell
New-Item -ItemType Directory -Path data
.\target\debug\orvexa-cli.exe wallet-new --wallet data\alice.wallet.json
.\target\debug\orvexa-cli.exe wallet-new --wallet data\bob.wallet.json
$alice = .\target\debug\orvexa-cli.exe address --wallet data\alice.wallet.json
$bob = .\target\debug\orvexa-cli.exe address --wallet data\bob.wallet.json
.\target\debug\orvexa-cli.exe genesis --authority $alice --recipient $alice --out data\genesis.json
.\target\debug\orvexa-node.exe init --db data\node-a.sqlite --genesis data\genesis.json
.\target\debug\orvexa-node.exe run --db data\node-a.sqlite
```

`wallet-new` запрашивает пароль без отображения. Кошелёк защищён Argon2id
(64 MiB, 3 прохода, 1 поток) и XChaCha20-Poly1305 с случайными солью и nonce.
Не менее 12 символов. Сделайте резервную копию зашифрованного файла и сохраните
пароль отдельно. Seed-фраза и её экспорт пока не реализованы. CLI никогда не
принимает пароль аргументом; `--password-stdin` предназначен для автоматизации.

В другом терминале снова получите `$alice` и `$bob` через `address`, затем:

```powershell
.\target\debug\orvexa-cli.exe status
.\target\debug\orvexa-cli.exe balance --address $alice
.\target\debug\orvexa-cli.exe send --wallet data\alice.wallet.json --to $bob --amount 250 --fee 2
.\target\debug\orvexa-cli.exe produce --wallet data\alice.wallet.json
.\target\debug\orvexa-cli.exe balance --address $bob
.\target\debug\orvexa-cli.exe history --address $bob
```

После блока Alice имеет 999748, Bob — 250, сожжено 2 атома. `send` только помещает
транзакцию в pending pool; подтверждение происходит после `produce`. Поле
`pending_nonce` позволяет последовательно отправлять несколько транзакций.
Для закрепления выбранной сети используйте глобальный `--expect-chain <chain_id>`.
Остановка узла — Ctrl+C; повторный `run` восстанавливает подтверждённое состояние.
Неподтверждённые транзакции после перезапуска нужно отправить повторно.

Для отдельного авторитета создайте третий кошелёк, укажите его адрес в
`genesis --authority`, а Alice в `--recipient`; выпускайте блоки этим третьим
кошельком. Узел не хранит ключ авторитета: CLI подписывает блок локально.

## Несколько узлов

Используйте **один и тот же genesis** и разные базы/порты:

```powershell
.\target\debug\orvexa-node.exe init --db data\node-b.sqlite --genesis data\genesis.json
.\target\debug\orvexa-node.exe run --db data\node-b.sqlite --listen 127.0.0.1:4243
```

Третий узел, в отдельном терминале:

```powershell
.\target\debug\orvexa-node.exe init --db data\node-c.sqlite --genesis data\genesis.json
.\target\debug\orvexa-node.exe run --db data\node-c.sqlite --listen 127.0.0.1:4244
```

Из другого терминала:

```powershell
.\target\debug\orvexa-cli.exe --node http://127.0.0.1:4243 sync --peer http://127.0.0.1:4242
.\target\debug\orvexa-cli.exe --node http://127.0.0.1:4244 sync --peer http://127.0.0.1:4242
```

Команда забирает блоки порциями и pending-транзакции; локальный узел проверяет
каждую подпись и переход состояния. Это ручная pull-синхронизация, без gossip,
обнаружения пиров и подключения к интернету. Синхронизация конфликтующей ветки
завершится ошибкой, принятая история не заменяется. Произвольное число локальных
процессов не делает консенсус децентрализованным.

## Документация и ограничения

- [Архитектура, консенсус и тестовая эмиссия](ARCHITECTURE.md)
- [Модель угроз](THREAT_MODEL.md)
- [Исследование приватности и граница интеграции](PRIVACY.md)
- [Сравнение Monero и Orchard/Ironwood, рекомендация и этапы v2](docs/PRIVACY_DESIGN.md)
- [План негативных и восстановительных тестов v2](docs/PRIVACY_TEST_PLAN.md)
- [HTTP API](docs/API.md)
- [Wallet, Explorer и проверка двух кошельков](docs/WEB_WALLET.md)
- [SVG и происхождение логотипа](docs/BRAND.md)
- [Результаты локальных проверок](docs/VALIDATION.md)
- [Аудит переименования](docs/RENAME_AUDIT.md)
- [Следующие этапы](ROADMAP.md)

Нет приватных переводов, seed recovery, публичного P2P, защиты от Sybil/DoS,
доказательства финальности, автоматического fork choice, платёжных ссылок
или внешнего аудита. SQLite восстанавливает подтверждённые блоки; полная потеря
диска требует резервной копии. Сетевые метаданные не защищены. Ключи зашифрованы
на диске, но вредоносная программа на разблокированном устройстве остаётся угрозой.
Нет скрытых комиссий, админ-прав изменения балансов, последующей эмиссии,
телеметрии и платных сервисов.
