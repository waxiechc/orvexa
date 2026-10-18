param([switch]$InstallTools, [string]$Toolchain = '1.93.0')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if ($InstallTools) {
        & rustup toolchain install $Toolchain --profile minimal --target wasm32-unknown-unknown
        if ($LASTEXITCODE) { throw 'Could not install wasm target' }
        & cargo install wasm-bindgen-cli --version 0.2.129 --locked
        if ($LASTEXITCODE) { throw 'Could not install wasm-bindgen CLI' }
    }
    & cargo "+$Toolchain" build -p orvexa-web-wallet --target wasm32-unknown-unknown --release --locked
    if ($LASTEXITCODE) { throw 'Wasm wallet build failed' }
    & wasm-bindgen --target web --out-dir web/public/wasm --out-name orvexa_web_wallet target/wasm32-unknown-unknown/release/orvexa_web_wallet.wasm
    if ($LASTEXITCODE) { throw 'wasm-bindgen failed; install the pinned CLI version' }
    Push-Location web
    try {
        & npm.cmd ci --ignore-scripts
        if ($LASTEXITCODE) { throw 'npm ci failed' }
        & npm.cmd run build
        if ($LASTEXITCODE) { throw 'Web build failed' }
    } finally { Pop-Location }
} finally { Pop-Location }
