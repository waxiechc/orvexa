# Проверки ORVEXA — 2026-10-18

[История и предыдущие результаты на английском](VALIDATION.en.md).
Текущий выпуск — `v0.1.0-desktop-preview.4`, русский основной / English опционально.

## preview.4 — выполнено на Mac (Apple M4, macOS 27.0.1)

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
  --all-features --locked -- -D warnings`: успешно.
- `cargo test --workspace --locked`: **48 пройдено**.
- `npm run build`, `npm run test:unit`: успешно. `npx playwright test`:
  **6 пройдено**, 1 пропущен (нативное окно Windows), включая новый тест
  состояния «Синхронизация».
- macOS universal: `verify-macos-release.mjs` — PASS; окно `ORVEXA.app`
  открыто, узел в сети. Подробности и непроверенное — [MACOS.md](MACOS.md).
- Windows x64: собран кросс-компиляцией `scripts/package-windows-cross.sh`
  (cargo-xwin, Homebrew LLVM + lld, static CRT), проверен PE-заголовок x64.
  **На настоящем Windows не запускался.**

```text
e4344ac021bab70ed82d1c2358855f5dc349c3555d1b14c274cb14b30a08e26d  ORVEXA-macOS-universal.zip
9aaf34de86f147ee4b1162d74dcce727dfbaf5565ffcb57a5be647d3dba9aede  ORVEXA.exe
0578180f76303f841e2b84f282c18d5d146e0dde63b73ab918e777c533edf408  ORVEXA-Desktop-Windows-x64.zip
```

GitHub CI для этого коммита не запускался (блокировка биллинга аккаунта);
результаты выше — локальные.

## preview.2 — выполнено на Windows x64

- `cargo fmt --all -- --check`: успешно.
- `cargo check --workspace --locked`: успешно.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: успешно.
- `cargo test --workspace --locked`: **48 пройдено**, включая 100 настоящих
  локальных TCP/Noise-узлов, несовместимую сеть, повреждённые подписи, переполнения,
  повторные траты, durable restart и постоянный quarantine.
- Rust 1.93.0 Wasm / wasm-bindgen 0.2.129 / TypeScript/Vite production: успешно.
- `npm run test:unit`: **2 пройдено**, точность u64 и опасные JSON объекты.
- `npm test` с Release binaries, `ORVEXA_NATIVE_GUI_TEST=1` и
  `ORVEXA_TEST_EMBEDDED=1`: **6 пройдено**. Последний прогон — 37,5 секунды.
- Настоящий desktop controller: общий genesis/manifest, live peer, локальная
  подпись, перевод, authority confirmation, Explorer, stop/resume с прежним tip.
- RU: создание кошелька, несовпадающие пароли, encrypted backup, открытие в CLI,
  восстановление в другом профиле, все страницы, RU/EN + reload и сохранение имени.
- Настоящее окно WebView2: русский по умолчанию, английский после переключения,
  создание encrypted wallet через встроенный Rust Wasm.
- Отдельный тест A → B → A с настоящими ID, балансами, комиссиями и restart.

Протоколы и снимки:
`demo-data/desktop-e2e-hneCOW/`, включая `result.json`, `native-window-result.json`,
`native-window-russian.png`, `node-russian-desktop.png`, `node-russian-mobile.png`.
В этих каталогах локальные одноразовые тестовые данные; они не публикуются в Git.

## Артефакт и сайт

`release/desktop-20261018-133658-945295/ORVEXA.exe`: 13 044 736 байт.
ZIP: 5 553 429 байт. Точные SHA-256 опубликованы в release `SHA256SUMS.txt`:

```text
41d46e9de58bd3b683b80edf5ff5738c007ace7eece964e50fad6163895af737  ORVEXA.exe
f81dd041660e3bc4863e2bd73ec82145b67ea2af84b21bd922a43d1200247b23  ORVEXA-Desktop-Windows-x64.zip
```
Протокол конфиденциальности — прежняя прозрачная v1, rewards/validator quorum выключены.

`node scripts/verify-desktop-release.mjs release/desktop-20261018-133658-945295`:
успешно. ZIP распакован, EXE запущен из своей папки без исходного web/dist,
встроенный Wasm и текст лицензий доступны, SHA совпадают. Остановленный ledger
возвращает 503. RU/EN сайт, переход языка, метаданные, keyboard skip link,
широкий/мобильный экран, отсутствие горизонтального overflow и ошибок ассетов:
успешно. Протокол: `demo-data/desktop-release-u4JelA/result.json`.

## macOS: что проверено и что нет

Сценарий `scripts/package-macos.sh` проходит Bash syntax check на этом хосте.
Portable license collector проходит для обоих Apple targets + Wasm: **358 пакетов**
в данной локальной установке npm. Точное число зависит от platform npm artifacts;
индекс каждой настоящей сборки записывается отдельно. Недостающие Objective-C
upstream notices восстановлены по точным commit/hash.

**Mac binary, универсальный Mach-O, окно WKWebView, профиль, загрузки, clipboard,
Gatekeeper, notarization, CPU/RAM/батарея не проверены на этом Windows-хосте.**
Готовый Mac-архив не публикуется как рабочий. Отдельный workflow `macOS native preview`
предназначен для сборки и headless smoke; ручная проверка окна всё равно нужна.
[Сценарий Mac](MACOS.md).

GitHub CI и явный [запуск Mac workflow](https://github.com/waxiechc/orvexa/actions/runs/39120181102)
не стартовали: GitHub сообщил `The job was not started because your account is
locked due to a billing issue.` Проверок на удалённом Mac не было. Ограничение
требует действий владельца аккаунта или отдельного Mac-хоста, не смены протокола.
Локальный успех Windows не объявляется успехом удалённого CI.

Русский сайт и английская страница опубликованы на GitHub Pages; deployment
успешен. EXE и ZIP скачаны заново из публичного release preview.2; их SHA-256
совпадают с локальными проверенными файлами.

## Границы результатов

Нет live публичной сети/seeds, проверки независимыми интернет-операторами,
permissionless consensus, failover authority, rewards, mnemonic, private ledger,
независимого аудита, installer/tray/OS-autostart. Закрытие preview останавливает
узел. Original v1 совместим, криптография и экономика не изменены.
