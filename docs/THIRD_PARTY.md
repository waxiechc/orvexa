# Лицензии стороннего ПО

[English](THIRD_PARTY.en.md). Исходники ORVEXA — MIT, см. LICENSE. В Windows ZIP
включён `third-party-licenses/` с исходными upstream notices и индексом точных
версий Cargo/npm. В индекс входят и build/test зависимости; это не утверждение,
что каждая библиотека слинкована в каждом бинарном файле. Версии закреплены в
Cargo.lock и web/package-lock.json.

Для Windows выбирается enabled граф Windows x64 + Wasm, для Mac — запрошенные
Apple targets + Wasm; невыбранные optional протоколы исключаются. Нативный P2P
использует закреплённый libp2p 0.56.0 (MIT), Noise/Yamux через upstream.
Desktop — Wry/Tao, macOS меню — Muda. Windows WebView2 устанавливается отдельно;
Mac использует системный WebKit. Ассеты приложения и notices встроены в EXE.

Некоторые опубликованные crates не включают root-лицензии. Для них
`docs/third-party-notices/` содержит документы точных upstream commits,
source.json фиксирует URL/хеши и проверяется сборщиком. Если standalone notice
отсутствует и в исходном root (base256emoji, base45, if-watch), сохранены
Cargo declaration/authors и явно помеченная SPDX MIT reference с оригинальными
placeholders; авторские сведения не выдумываются. Для Yamux 0.13.10 выбран
release tag commit поддерживаемого репозитория с совпадающим manifest.
Обновление notices нужно только при сборке, не как runtime сетевой сервис.

Также используются RustCrypto Argon2/XChaCha20-Poly1305, ed25519-dalek, Tokio,
Axum, Reqwest, SQLite/rusqlite, wasm-bindgen, React, Vite, Lucide, qrcode и
json-bigint. Интерфейс встраивает шрифт Inter 4.0 (SIL OFL 1.1, подмножество
латиница + кириллица): `web/src/fonts/LICENSE-Inter.txt`. subtle 2.6.1 сравнивает credential, не меняет transaction crypto.
SQLite — public domain; Rust bindings имеют собственные лицензии.

**Юридические тексты лицензий сохранены в оригинале**, без подмены переводом.
Старый opt-in VPN friends-профиль использует отдельно установленный Tailscale,
не входящий в программу. Его сервис имеет собственные условия; native P2P не
использует его. [Условия](https://tailscale.com/pricing),
[шифрование](https://tailscale.com/docs/reference/connection-types).
Проверяйте право использования выбранного плана. VPN не делает v1 анонимной
и не добавляет финальность консенсуса.
