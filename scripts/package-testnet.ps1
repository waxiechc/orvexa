param([ValidateSet('Debug','Release')][string]$Configuration = 'Release', [switch]$SkipBuild, [string]$TargetDir = 'target\friends', [switch]$NativeP2P, [string]$NetworkDir = '')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    if (-not $SkipBuild) {
        $buildArguments = @('build','--locked','--target-dir',$TargetDir,'-p','orvexa-node','-p','orvexa-cli')
        if ($Configuration -eq 'Release') { $buildArguments += '--release' }
        $savedRustFlags = [Environment]::GetEnvironmentVariable('RUSTFLAGS', 'Process')
        try {
            # Bundle the MSVC CRT into the Windows executables. Do not require
            # friends to install the compiler or a separate VC runtime package.
            $env:RUSTFLAGS = (($savedRustFlags + ' -C target-feature=+crt-static').Trim())
            & cargo @buildArguments
            if ($LASTEXITCODE) { throw 'Native build failed' }
        } finally { [Environment]::SetEnvironmentVariable('RUSTFLAGS', $savedRustFlags, 'Process') }
        & (Join-Path $PSScriptRoot 'build-web.ps1')
        if ($LASTEXITCODE) { throw 'Web build failed' }
    }
    $binaryDir = Join-Path (Join-Path $projectRoot $TargetDir) $Configuration.ToLowerInvariant()
    foreach ($name in @('orvexa-node.exe','orvexa-cli.exe')) {
        if (-not (Test-Path -LiteralPath (Join-Path $binaryDir $name))) { throw "Missing $name; build $Configuration first." }
        $executableBytes = [IO.File]::ReadAllBytes((Join-Path $binaryDir $name))
        $peOffset = [BitConverter]::ToInt32($executableBytes, 0x3c)
        if ([BitConverter]::ToUInt16($executableBytes, $peOffset + 4) -ne 0x8664) { throw 'This bundle is for Windows x64 only; build the correct target.' }
    }
    if (-not (Test-Path -LiteralPath 'web\dist\wasm\orvexa_web_wallet_bg.wasm')) { throw 'Build the web app first.' }
    $releaseDir = Join-Path $projectRoot 'release'
    New-Item -ItemType Directory -Path $releaseDir -Force | Out-Null
    $stamp = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,6)
    $stage = Join-Path $releaseDir "bundle-$stamp"
    foreach ($folder in @('bin','scripts','docs','web')) { New-Item -ItemType Directory -Path (Join-Path $stage $folder) | Out-Null }
    foreach ($name in @('orvexa-node.exe','orvexa-cli.exe')) { Copy-Item -LiteralPath (Join-Path $binaryDir $name) -Destination (Join-Path $stage "bin\$name") }
    Copy-Item -LiteralPath 'web\dist' -Destination (Join-Path $stage 'web') -Recurse
    foreach ($name in @('testnet-common.ps1','new-testnet.ps1','start-testnet.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $stage "scripts\$name") }
    if ($NativeP2P) {
        foreach ($name in @('start-p2p.ps1','new-p2p-network.ps1')) { Copy-Item -LiteralPath (Join-Path $PSScriptRoot $name) -Destination (Join-Path $stage "scripts\$name") }
        Copy-Item -LiteralPath (Join-Path $projectRoot 'START-ORVEXA.cmd') -Destination $stage
        foreach ($name in @('NATIVE_P2P.md','P2P_DESIGN.md')) { Copy-Item -LiteralPath (Join-Path $projectRoot "docs\$name") -Destination (Join-Path $stage "docs\$name") }
        New-Item -ItemType Directory -Path (Join-Path $stage 'network') | Out-Null
        if ($NetworkDir) {
            $manifest = Get-Content -LiteralPath (Join-Path $NetworkDir 'network.json') -Raw | ConvertFrom-Json
            if ($manifest.profile -ne 'orvexa-p2p-v1' -or $manifest.chain_id -notmatch '^[0-9a-f]{64}$' -or @($manifest.seeds).Count -eq 0) { throw 'Invalid common P2P network manifest' }
            foreach ($name in @('network.json','genesis.json')) { Copy-Item -LiteralPath (Join-Path $NetworkDir $name) -Destination (Join-Path $stage 'network') }
        }
    } elseif ($NetworkDir) { throw 'NetworkDir requires NativeP2P' }
    foreach ($name in @('FRIENDS_TESTNET.md','THIRD_PARTY.md')) { Copy-Item -LiteralPath (Join-Path $projectRoot "docs\$name") -Destination (Join-Path $stage "docs\$name") }
    Copy-Item -LiteralPath 'LICENSE' -Destination (Join-Path $stage 'LICENSE')
    & (Join-Path $PSScriptRoot 'collect-licenses.ps1') -OutputDir (Join-Path $stage 'third-party-licenses') -Target x86_64-pc-windows-msvc -IncludeWasm
    [IO.File]::WriteAllText((Join-Path $stage 'START-HERE.txt'), "ORVEXA friends testnet / transparent test atoms / one authority.`r`nRead docs/FRIENDS_TESTNET.md. This bundle contains no wallets, network credentials or database.`r`n", [Text.UTF8Encoding]::new($false))
    if ($NativeP2P) {
        [IO.File]::WriteAllText((Join-Path $stage 'START-HERE.txt'), "ORVEXA native P2P / transparent TEST atoms / one authority.`r`nRead docs/NATIVE_P2P.md. Copy the SAME public genesis.json and network.json into network/, then double-click START-ORVEXA.cmd.`r`nNo Rust, Node.js, VPN, account or subscription needed to run this Windows x64 bundle.`r`nNo live bootstrap server is supplied. Operators must provide a reachable seed. The app never invents one or silently creates a chain.`r`n", [Text.UTF8Encoding]::new($false))
    }
    # Verify the complete bundle inventory before compression; no user data paths
    # are copied. This archive is code/assets only, separate from a private join zip.
    $forbidden = @(Get-ChildItem -LiteralPath $stage -Recurse -File | Where-Object { $_.Name -match '\.wallet\.json$|\.secret$|\.peerkey$|\.sqlite' -or ((-not $NativeP2P) -and $_.Name -match '^genesis\.json$|^network\.json$') })
    if ($forbidden.Count) { throw 'Bundle inventory contains forbidden user/network data; refusing to package.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $profile = if ($NativeP2P) { 'native-p2p-testnet' } else { 'friends-testnet' }
    $archive = Join-Path $releaseDir "ORVEXA-$profile-windows-x64-$stamp.zip"
    [IO.Compression.ZipFile]::CreateFromDirectory($stage, $archive)
    $hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    [IO.File]::WriteAllText("$archive.sha256", "$hash  $(Split-Path -Leaf $archive)`n", [Text.UTF8Encoding]::new($false))
    Write-Host "Windows bundle: $archive"
    Write-Host "SHA-256: $hash"
    if ($NativeP2P) { Write-Host 'Native P2P uses only public join files. Never distribute operator wallets, peer identity or passwords.' }
    else { Write-Host 'Send the join-network.zip only to invited peers. Never add wallets/passwords to this app bundle.' }
} finally { Pop-Location }
