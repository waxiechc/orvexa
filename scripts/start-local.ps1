param(
    [int]$BasePort = 4242,
    [ValidateRange(1, 3)][int]$Nodes = 3,
    [string]$DataDir = 'data\local-web',
    [switch]$SkipBuild,
    [switch]$OpenBrowser,
    [ValidateRange(0, 86400)][int]$RunForSeconds = 0
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$processes = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()
Push-Location $projectRoot
try {
    if ($BasePort -lt 1024 -or ($BasePort + $Nodes - 1) -gt 65535) { throw 'Choose available ports between 1024 and 65535.' }
    if (-not $SkipBuild) {
        & cargo build --workspace --locked
        if ($LASTEXITCODE) { throw 'Native build failed' }
    }
    $webDir = Join-Path $projectRoot 'web\dist'
    if (-not (Test-Path -LiteralPath (Join-Path $webDir 'wasm\orvexa_web_wallet_bg.wasm'))) {
        throw 'Build the browser app first: powershell -File .\scripts\build-web.ps1 -InstallTools'
    }
    New-Item -ItemType Directory -Path $DataDir -Force | Out-Null
    $networkDir = (Resolve-Path -LiteralPath $DataDir).Path
    $cli = Join-Path $projectRoot 'target\debug\orvexa-cli.exe'
    $nodeExecutable = Join-Path $projectRoot 'target\debug\orvexa-node.exe'
    $aWallet = Join-Path $networkDir 'wallet-a.wallet.json'
    $bWallet = Join-Path $networkDir 'wallet-b.wallet.json'
    $genesisPath = Join-Path $networkDir 'genesis.json'
    if (-not (Test-Path -LiteralPath $genesisPath)) {
        if (@(Get-ChildItem -LiteralPath $networkDir -Filter '*.sqlite').Count) {
            throw 'Existing database without its genesis file. Restore that file or choose a new DataDir; existing data will not be replaced.'
        }
    }
    foreach ($walletPath in @($aWallet, $bWallet)) {
        if (-not (Test-Path -LiteralPath $walletPath)) {
            Write-Host "Create $(Split-Path -Leaf $walletPath). Choose a password of at least 12 characters."
            & $cli wallet-new --wallet $walletPath
            if ($LASTEXITCODE) { throw 'Wallet creation failed' }
        }
    }
    $a = (& $cli address --wallet $aWallet).Trim()
    if ($LASTEXITCODE) { throw 'Wallet A is unreadable' }
    if (-not (Test-Path -LiteralPath $genesisPath)) {
        & $cli genesis --authority $a --recipient $a --out $genesisPath
        if ($LASTEXITCODE) { throw 'Genesis creation failed' }
    }
    $genesis = Get-Content -Raw -LiteralPath $genesisPath | ConvertFrom-Json
    if ($genesis.authority -ne $a -or $genesis.recipient -ne $a) {
        throw 'This launcher expects Wallet A to own genesis and be authority. Use manual commands for other configurations, or choose a separate new DataDir.'
    }
    for ($index = 0; $index -lt $Nodes; $index++) {
        $db = Join-Path $networkDir "node-$index.sqlite"
        if (-not (Test-Path -LiteralPath $db)) {
            & $nodeExecutable init --db $db --genesis $genesisPath
            if ($LASTEXITCODE) { throw 'Database initialization failed' }
        }
        $port = $BasePort + $index
        # Launch only hidden child windows; quote arguments containing spaces.
        $arguments = "run --db `"$db`" --listen 127.0.0.1:$port"
        if ($index -eq 0) { $arguments += " --web-dir `"$webDir`"" }
        $process = Start-Process -FilePath $nodeExecutable -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $networkDir "node-$index.stdout.log") -RedirectStandardError (Join-Path $networkDir "node-$index.stderr.log")
        $processes.Add($process)
    }
    $origin = "http://127.0.0.1:$BasePort"
    for ($attempt = 0; $attempt -lt 40; $attempt++) {
        if (@($processes | Where-Object { $_.HasExited }).Count) { throw 'A node exited. Check node stderr logs and whether ports are already occupied.' }
        try { $status = Invoke-RestMethod "$origin/v1/status" -TimeoutSec 1; break } catch { Start-Sleep -Milliseconds 250 }
    }
    if (-not (Get-Variable status -ErrorAction SilentlyContinue)) { throw 'Primary node did not start' }
    $chain = $status.chain_id
    foreach ($index in 0..($Nodes - 1)) {
        $peer = "http://127.0.0.1:$($BasePort + $index)"
        $peerGenesis = Invoke-RestMethod "$peer/v1/genesis" -TimeoutSec 2
        foreach ($field in @('version','network_salt','authority','recipient','supply')) {
            if ($peerGenesis.$field -ne $genesis.$field) { throw 'A database belongs to another genesis. Use separate data directories.' }
        }
    }
    Write-Host "`nORVEXA Wallet & Explorer: $origin"
    Write-Host "Wallet A (funded / authority): $aWallet"
    Write-Host "Wallet B: $bWallet"
    Write-Host "Chain ID: $chain"
    Write-Host 'Import these encrypted files in Wallets using the passwords you chose.'
    Write-Host 'Produce blocks in the browser using Wallet A. No faucet or automatic minting.'
    Write-Host "$Nodes process(es). Peers pull from node 0 every two seconds. Ctrl+C stops only these child processes."
    if ($OpenBrowser) { Start-Process $origin }
    $lastError = ''
    $started = [Diagnostics.Stopwatch]::StartNew()
    while ($RunForSeconds -eq 0 -or $started.Elapsed.TotalSeconds -lt $RunForSeconds) {
        if (@($processes | Where-Object { $_.HasExited }).Count) { throw 'A child node stopped. Check its stderr log.' }
        try {
            for ($index = 1; $index -lt $Nodes; $index++) {
                $peer = "http://127.0.0.1:$($BasePort + $index)"
                $syncOutput = & $cli --node $peer --expect-chain $chain sync --peer $origin 2>&1
                if ($LASTEXITCODE) { throw "Peer $index sync failed: $syncOutput" }
            }
            $lastError = ''
        } catch {
            if ($_.Exception.Message -ne $lastError) { Write-Warning $_.Exception.Message; $lastError = $_.Exception.Message }
        }
        Start-Sleep -Seconds 2
    }
} finally {
    foreach ($process in $processes) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    Pop-Location
}
