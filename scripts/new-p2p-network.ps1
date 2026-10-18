param(
    [Parameter(Mandatory = $true)][string]$PublicAddress,
    [ValidateRange(1024,65535)][int]$PeerPort = 4243,
    [string]$DataDir = 'data\p2p-operator',
    [switch]$AllowPrivatePeers
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'testnet-common.ps1')
Push-Location $script:OrvexaRoot
try {
    $ip = [Net.IPAddress]::Parse($PublicAddress)
    $node = Get-OrvexaBinary 'orvexa-node' -NativeP2P
    $cli = Get-OrvexaBinary 'orvexa-cli' -NativeP2P
    if (Test-Path -LiteralPath $DataDir) {
        if (@(Get-ChildItem -LiteralPath $DataDir -Force).Count) { throw 'Choose a new empty operator directory; existing networks will not be reset.' }
    } else { New-Item -ItemType Directory -Path $DataDir | Out-Null }
    $directory = (Resolve-Path -LiteralPath $DataDir).Path
    $identity = Join-Path $directory 'identity.peerkey'
    $peerId = (& $node peer-id --identity $identity).Trim()
    if ($LASTEXITCODE) { throw 'Peer identity creation failed' }
    $kind = if ($ip.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetwork) { 'ip4' } else { 'ip6' }
    $seed = "/$kind/$PublicAddress/tcp/$PeerPort/p2p/$peerId"
    $validation = @('peer-address','--address',$seed)
    if ($AllowPrivatePeers) { $validation += '--allow-private-peers' }
    & $node @validation
    if ($LASTEXITCODE) { throw 'Invalid public seed address; no wallets or genesis were created.' }
    Write-Host 'Create the separate block authority wallet. Keep this key on exactly one signing machine.'
    $authorityWallet = Join-Path $directory 'authority.wallet.json'
    & $cli wallet-new --wallet $authorityWallet
    if ($LASTEXITCODE) { throw 'Authority wallet creation failed' }
    $authority = (& $cli address --wallet $authorityWallet).Trim()
    Write-Host 'Create the funded owner wallet. Do not distribute it or load it as the node authority.'
    $ownerWallet = Join-Path $directory 'owner.wallet.json'
    & $cli wallet-new --wallet $ownerWallet
    if ($LASTEXITCODE) { throw 'Owner wallet creation failed' }
    $owner = (& $cli address --wallet $ownerWallet).Trim()
    $genesisPath = Join-Path $directory 'genesis.json'
    $result = & $cli genesis --authority $authority --recipient $owner --out $genesisPath
    if ($LASTEXITCODE) { throw 'Genesis creation failed' }
    $chain = (($result -join "`n") | ConvertFrom-Json).chain_id
    # Create the actual pinned database before publishing the public join pack.
    $db = Join-Path $directory 'node.sqlite'
    & $node init --db $db --genesis $genesisPath
    if ($LASTEXITCODE) { throw 'Initial database creation failed' }
    $manifest = [ordered]@{profile='orvexa-p2p-v1';chain_id=$chain;seeds=@($seed)}
    Write-OrvexaUtf8 (Join-Path $directory 'network.json') ($manifest | ConvertTo-Json -Depth 3)
    $join = Join-Path $directory 'network'
    New-Item -ItemType Directory -Path $join | Out-Null
    foreach ($name in @('genesis.json','network.json')) { Copy-Item -LiteralPath (Join-Path $directory $name) -Destination $join }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = Join-Path $directory 'join-network.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($join,$archive)
    Write-Host "Public join pack (two public files only): $archive"
    Write-Host "Seed: $seed"
    Write-Host "Pinned chain: $chain"
    Write-Host "Authority wallet and peer identity stay in $directory. Never distribute this whole directory."
    Write-Host 'This command creates a NEW explicit test network. It does not migrate any existing v1 chain.'
    Write-Host 'Run start-p2p.ps1 with this NetworkDir/DataDir, AuthorityWallet and Announce set to the seed above.'
    if ($AllowPrivatePeers) { Write-Host 'Private-address exercise: also pass -AllowPrivatePeers to every launcher. This is not an Internet deployment.' }
} finally { Pop-Location }
