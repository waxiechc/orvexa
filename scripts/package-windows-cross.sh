#!/bin/bash
# Builds the Windows x64 desktop release on a Mac (or Linux) with cargo-xwin.
# Same output as scripts/package-desktop.ps1: static-CRT MSVC ORVEXA.exe with
# the web app embedded, a portable ZIP, SHA256SUMS.txt and build.json.
# The EXE is cross-compiled: run it on real Windows before handing it out.
set -euo pipefail
cd "$(dirname "$0")/.."
version="${ORVEXA_VERSION:-0.1.0-desktop-preview.4}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]] || { echo 'Invalid version' >&2; exit 1; }
target=x86_64-pc-windows-msvc

for command in cargo rustup node npm wasm-bindgen; do command -v "$command" >/dev/null || { echo "Missing build tool: $command" >&2; exit 1; }; done
[[ "$(wasm-bindgen --version)" == 'wasm-bindgen 0.2.129' ]] || { echo 'Install wasm-bindgen-cli 0.2.129 --locked' >&2; exit 1; }
node -e 'if (Number(process.versions.node.split(".")[0]) < 24) throw Error("Node.js 24+ is required")'

# cargo-xwin drives clang-cl/lld-link; Apple's clang has neither, Homebrew LLVM does.
if [[ "$(uname -s)" == Darwin ]]; then
  command -v brew >/dev/null || { echo 'Homebrew is needed for LLVM: https://brew.sh' >&2; exit 1; }
  # Homebrew ships lld-link in its own "lld" formula, separate from "llvm".
  for formula in llvm lld; do brew list "$formula" >/dev/null 2>&1 || brew install "$formula"; done
  export PATH="$(brew --prefix llvm)/bin:$(brew --prefix lld)/bin:$PATH"
fi
for command in clang-cl lld-link llvm-lib; do command -v "$command" >/dev/null || { echo "Missing LLVM tool: $command" >&2; exit 1; }; done
command -v cargo-xwin >/dev/null || cargo install cargo-xwin --locked

# cargo-xwin downloads Microsoft's CRT and Windows SDK. Their license must be
# accepted by the person building, not silently by this script.
if [[ "${XWIN_ACCEPT_LICENSE:-}" != 1 ]]; then
  echo
  echo 'Для сборки Windows cargo-xwin скачает Microsoft CRT и Windows SDK.'
  echo 'Их лицензия: https://go.microsoft.com/fwlink/?LinkId=2086102'
  read -r -p 'Принять лицензию Microsoft и продолжить? Введите yes: ' answer
  [[ "$answer" == yes ]] || { echo 'Сборка Windows отменена.'; exit 1; }
  export XWIN_ACCEPT_LICENSE=1
fi

rustup target add "$target"
rustup toolchain install 1.93.0 --profile minimal --target wasm32-unknown-unknown
cargo fetch --locked
(cd web && npm ci --ignore-scripts)

stamp="$(date +%Y%m%d-%H%M%S)-$RANDOM"
output="$PWD/release/windows-$version-$stamp"
stage="$output/app"
mkdir -p "$stage"
node scripts/collect-licenses.mjs "$stage/third-party-licenses" "$target" wasm32-unknown-unknown
cargo +1.93.0 build -p orvexa-web-wallet --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir web/public/wasm --out-name orvexa_web_wallet target/wasm32-unknown-unknown/release/orvexa_web_wallet.wasm
(cd web && npm run build)

RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static" ORVEXA_DESKTOP_WEB_DIR="$PWD/web/dist" \
  cargo xwin build --release --locked --target "$target" --target-dir target/windows \
  -p orvexa-desktop -p orvexa-node -p orvexa-cli

exe_source="target/windows/$target/release/orvexa-desktop.exe"
# PE header check: machine type must be x64 (0x8664).
node -e '
const b = require("fs").readFileSync(process.argv[1]);
const pe = b.readInt32LE(0x3c);
if (b.toString("latin1", pe, pe + 4) !== "PE\0\0" || b.readUInt16LE(pe + 4) !== 0x8664) throw Error("Expected Windows x64 executable");
' "$exe_source"

cp "$exe_source" "$stage/ORVEXA.exe"
cp LICENSE "$stage/"
for name in DESKTOP_APP.md PLATFORM_PLAN.md NATIVE_P2P.md PRIVACY_DESIGN.md; do cp "docs/$name" "$stage/"; done
cp docs/DESKTOP_START.txt "$stage/START-HERE.txt"
if find "$stage" -type f | grep -Eiq '\.wallet\.json$|\.peerkey$|\.secret$|\.sqlite|/profile\.json$|/genesis\.json$|/network\.json$'; then
  echo 'Refusing to package operator or user data' >&2; exit 1
fi
archive="$output/ORVEXA-Desktop-Windows-x64.zip"
(cd "$stage" && zip -qr -X "$archive" .)
cp "$stage/ORVEXA.exe" "$output/ORVEXA.exe"
(cd "$output" && shasum -a 256 ORVEXA.exe "$(basename "$archive")" > SHA256SUMS.txt)
node -e '
const fs = require("fs"); const [out, version, exe, zip] = process.argv.slice(1);
fs.writeFileSync(out + "/build.json", JSON.stringify({ version, exe_bytes: fs.statSync(exe).size, zip_bytes: fs.statSync(zip).size,
  embedded_web: true, configuration: "Release x64 static CRT (cross-compiled with cargo-xwin)", languages: ["ru", "en"],
  macos_download_available: false, privacy: "transparent v1", validator_quorum: false, rewards: false, windows_runtime_verified: false }, null, 2) + "\n");
' "$output" "$version" "$output/ORVEXA.exe" "$archive"
echo "Windows release directory: $output"
echo "Portable app: $output/ORVEXA.exe"
echo "ZIP: $archive"
echo 'Cross-compiled on this computer. Check it on a real Windows PC before sharing.'
