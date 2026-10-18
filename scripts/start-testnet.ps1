param(
    [string]$MeshAddress = '',
    [string]$Peers = '',
    [string]$NetworkDir = 'data\friends-testnet',
    [string]$DataDir = 'data\friends-node',
    [string]$AuthorityWallet = '',
    [ValidateRange(1024,65535)][int]$UiPort = 4242,
    [ValidateRange(1024,65535)][int]$MeshPort = 4243,
    [switch]$OpenBrowser
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'testnet-common.ps1')
Push-Location $script:OrvexaRoot
try {
    $node = Get-OrvexaBinary 'orvexa-node'
    if (-not $MeshAddress) {
        $tailscale = Get-Command tailscale -ErrorAction SilentlyContinue
        $tailscalePath = if ($tailscale) { $tailscale.Source } else { Join-Path $env:ProgramFiles 'Tailscale\tailscale.exe' }
        if (-not (Test-Path -LiteralPath $tailscalePath)) { throw 'Install/connect Tailscale first, or pass -MeshAddress with your Tailscale IPv4 address.' }
        $MeshAddress = (& $tailscalePath ip -4 | Select-Object -First 1)
        if ($LASTEXITCODE -or -not $MeshAddress) { throw 'Tailscale has no active IPv4 address. Connect it first.' }
        $MeshAddress = $MeshAddress.Trim()
    }
    $ip = [Net.IPAddress]::Parse($MeshAddress)
    $octets = $ip.GetAddressBytes()
    if ($octets.Length -ne 4 -or (-not [Net.IPAddress]::IsLoopback($ip) -and ($octets[0] -ne 100 -or $octets[1] -lt 64 -or $octets[1] -gt 127))) {
        throw 'Use an assigned Tailscale IPv4 address (100.64.0.0/10). Loopback is allowed only for local process tests.'
    }
    $peerUrls = @($Peers.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    if (-not $AuthorityWallet -and $peerUrls.Count -eq 0) { throw 'A replica needs at least one -Peers URL to synchronize.' }
    if ($peerUrls.Count -gt 8) { throw 'At most eight peers are supported.' }
    $networkPath = (Resolve-Path -LiteralPath $NetworkDir).Path
    $manifest = Get-Content -LiteralPath (Join-Path $networkPath 'network.json') -Raw | ConvertFrom-Json
    if ($manifest.profile -ne 'orvexa-friends-v1' -or $manifest.chain_id -notmatch '^[0-9a-f]{64}$') { throw 'Invalid friends-testnet manifest' }
    $genesis = Join-Path $networkPath 'genesis.json'
    $token = Join-Path $networkPath 'mesh.secret'
    if (-not (Test-Path -LiteralPath $token)) { throw 'Missing mesh.secret from the private join bundle.' }
    $webDir = Join-Path $script:OrvexaRoot 'web\dist'
    if (-not (Test-Path -LiteralPath (Join-Path $webDir 'wasm\orvexa_web_wallet_bg.wasm'))) { throw 'Build the web app first, or use the complete Windows bundle.' }
    New-Item -ItemType Directory -Path $DataDir -Force | Out-Null
    $nodeDir = (Resolve-Path -LiteralPath $DataDir).Path
    $db = Join-Path $nodeDir 'node.sqlite'
    if (-not (Test-Path -LiteralPath $db)) {
        & $node init --db $db --genesis $genesis
        if ($LASTEXITCODE) { throw 'Database initialization failed' }
    }
    $nodeArguments = @('run', '--db', $db, '--listen', "127.0.0.1:$UiPort", '--web-dir', $webDir,
        '--mesh-listen', "${MeshAddress}:$MeshPort", '--mesh-token-file', $token,
        '--expect-chain', $manifest.chain_id, '--evidence-dir', (Join-Path $nodeDir 'evidence'))
    foreach ($peer in $peerUrls) { $nodeArguments += @('--peer', $peer) }
    if ($AuthorityWallet) {
        $authority = (Resolve-Path -LiteralPath $AuthorityWallet).Path
        $nodeArguments += @('--authority-wallet', $authority)
    }
    Write-Host "Wallet & Explorer (this computer): http://127.0.0.1:$UiPort"
    Write-Host "Peer address to give friends: http://${MeshAddress}:$MeshPort"
    Write-Host "Pinned chain: $($manifest.chain_id)"
    Write-Host 'Keep Tailscale connected. The peer port is read-only and has no wallet/signing endpoint.'
    Write-Host 'Exactly one authority may run. If it goes offline, transfers wait pending; replicas never promote themselves.'
    Write-Host 'Ctrl+C stops this node. Re-run the same command to restore its chain and reconnect.'
    if ($OpenBrowser) { Start-Process "http://127.0.0.1:$UiPort" }
    & $node @nodeArguments
    if ($LASTEXITCODE) { throw 'Node stopped with an error; inspect the message above. Data was not reset.' }
} finally { Pop-Location }
