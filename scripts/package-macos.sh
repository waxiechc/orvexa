#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)" == Darwin ]] || { echo 'Run this script on a Mac with Xcode Command Line Tools.' >&2; exit 1; }
[[ "$(sw_vers -productVersion | cut -d. -f1)" -ge 14 ]] || { echo 'macOS 14+ is required.' >&2; exit 1; }
version="${ORVEXA_VERSION:-0.1.0-desktop-preview.4}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.-]+)?$ ]] || { echo 'Invalid version' >&2; exit 1; }
arch="${1:-universal}"
case "$arch" in
  universal) targets=(aarch64-apple-darwin x86_64-apple-darwin); label=universal ;;
  arm64) targets=(aarch64-apple-darwin); label=arm64 ;;
  x64) targets=(x86_64-apple-darwin); label=x64 ;;
  *) echo 'Usage: bash scripts/package-macos.sh [universal|arm64|x64]' >&2; exit 1 ;;
esac
for command in cargo rustup node npm wasm-bindgen xcrun; do command -v "$command" >/dev/null || { echo "Missing build tool: $command" >&2; exit 1; }; done
[[ "$(wasm-bindgen --version)" == 'wasm-bindgen 0.2.129' ]] || { echo 'Install wasm-bindgen-cli 0.2.129 --locked' >&2; exit 1; }
xcrun --find clang >/dev/null
node -e 'if (Number(process.versions.node.split(".")[0]) < 24) throw Error("Node.js 24+ is required")'
rustup target add "${targets[@]}"
rustup toolchain install 1.93.0 --profile minimal --target wasm32-unknown-unknown
cargo fetch --locked
(cd web && npm ci --ignore-scripts)
output="$PWD/release/macos-$version-$(date +%Y%m%d-%H%M%S)-$RANDOM"
app="$output/ORVEXA.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
node scripts/collect-licenses.mjs "$app/Contents/Resources/third-party-licenses" "${targets[@]}" wasm32-unknown-unknown
cargo +1.93.0 build -p orvexa-web-wallet --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir web/public/wasm --out-name orvexa_web_wallet target/wasm32-unknown-unknown/release/orvexa_web_wallet.wasm
(cd web && npm run build)
export ORVEXA_DESKTOP_WEB_DIR="$PWD/web/dist"
export MACOSX_DEPLOYMENT_TARGET=14.0
binaries=()
for target in "${targets[@]}"; do
  cargo build --release --locked --target "$target" -p orvexa-desktop
  binaries+=("$PWD/target/$target/release/orvexa-desktop")
done
if [[ "$arch" == universal ]]; then
  xcrun lipo -create "${binaries[@]}" -output "$app/Contents/MacOS/ORVEXA"
else
  cp "${binaries[0]}" "$app/Contents/MacOS/ORVEXA"
fi
chmod 755 "$app/Contents/MacOS/ORVEXA"
iconset="$output/ORVEXA.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" docs/brand/orvexa-app-icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" docs/brand/orvexa-app-icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/ORVEXA.icns"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>ORVEXA</string>
<key>CFBundleDisplayName</key><string>ORVEXA</string>
<key>CFBundleIdentifier</key><string>org.orvexa.desktop</string>
<key>CFBundleExecutable</key><string>ORVEXA</string>
<key>CFBundleIconFile</key><string>ORVEXA.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>CFBundleVersion</key><string>4</string>
<key>ORVEXAPreviewVersion</key><string>$version</string>
<key>LSMinimumSystemVersion</key><string>14.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
<key>NSLocalNetworkUsageDescription</key><string>ORVEXA соединяется с участниками выбранной тестовой сети для проверки и обмена блоками.</string>
<key>NSAppTransportSecurity</key><dict>
<key>NSAllowsLocalNetworking</key><true/>
<key>NSExceptionDomains</key><dict>
<key>127.0.0.1</key><dict><key>NSExceptionAllowsInsecureHTTPLoads</key><true/></dict>
<key>::1</key><dict><key>NSExceptionAllowsInsecureHTTPLoads</key><true/></dict>
</dict></dict>
</dict></plist>
EOF
cp LICENSE "$app/Contents/Resources/"
cp docs/DESKTOP_APP.md docs/MACOS.md LICENSE "$output/"
plutil -lint "$app/Contents/Info.plist"
# Ad-hoc signature is for bundle integrity, NOT Developer ID/notarization.
# Supply an existing identity explicitly for a Developer ID build.
codesign --force --sign "${ORVEXA_SIGN_IDENTITY:--}" "$app"
codesign --verify --strict "$app"
xcrun lipo -info "$app/Contents/MacOS/ORVEXA"
archive="$output/ORVEXA-macOS-$label.zip"
staging="$output/archive"
mkdir -p "$staging"
ditto "$app" "$staging/ORVEXA.app"
cp "$output/MACOS.md" "$output/DESKTOP_APP.md" "$output/LICENSE" "$staging/"
(cd "$staging" && shasum -a 256 ORVEXA.app/Contents/MacOS/ORVEXA > SHA256SUMS-app.txt)
ditto -c -k --sequesterRsrc "$staging" "$archive"
(cd "$output" && shasum -a 256 "$(basename "$archive")" > SHA256SUMS-macOS.txt)
echo "App: $app"
echo "Archive: $archive"
echo 'This build is not notarized unless you separately notarize it. No public network is supplied.'
