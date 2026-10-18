param([int]$BasePort = 44242, [switch]$SkipBuild, [string]$BinaryDir = 'target\debug')
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$runDirectory = Join-Path $projectRoot ('demo-data\' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Path $runDirectory -Force | Out-Null
$cli = Join-Path (Join-Path $projectRoot $BinaryDir) 'orvexa-cli.exe'
$nodeExecutable = Join-Path (Join-Path $projectRoot $BinaryDir) 'orvexa-node.exe'
$expectedNetworkLabel = 'ORVEXA local devnet ' + [char]0x2014 + ' test atoms only'
$testPassword = [guid]::NewGuid().ToString('N') + [guid]::NewGuid().ToString('N')
$processes = [System.Collections.Generic.List[System.Diagnostics.Process]]::new()

function Invoke-OrvexaCli {
    param([string[]]$CommandArgs, [switch]$Unlock)
    if ($Unlock) { $output = $testPassword | & $cli @CommandArgs }
    else { $output = & $cli @CommandArgs }
    if ($LASTEXITCODE -ne 0) { throw "orvexa-cli failed: $($CommandArgs -join ' ')" }
    return ($output -join "`n")
}
function Get-OrvexaStatus {
    param([string]$Url)
    # Windows PowerShell 5.1 otherwise guesses Latin-1 for JSON without charset.
    $response = Invoke-WebRequest "$Url/v1/status" -UseBasicParsing -TimeoutSec 1
    $json = [Text.Encoding]::UTF8.GetString($response.RawContentStream.ToArray())
    return ($json | ConvertFrom-Json)
}
function Start-OrvexaNode {
    param([string]$Name, [int]$Port, [string]$ExpectedChain)
    $dbPath = Join-Path $runDirectory "$Name.sqlite"
    $suffix = [guid]::NewGuid().ToString('N').Substring(0, 6)
    $process = Start-Process -FilePath $nodeExecutable -ArgumentList @('run', '--db', ('"{0}"' -f $dbPath), '--listen', "127.0.0.1:$Port") -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $runDirectory "$Name-$suffix.stdout.log") -RedirectStandardError (Join-Path $runDirectory "$Name-$suffix.stderr.log")
    $processes.Add($process)
    $url = "http://127.0.0.1:$Port"
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        if ($process.HasExited) { throw "Node $Name exited; inspect $runDirectory" }
        try { $status = Get-OrvexaStatus -Url $url }
        catch { Start-Sleep -Milliseconds 100; continue }
        if ($status.network -cne $expectedNetworkLabel) { throw "Unexpected product name on node $Name" }
        if ($status.chain_id -ne $ExpectedChain -or $status.protocol -ne 1) { throw "Unexpected network or protocol on node $Name" }
        if ($process.HasExited) { throw "Node $Name exited during startup" }
        return $process
    }
    throw "Node $Name did not become ready"
}
function Assert-RejectedTransaction {
    param([string]$Url, [string]$Json)
    $rejected = $false
    try { Invoke-RestMethod "$Url/v1/transactions" -Method Post -ContentType 'application/json' -Body $Json | Out-Null }
    catch {
        if ([int]$_.Exception.Response.StatusCode -ne 400) { throw }
        $rejected = $true
    }
    if (-not $rejected) { throw 'Invalid transaction was accepted' }
}

