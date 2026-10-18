param([int]$BasePort = 56242, [string]$BinaryDir = 'target\friends\release')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$runDirectory = Join-Path $projectRoot ('demo-data\p2p-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
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
        '--p2p-listen',"/ip4/127.0.0.1/tcp/$($BasePort + 10 + $Index)",'--p2p-identity',(Join-Path $runDirectory "$Index.peerkey"),'--allow-private-peers',
        '--expect-chain',$Chain,'--evidence-dir',(Join-Path $runDirectory "evidence-$Index"),'--sync-every','1','--block-every','1')
    foreach ($peerIndex in 0..2) { if ($peerIndex -ne $Index) { $arguments += @('--p2p-seed',$script:peerAddresses[$peerIndex]) } }
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
    $networkDir = Join-Path $runDirectory 'network'
    New-Item -ItemType Directory -Path $networkDir | Out-Null
    $result = Invoke-Cli @('genesis','--authority',$authority,'--recipient',$owner,'--out',(Join-Path $networkDir 'genesis.json')) | ConvertFrom-Json
    $chain = $result.chain_id
    $script:peerAddresses = @(0..2 | ForEach-Object {
        $id = (& $node peer-id --identity (Join-Path $runDirectory "$_.peerkey")).Trim()
        if ($LASTEXITCODE) { throw 'Peer identity creation failed' }
        "/ip4/127.0.0.1/tcp/$($BasePort + 10 + $_)/p2p/$id"
    })
    $manifest = [ordered]@{profile='orvexa-p2p-v1';chain_id=$chain;seeds=@($script:peerAddresses[1])}
    [IO.File]::WriteAllText((Join-Path $networkDir 'network.json'),($manifest|ConvertTo-Json -Depth 3),[Text.UTF8Encoding]::new($false))
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $joinArchive = Join-Path $runDirectory 'join-network.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($networkDir,$joinArchive)
    $archive = [IO.Compression.ZipFile]::OpenRead($joinArchive)
    try {
        $names = @($archive.Entries | ForEach-Object { $_.FullName } | Sort-Object)
        if (($names -join ',') -ne 'genesis.json,network.json') { throw 'Public join bundle contains unexpected files' }
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
    $firstDetails = Invoke-RestMethod "http://127.0.0.1:$BasePort/v1/transactions/$($first.id)"
    if ($firstDetails.status -ne 'confirmed' -or $firstDetails.confirmations -ne 3) { throw 'Explorer did not report the actual confirmed transfer' }
    $runtime = Invoke-RestMethod "http://127.0.0.1:$BasePort/v1/runtime"
    if ($runtime.transport -ne 'libp2p-tcp-noise') { throw 'Native transport not active' }
    $result = [ordered]@{ result = 'PASS'; transport_test = 'three local OS processes over native TCP/Noise; public Internet not exercised'; chain_id = $chain; tip = $statuses[0].tip; height = 3; bob_balance = 198; burned_fees = 4; first_tx = $first.id; second_tx = $second.id; owner_offline_transfer_confirmed = $true; owner_restart_caught_up = $true; authority_offline_waited_pending = $true; authority_restart_confirmed_pending = $true; join_bundle_only_two_public_files = $true; explorer_confirmations = $firstDetails.confirmations; active_process_ids = @($a.Id,$b.Id,$c.Id) }
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
