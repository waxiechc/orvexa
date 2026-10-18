param(
    [Parameter(Mandatory = $true)][string]$AuthorityAddress,
    [string]$DataDir = 'data\friends-testnet',
    [string]$OwnerWallet = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'testnet-common.ps1')
Push-Location $script:OrvexaRoot
try {
    $cli = Get-OrvexaBinary 'orvexa-cli'
    $node = Get-OrvexaBinary 'orvexa-node'
    if (Test-Path -LiteralPath $DataDir) {
        if (@(Get-ChildItem -LiteralPath $DataDir -Force).Count) {
            throw 'Choose a new empty DataDir. Existing networks and wallets will not be changed.'
        }
    } else { New-Item -ItemType Directory -Path $DataDir | Out-Null }
    $networkDir = (Resolve-Path -LiteralPath $DataDir).Path
    $ownerWalletPath = Join-Path $networkDir 'owner.wallet.json'
    if ($OwnerWallet) {
        Copy-Item -LiteralPath (Resolve-Path -LiteralPath $OwnerWallet).Path -Destination $ownerWalletPath
    } else {
        Write-Host 'Create the funded owner wallet. Keep its password and encrypted backup yourself.'
        & $cli wallet-new --wallet $ownerWalletPath
        if ($LASTEXITCODE) { throw 'Owner wallet creation failed' }
    }
    $owner = (& $cli address --wallet $ownerWalletPath).Trim()
    if ($LASTEXITCODE) { throw 'Cannot read owner wallet' }
    if ($AuthorityAddress -eq $owner) { throw 'Authority must use a separate key, not the funded owner wallet.' }
    $genesisPath = Join-Path $networkDir 'genesis.json'
    $genesisResult = & $cli genesis --authority $AuthorityAddress --recipient $owner --out $genesisPath
    if ($LASTEXITCODE) { throw 'Genesis creation failed. Check the authority public address.' }
    $genesis = ($genesisResult -join "`n") | ConvertFrom-Json
    $tokenPath = Join-Path $networkDir 'mesh.secret'
    & $node mesh-token --out $tokenPath
    if ($LASTEXITCODE) { throw 'Mesh credential creation failed' }
    $manifestPath = Join-Path $networkDir 'network.json'
    $manifest = [ordered]@{ profile = 'orvexa-friends-v1'; chain_id = $genesis.chain_id; authority = $AuthorityAddress; recipient = $owner; supply_test_atoms = 1000000 }
    Write-OrvexaUtf8 $manifestPath ($manifest | ConvertTo-Json)
    $join = Join-Path $networkDir 'join'
    New-Item -ItemType Directory -Path $join | Out-Null
    # Deliberate allowlist: never copy a wallet, password, database or logs.
    foreach ($name in @('genesis.json', 'network.json', 'mesh.secret')) {
        Copy-Item -LiteralPath (Join-Path $networkDir $name) -Destination (Join-Path $join $name)
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = Join-Path $networkDir 'join-network.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($join, $archive)
    Write-Host "`nORVEXA friends testnet created. Chain ID: $($genesis.chain_id)"
    Write-Host "Join bundle for invited node operators: $archive"
    Write-Host 'It contains a shared access credential. Send privately, and verify the chain ID separately.'
    Write-Host "Keep this funded wallet yourself: $ownerWalletPath"
    Write-Host 'The authority operator keeps their own encrypted authority wallet; its key has no genesis allocation.'
    Write-Host 'No existing local v1 network was migrated or reset.'
} finally { Pop-Location }
