param(
    [string]$NetworkDir = 'network',
    [string]$DataDir = 'data\p2p-node',
    [string]$AuthorityWallet = '',
    [ValidateRange(1024,65535)][int]$UiPort = 4242,
    [ValidateRange(1024,65535)][int]$PeerPort = 4243,
    [string[]]$Seeds = @(),
    [string[]]$Announce = @(),
    [switch]$AllowPrivatePeers,
    [switch]$OpenBrowser
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'testnet-common.ps1')
Push-Location $script:OrvexaRoot
try {
    $node = Get-OrvexaBinary 'orvexa-node' -NativeP2P
    if (-not (Test-Path -LiteralPath (Join-Path $NetworkDir 'network.json'))) {
        throw 'No common network configured. Copy the operator-provided public genesis.json and network.json into network/. A join never creates its own chain. Read docs/NATIVE_P2P.md.'
    }
    $networkPath = (Resolve-Path -LiteralPath $NetworkDir).Path
    $manifest = Get-Content -LiteralPath (Join-Path $networkPath 'network.json') -Raw | ConvertFrom-Json
    if ($manifest.profile -ne 'orvexa-p2p-v1' -or $manifest.chain_id -notmatch '^[0-9a-f]{64}$') { throw 'Invalid native P2P manifest' }
    $seedAddresses = @(if ($Seeds.Count) { $Seeds } else { $manifest.seeds })
    if (-not $AuthorityWallet -and $seedAddresses.Count -eq 0) { throw 'A replica needs an actual reachable seed address from the network operator.' }
    if ($seedAddresses.Count -gt 32) { throw 'At most 32 seed addresses are supported.' }
    if ($UiPort -eq $PeerPort) { throw 'Use different UI and peer ports.' }
    New-Item -ItemType Directory -Path $DataDir -Force | Out-Null
    $nodeDir = (Resolve-Path -LiteralPath $DataDir).Path
    $db = Join-Path $nodeDir 'node.sqlite'
    if (-not (Test-Path -LiteralPath $db)) {
        & $node init --db $db --genesis (Join-Path $networkPath 'genesis.json')
        if ($LASTEXITCODE) { throw 'Database initialization failed' }
    }
    $arguments = @('run','--db',$db,'--listen',"127.0.0.1:$UiPort",'--expect-chain',$manifest.chain_id,
        '--p2p-listen',"/ip4/0.0.0.0/tcp/$PeerPort",'--p2p-identity',(Join-Path $nodeDir 'identity.peerkey'),
        '--evidence-dir',(Join-Path $nodeDir 'evidence'))
    $webDir = Join-Path $script:OrvexaRoot 'web\dist'
    if (Test-Path -LiteralPath (Join-Path $webDir 'index.html')) { $arguments += @('--web-dir',$webDir) }
    foreach ($address in $seedAddresses) { $arguments += @('--p2p-seed', $address) }
    foreach ($address in $Announce) { $arguments += @('--p2p-announce', $address) }
    if ($AllowPrivatePeers) { $arguments += '--allow-private-peers' }
    if ($AuthorityWallet) { $arguments += @('--authority-wallet', (Resolve-Path -LiteralPath $AuthorityWallet).Path) }
    Write-Host "ORVEXA native TCP/Noise | chain $($manifest.chain_id)"
    Write-Host "Wallet & Explorer on this computer: http://127.0.0.1:$UiPort"
    Write-Host 'No VPN, account or shared access token. Keep this window open; Ctrl+C stops the node.'
    Write-Host 'Replicas verify/store/relay. One separate authority confirms transfers. Test atoms only.'
    Write-Host 'Inbound participation needs TCP port forwarding/firewall permission. Outbound syncing needs no open inbound port.'
    if ($OpenBrowser) { Start-Process "http://127.0.0.1:$UiPort" }
    & $node @arguments
    if ($LASTEXITCODE) { throw 'Node stopped with an error; data was not reset.' }
} finally { Pop-Location }
