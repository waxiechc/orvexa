# Кошелёк и обозреватель ORVEXA v1

[English](WEB_WALLET.en.md). Основной язык — русский; RU/EN в шапке сохраняет
выбор локально. Интерфейс использует настоящий backend Rust. Переводы
**прозрачные**: адреса, суммы и связи публичны. Только тестовые атомы без реальных
средств. Приватный дизайн v2 не активирован.

Для готового Windows EXE читайте [Desktop](DESKTOP_APP.md), для нативного Mac —
[MACOS.md](MACOS.md), для отдельных компьютеров без сервисов —
[NATIVE_P2P.md](NATIVE_P2P.md). Ниже сохранён воспроизводимый локальный сценарий
трёх процессов; это pull-репликация, отдельная от нативного P2P.

## Сборка и запуск Windows PowerShell

Нужны Rust 1.93+, Node.js 24+, npm и Windows Rust build tools. Интернет нужен
для сборочных зависимостей; готовый интерфейс не использует CDN, внешние шрифты,
телеметрию или удалённый сервис подписи.

Из корня клонированного репозитория:

```powershell
cargo build --workspace --locked
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\build-web.ps1 -InstallTools
powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -OpenBrowser
```

Отдельный toolchain Rust 1.93.0/wasm32 и wasm-bindgen-cli 0.2.129 закреплены.
При следующих сборках можно не указывать `-InstallTools`. Если узлы уже запущены
из `target/debug`, используйте отдельный target-dir для сборки, не перезаписывая
занятые EXE. Не останавливайте чужие процессы.

При первом запуске выберите и повторите пароли A и B, минимум 12 символов.
Пароли не передаются аргументами, в логи, на сервер и не сохраняются сценарием.
Откройте `http://127.0.0.1:4242`; нужен точный числовой origin, не `localhost`.

Три скрытых процесса используют разные SQLite на портах 4242–4244. Первый
обслуживает интерфейс; launcher каждые 2 секунды копирует блоки/pending на два
других. Ctrl+C завершает только собственные дочерние процессы запуска.

Каталог `data/local-web`: кошельки `wallet-a.wallet.json` и `wallet-b.wallet.json`,
общая `genesis.json`, `node-0.sqlite`…`node-2.sqlite` и диагностические логи без
секретов. A — получатель genesis и authority именно в этом legacy сценарии.
Для реалистичной сети ключи расходов и автоматического authority разделяются.
Существующие данные не сбрасываются. Параметры `-BasePort 4342`,
`-DataDir data\another-network`, `-Nodes 1` задают отдельные варианты. Изменённый
порт — другой origin и другое локальное хранилище, понадобится импорт копий.

## Проверка двух кошельков

1. **Кошельки → Восстановить кошелёк**. Выберите
   `data\local-web\wallet-a.wallet.json`, задайте название A и пароль A.
   Скачайте зашифрованную копию и продолжите. Повторите для B с его паролем.
2. Выберите A в шапке. На обзоре 1 000 000 тестовых атомов, у B — 0.
   Это начальное распределение; крана, admin balance или последующего mint нет.
   Создание нового кошелька не создаёт монеты.
3. У B откройте **Получить**, скопируйте адрес. Выберите A → **Отправить**,
   вставьте адрес B, сумма **250**, комиссия **2**, проверьте и подпишите
   паролем A. Принятие означает **Ожидает**, не подтверждение.
4. **Открыть в обозревателе** показывает настоящий хеш и 0 подтверждений.
   Нажмите **Создать блок authority**, введите пароль A. Кнопка **Создать блок**
   на обзоре также работает. Сервер не хранит authority spending password.
5. Блок 1: **A = 999 748, B = 250, сожжённые комиссии = 2**. Найдите хеш
   в обозревателе. Также доступны положительная высота и публичный адрес v1.
6. От B отправьте **40** обратно A с комиссией **1**, пароль B. Следующий
   блок подписывает A. Высота 2: **A = 999 788, B = 209, комиссии = 3**.
   Балансы плюс сожжённые комиссии равны 1 000 000. Реплики догонят при опросе.
7. Остановите launcher через Ctrl+C и запустите снова с тем же каталогом:

   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\start-local.ps1 -SkipBuild -OpenBrowser
   ```

   Кошельки/genesis не пересоздаются. Подтверждённые блоки, хеши и балансы
   восстанавливаются из SQLite. Pending временный и может исчезнуть. Неизвестный
   хеш никогда не считается подтверждённым.

Проверка чтением из другого терминала:

```powershell
$a = .\target\debug\orvexa-cli.exe address --wallet .\data\local-web\wallet-a.wallet.json
$b = .\target\debug\orvexa-cli.exe address --wallet .\data\local-web\wallet-b.wallet.json
Invoke-RestMethod "http://127.0.0.1:4242/v1/balance/$a"
Invoke-RestMethod "http://127.0.0.1:4242/v1/balance/$b"
4242..4244 | ForEach-Object { Invoke-RestMethod "http://127.0.0.1:$_/v1/status" } |
    Select-Object chain_id,height,tip,burned_fees,pending
```

Существующая ручная база:

```powershell
.\target\debug\orvexa-node.exe run --db .\data\node-a.sqlite --listen 127.0.0.1:4242 --web-dir .\web\dist
```

Без `--web-dir` сохраняется исходная CLI-only политика Origin. Транзакции,
домены, адреса, консенсус, схема и экономика v1 не изменены.

## Безопасность и ограничения

`orvexa-web-wallet` использует исходные Rust core/wallet в отдельном новом
worker на операцию. После завершения worker уничтожается; разблокированная
сессия не сохраняется. На node отправляются только signed tx/block, не
wallet/seed/password. В localStorage только encrypted v1 контейнер, адрес и
метки. **Это не backup.** Пароли очищаются из полей, но гарантированно стереть
все копии JavaScript/browser/ОС нельзя. Malware, расширения и подмена кода опасны.

Формат прежний: Argon2id 64 МиБ / 3 прохода / 1 lane, XChaCha20-Poly1305 и
аутентифицированные public metadata. Восстановление проверяет пароль до
сохранения. **Резервная копия** скачивает зашифрованный файл и проверяет его
локально. Файл браузера можно проверить native CLI:

```powershell
.\target\debug\orvexa-cli.exe wallet-verify --wallet .\data\local-web\wallet-a.wallet.json
```

В v1 нет mnemonic. Файл + пароль восстанавливают ключ/адрес, названия локальны,
история берётся из выбранной цепочки. Сверяйте chain ID и резервную копию.
Первый проверенный genesis закрепляет сеть; смена отключает подпись до
явного решения в настройках. Host/Origin/fetch-site, CSP и запрет framing/CORS
не защищают от локального malware. Это не аудированный light client.

`/v1/ui-state` получает согласованные tip/balance/history/mempool под одной
блокировкой. Суммы/счётчики — десятичные строки/BigInt. История — последние 256
переводов, страницы блоков — по 8. [API](API.md). Для приватности нужен отдельный
адаптер, scanning и recovery; прозрачный address lookup не используется вместо них.

## Проверки

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
Set-Location web
npm.cmd run build
npm.cmd run test:unit
npm.cmd exec -- playwright install chromium
npm.cmd test
```

Тесты используют настоящие Rust/SQLite процессы, Wasm-подпись A → B → A,
включение, restart, native-compatible backup, неправильные пароли, неизвестные
хеши, chain pin, мобильную навигацию и русский/английский. Отдельная явная
Windows проверка включает `ORVEXA_NATIVE_GUI_TEST=1` и запускает WebView2.
[Результаты](VALIDATION.md), [бренд](BRAND.md).