Push-Location $projectRoot
try {
    if (-not $SkipBuild) {
        & cargo build --workspace --locked
        if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
    }
    if ($BasePort -lt 1024 -or $BasePort -gt 65533) { throw 'Choose a base port between 1024 and 65533' }
    $alicePath = Join-Path $runDirectory 'alice.wallet.json'
    $bobPath = Join-Path $runDirectory 'bob.wallet.json'
    $alice = Invoke-OrvexaCli -CommandArgs @('wallet-new', '--wallet', $alicePath, '--password-stdin') -Unlock
    $bob = Invoke-OrvexaCli -CommandArgs @('wallet-new', '--wallet', $bobPath, '--password-stdin') -Unlock
    $genesisPath = Join-Path $runDirectory 'genesis.json'
    $genesisResult = (Invoke-OrvexaCli -CommandArgs @('genesis', '--authority', $alice, '--recipient', $alice, '--out', $genesisPath)) | ConvertFrom-Json
    foreach ($name in @('a', 'b', 'c')) {
        & $nodeExecutable init --db (Join-Path $runDirectory "$name.sqlite") --genesis $genesisPath | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Node init failed: $name" }
    }
    $nodeA = Start-OrvexaNode -Name 'a' -Port $BasePort -ExpectedChain $genesisResult.chain_id
    $nodeB = Start-OrvexaNode -Name 'b' -Port ($BasePort + 1) -ExpectedChain $genesisResult.chain_id
    $nodeC = Start-OrvexaNode -Name 'c' -Port ($BasePort + 2) -ExpectedChain $genesisResult.chain_id
    $urlA = "http://127.0.0.1:$BasePort"
    $urlB = "http://127.0.0.1:$($BasePort + 1)"
    $urlC = "http://127.0.0.1:$($BasePort + 2)"
    $transfer = (Invoke-OrvexaCli -CommandArgs @('--node', $urlA, '--expect-chain', $genesisResult.chain_id, 'send', '--wallet', $alicePath, '--to', $bob, '--amount', '250', '--fee', '2', '--password-stdin') -Unlock) | ConvertFrom-Json
    $pending = @(Invoke-RestMethod "$urlA/v1/mempool")
    if ($pending.Count -ne 1) { throw 'Expected one pending transaction' }
    $signedJson = $pending[0] | ConvertTo-Json -Depth 10 -Compress
    Assert-RejectedTransaction -Url $urlA -Json $signedJson
    $tampered = $signedJson | ConvertFrom-Json
    $tampered.transaction.amount = 251
    Assert-RejectedTransaction -Url $urlB -Json ($tampered | ConvertTo-Json -Depth 10 -Compress)
    Invoke-OrvexaCli -CommandArgs @('--node', $urlB, 'sync', '--peer', $urlA) | Out-Null
    Invoke-OrvexaCli -CommandArgs @('--node', $urlB, 'produce', '--wallet', $alicePath, '--password-stdin') -Unlock | Out-Null
    Invoke-OrvexaCli -CommandArgs @('--node', $urlA, 'sync', '--peer', $urlB) | Out-Null
    Invoke-OrvexaCli -CommandArgs @('--node', $urlC, 'sync', '--peer', $urlB) | Out-Null
    Assert-RejectedTransaction -Url $urlA -Json $signedJson
    $before = Get-OrvexaStatus -Url $urlA
    Stop-Process -Id $nodeA.Id
    $nodeA.WaitForExit()
    $nodeA = Start-OrvexaNode -Name 'a' -Port $BasePort -ExpectedChain $genesisResult.chain_id
    $after = Get-OrvexaStatus -Url $urlA
    if ($before.tip -ne $after.tip -or $after.height -ne 1) { throw 'Restart did not restore the same chain' }
    $activeNodes = @(
        @{ url = $urlA; process = $nodeA },
        @{ url = $urlB; process = $nodeB },
        @{ url = $urlC; process = $nodeC }
    )
    if (@($activeNodes.process.Id | Sort-Object -Unique).Count -ne 3) { throw 'Expected three distinct node processes' }
    $nodeResults = foreach ($activeNode in $activeNodes) {
        $url = $activeNode.url
        if ($activeNode.process.HasExited -or $activeNode.process.ProcessName -ne 'orvexa-node') { throw 'Expected a running ORVEXA node process' }
        $status = Get-OrvexaStatus -Url $url
        if ($status.network -cne $expectedNetworkLabel -or $status.chain_id -ne $genesisResult.chain_id -or $status.protocol -ne 1) { throw 'Node identity does not match ORVEXA devnet' }
        $balance = (Invoke-OrvexaCli -CommandArgs @('--node', $url, 'balance', '--address', $bob)) | ConvertFrom-Json
        if ($status.tip -ne $after.tip -or $balance.confirmed.balance -ne 250 -or $status.burned_fees -ne 2) { throw 'Nodes disagree' }
        [ordered]@{ url = $url; process_id = $activeNode.process.Id; process_name = $activeNode.process.ProcessName; network = $status.network; height = $status.height; tip = $status.tip; bob_balance = $balance.confirmed.balance; burned_fees = $status.burned_fees }
    }
    $history = (Invoke-OrvexaCli -CommandArgs @('--node', $urlA, 'history', '--address', $bob)) | ConvertFrom-Json
    if (@($history).Count -ne 1) { throw 'Expected one confirmed history item' }
    $result = [ordered]@{ result = 'PASS'; chain_id = $genesisResult.chain_id; transaction_id = $transfer.id; invalid_signature_rejected = $true; pending_double_spend_rejected = $true; confirmed_replay_rejected = $true; restart_restored = $true; nodes = @($nodeResults); privacy = 'TRANSPARENT'; consensus = 'EXPERIMENTAL single authority' }
    $result | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $runDirectory 'result.json') -Encoding UTF8
    $result | ConvertTo-Json -Depth 10
    Write-Output "Demo evidence: $runDirectory"
}
finally {
    foreach ($process in $processes) {
        if (-not $process.HasExited) { Stop-Process -Id $process.Id; $process.WaitForExit() }
        $process.Dispose()
    }
    $testPassword = $null
    Pop-Location
}
