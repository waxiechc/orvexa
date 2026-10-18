param([int]$BasePort = 50242, [string]$BinaryDir = 'target\friends\debug')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$runDirectory = Join-Path $projectRoot ('demo-data\friends-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
$cli = Join-Path (Join-Path $projectRoot $BinaryDir) 'orvexa-cli.exe'
$node = Join-Path (Join-Path $projectRoot $BinaryDir) 'orvexa-node.exe'
$password = [guid]::NewGuid().ToString('N') + [guid]::NewGuid().ToString('N')
$processes = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()

function Invoke-Cli {
    param([string[]]$Arguments, [switch]$Unlock)
    if ($Unlock) { $output = $password | & $cli @Arguments } else { $output = & $cli @Arguments }
    if ($LASTEXITCODE) { throw 'Test CLI operation failed' }
    return ($output -join "`n")
}
function Status([int]$Index) { Invoke-RestMethod "http://127.0.0.1:$($BasePort + $Index)/v1/status" -TimeoutSec 1 }
function Wait-Height([int]$Index, [int]$Height) {
    for ($attempt = 0; $attempt -lt 150; $attempt++) {
        if ((Status $Index).height -eq $Height) { return }
        Start-Sleep -Milliseconds 100
    }
    throw "Node $Index failed to reach height $Height"
}
function Start-Peer([int]$Index, [string]$Chain, [switch]$Authority) {
    $arguments = @('run','--db',(Join-Path $runDirectory "$Index.sqlite"),'--listen',"127.0.0.1:$($BasePort + $Index)",
        '--mesh-listen',"127.0.0.1:$($BasePort + 10 + $Index)",'--mesh-token-file',(Join-Path $runDirectory 'network\mesh.secret'),
        '--expect-chain',$Chain,'--evidence-dir',(Join-Path $runDirectory "evidence-$Index"),'--sync-every','1','--block-every','1')
    foreach ($peerIndex in 0..2) { if ($peerIndex -ne $Index) { $arguments += @('--peer',"http://127.0.0.1:$($BasePort + 10 + $peerIndex)") } }
    if ($Authority) { $arguments += @('--authority-wallet',(Join-Path $runDirectory 'authority.wallet.json'),'--password-stdin') }
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $node
    $info.Arguments = ($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.WindowStyle = [Diagnostics.ProcessWindowStyle]::Hidden
    $info.RedirectStandardInput = $true
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $info
    [void]$process.Start()
    $processes.Add($process)
    if ($Authority) { $process.StandardInput.WriteLine($password) }
    $process.StandardInput.Close()
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($process.HasExited) { throw "Node $Index exited during startup" }
        try { $status = Status $Index; if ($status.chain_id -eq $Chain) { return $process } } catch { }
        Start-Sleep -Milliseconds 100
    }
    throw "Node $Index did not start"
}

Push-Location $projectRoot
try {
    if ($BasePort -lt 1024 -or $BasePort + 12 -gt 65535) { throw 'Invalid base port' }
    New-Item -ItemType Directory -Path $runDirectory | Out-Null
    $authority = Invoke-Cli @('wallet-new','--wallet',(Join-Path $runDirectory 'authority.wallet.json'),'--password-stdin') -Unlock
    $owner = Invoke-Cli @('wallet-new','--wallet',(Join-Path $runDirectory 'owner.wallet.json'),'--password-stdin') -Unlock
    $bob = Invoke-Cli @('wallet-new','--wallet',(Join-Path $runDirectory 'bob.wallet.json'),'--password-stdin') -Unlock
    & (Join-Path $PSScriptRoot 'new-testnet.ps1') -AuthorityAddress $authority -DataDir (Join-Path $runDirectory 'network') -OwnerWallet (Join-Path $runDirectory 'owner.wallet.json')
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $runDirectory 'network\network.json') | ConvertFrom-Json
    $chain = $manifest.chain_id
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead((Join-Path $runDirectory 'network\join-network.zip'))
    try {
        $names = @($archive.Entries | ForEach-Object { $_.FullName } | Sort-Object)
        if (($names -join ',') -ne 'genesis.json,mesh.secret,network.json') { throw 'Join bundle contains unexpected files' }
    } finally { $archive.Dispose() }
    foreach ($index in 0..2) {
        & $node init --db (Join-Path $runDirectory "$index.sqlite") --genesis (Join-Path $runDirectory 'network\genesis.json') | Out-Null
        if ($LASTEXITCODE) { throw 'Test node initialization failed' }
    }
    $a = Start-Peer 0 $chain
    $b = Start-Peer 1 $chain -Authority
    $c = Start-Peer 2 $chain
    $first = (Invoke-Cli @('--node',"http://127.0.0.1:$BasePort",'--expect-chain',$chain,'send','--wallet',(Join-Path $runDirectory 'owner.wallet.json'),'--to',$bob,'--amount','250','--fee','2','--password-stdin') -Unlock) | ConvertFrom-Json
    foreach ($index in 0..2) { Wait-Height $index 1 }
    $a.Kill(); $a.WaitForExit()
    $second = (Invoke-Cli @('--node',"http://127.0.0.1:$($BasePort + 2)",'--expect-chain',$chain,'send','--wallet',(Join-Path $runDirectory 'bob.wallet.json'),'--to',$owner,'--amount','40','--fee','1','--password-stdin') -Unlock) | ConvertFrom-Json
    Wait-Height 1 2; Wait-Height 2 2
    $a = Start-Peer 0 $chain
    Wait-Height 0 2
    $b.Kill(); $b.WaitForExit()
    Invoke-Cli @('--node',"http://127.0.0.1:$($BasePort + 2)",'--expect-chain',$chain,'send','--wallet',(Join-Path $runDirectory 'bob.wallet.json'),'--to',$owner,'--amount','10','--fee','1','--password-stdin') -Unlock | Out-Null
    Start-Sleep -Milliseconds 1500
    if ((Status 0).height -ne 2 -or (Status 2).height -ne 2) { throw 'Replica created a block without its authority' }
    $b = Start-Peer 1 $chain -Authority
    foreach ($index in 0..2) { Wait-Height $index 3 }
    $statuses = @(0..2 | ForEach-Object { Status $_ })
    if (@($statuses.tip | Sort-Object -Unique).Count -ne 1) { throw 'Nodes disagree after reconnect' }
    $balance = Invoke-RestMethod "http://127.0.0.1:$BasePort/v1/balance/$bob"
    if ($balance.confirmed.balance -ne 198 -or $statuses[0].burned_fees -ne 4) { throw 'Incorrect persisted balances/fees' }
    $result = [ordered]@{ result = 'PASS'; transport_test = 'three local OS processes; real cross-computer VPN not exercised'; chain_id = $chain; tip = $statuses[0].tip; height = 3; bob_balance = 198; burned_fees = 4; first_tx = $first.id; second_tx = $second.id; owner_offline_transfer_confirmed = $true; owner_restart_caught_up = $true; authority_offline_waited_pending = $true; authority_restart_confirmed_pending = $true; join_bundle_only_expected_files = $true; active_process_ids = @($a.Id,$b.Id,$c.Id) }
    [IO.File]::WriteAllText((Join-Path $runDirectory 'result.json'), ($result | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    $result | ConvertTo-Json -Depth 5
    Write-Host "Evidence: $runDirectory"
} finally {
    foreach ($process in $processes) {
        if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }
        $process.Dispose()
    }
    $password = $null
    Pop-Location
}
