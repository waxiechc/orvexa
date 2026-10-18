param([switch]$SkipBuild, [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$')][string]$Version = '0.1.0-desktop-preview.5')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,6)
    $output = Join-Path $projectRoot "release\desktop-$stamp"
    $stage = Join-Path $output 'app'
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    & (Join-Path $PSScriptRoot 'collect-licenses.ps1') -OutputDir (Join-Path $stage 'third-party-licenses') -Target x86_64-pc-windows-msvc -IncludeWasm
    if (-not $SkipBuild) {
        $notices = [Text.StringBuilder]::new()
        [void]$notices.AppendLine('ORVEXA open-source notices. Includes selected Windows/Wasm build and test dependencies.')
        [void]$notices.AppendLine([IO.File]::ReadAllText((Join-Path $projectRoot 'LICENSE')))
        [void]$notices.AppendLine("`n--- font/Inter ---`n")
        [void]$notices.AppendLine([IO.File]::ReadAllText((Join-Path $projectRoot 'web\src\fonts\LICENSE-Inter.txt')))
        foreach ($file in Get-ChildItem -LiteralPath (Join-Path $stage 'third-party-licenses') -Recurse -File | Sort-Object FullName) {
            [void]$notices.AppendLine("`n--- $($file.Directory.Name)/$($file.Name) ---`n")
            [void]$notices.AppendLine([IO.File]::ReadAllText($file.FullName))
        }
        [IO.File]::WriteAllText((Join-Path $projectRoot 'web\public\open-source-notices.txt'),$notices.ToString(),[Text.UTF8Encoding]::new($false))
        & (Join-Path $PSScriptRoot 'build-web.ps1')
        if ($LASTEXITCODE) { throw 'Web build failed' }
        $savedFlags = [Environment]::GetEnvironmentVariable('RUSTFLAGS','Process')
        $savedWeb = [Environment]::GetEnvironmentVariable('ORVEXA_DESKTOP_WEB_DIR','Process')
        try {
            $env:RUSTFLAGS = (($savedFlags + ' -C target-feature=+crt-static').Trim())
            $env:ORVEXA_DESKTOP_WEB_DIR = (Resolve-Path -LiteralPath 'web\dist').Path
            $env:ORVEXA_RELEASE = $Version
            & cargo build --release --locked --target-dir target/friends -p orvexa-desktop -p orvexa-node -p orvexa-cli
            if ($LASTEXITCODE) { throw 'Desktop build failed' }
        } finally {
            [Environment]::SetEnvironmentVariable('RUSTFLAGS',$savedFlags,'Process')
            [Environment]::SetEnvironmentVariable('ORVEXA_DESKTOP_WEB_DIR',$savedWeb,'Process')
        }
    }
    $source = Join-Path $projectRoot 'target\friends\release\orvexa-desktop.exe'
    $bytes = [IO.File]::ReadAllBytes($source)
    $peOffset = [BitConverter]::ToInt32($bytes,0x3c)
    if ([BitConverter]::ToUInt16($bytes,$peOffset+4) -ne 0x8664) { throw 'Expected Windows x64 executable' }
    Copy-Item -LiteralPath $source -Destination (Join-Path $stage 'ORVEXA.exe')
    Copy-Item -LiteralPath 'LICENSE' -Destination $stage
    foreach ($name in @('DESKTOP_APP.md','PLATFORM_PLAN.md','NATIVE_P2P.md','PRIVACY_DESIGN.md')) { Copy-Item -LiteralPath "docs\$name" -Destination $stage }
    Copy-Item -LiteralPath 'docs\DESKTOP_START.txt' -Destination (Join-Path $stage 'START-HERE.txt')
    $private = @(Get-ChildItem -LiteralPath $stage -Recurse -File | Where-Object {$_.Name -match '\.wallet\.json$|\.peerkey$|\.secret$|\.sqlite|^profile\.json$|^genesis\.json$|^network\.json$'})
    if ($private.Count) { throw 'Refusing to package operator or user data' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = Join-Path $output 'ORVEXA-Desktop-Windows-x64.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($stage,$archive)
    $exe = Join-Path $output 'ORVEXA.exe'
    Copy-Item -LiteralPath (Join-Path $stage 'ORVEXA.exe') -Destination $exe
    $lines = @(Get-FileHash -LiteralPath $exe,$archive -Algorithm SHA256 | ForEach-Object {"$($_.Hash.ToLowerInvariant())  $(Split-Path -Leaf $_.Path)"})
    [IO.File]::WriteAllText((Join-Path $output 'SHA256SUMS.txt'),($lines -join "`n")+"`n",[Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $output 'build.json'),([ordered]@{version=$Version;exe_bytes=(Get-Item -LiteralPath $exe).Length;zip_bytes=(Get-Item -LiteralPath $archive).Length;embedded_web=$true;configuration='Release x64 static CRT';languages=@('ru','en');macos_download_available=$false;privacy='transparent v1';validator_quorum=$false;rewards=$false}|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
    Write-Host "Desktop release directory: $output"
    Write-Host "Portable app: $exe"
    Write-Host "ZIP: $archive"
} finally { Pop-Location }
