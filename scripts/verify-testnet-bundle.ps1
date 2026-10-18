param(
    [Parameter(Mandatory = $true)][string]$Bundle,
    [Parameter(Mandatory = $true)][string]$TestNetworkDir,
    [int]$UiPort = 54242,
    [int]$MeshPort = 54243
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$testDir = Join-Path $projectRoot ('demo-data\bundle-check-' + [guid]::NewGuid().ToString('N'))
$wrapper = $null
try {
    $bundlePath = (Resolve-Path -LiteralPath $Bundle).Path
    $networkPath = (Resolve-Path -LiteralPath $TestNetworkDir).Path
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $networkPath 'network.json') | ConvertFrom-Json
    New-Item -ItemType Directory -Path $testDir | Out-Null
    $extract = Join-Path $testDir 'app'
    New-Item -ItemType Directory -Path $extract | Out-Null
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($bundlePath)
    try {
        foreach ($entry in $zip.Entries) {
            $resolvedEntry = [IO.Path]::GetFullPath((Join-Path $extract $entry.FullName))
            if (-not $resolvedEntry.StartsWith($extract + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe archive path' }
            if ($entry.FullName -match '\.wallet\.json$|\.secret$|\.sqlite|(^|/)genesis\.json$|(^|/)network\.json$') { throw 'App bundle contains user/network data' }
        }
    } finally { $zip.Dispose() }
    [IO.Compression.ZipFile]::ExtractToDirectory($bundlePath, $extract)
    $join = Join-Path $testDir 'join'
    New-Item -ItemType Directory -Path $join | Out-Null
    foreach ($name in @('genesis.json','network.json','mesh.secret')) {
        Copy-Item -LiteralPath (Join-Path $networkPath $name) -Destination (Join-Path $join $name)
    }
    $launcher = Join-Path $extract 'scripts\start-testnet.ps1'
    $nodeData = Join-Path $testDir 'node'
    $arguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',$launcher,
        '-MeshAddress','127.0.0.1','-Peers','http://127.0.0.1:54999',
        '-NetworkDir',$join,'-DataDir',$nodeData,'-UiPort',"$UiPort",'-MeshPort',"$MeshPort")
    $wrapper = Start-Process -FilePath 'powershell.exe' -ArgumentList (($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' ') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $testDir 'stdout.log') -RedirectStandardError (Join-Path $testDir 'stderr.log')
    $status = $null
    for ($attempt = 0; $attempt -lt 60; $attempt++) {
        if ($wrapper.HasExited) { throw "Bundled launcher exited. Inspect $testDir" }
        try { $status = Invoke-RestMethod "http://127.0.0.1:$UiPort/v1/status" -TimeoutSec 1; break } catch { Start-Sleep -Milliseconds 100 }
    }
    if (-not $status -or $status.chain_id -ne $manifest.chain_id) { throw 'Bundled node has wrong identity' }
    $runtime = Invoke-RestMethod "http://127.0.0.1:$UiPort/v1/runtime"
    if (-not $runtime.enabled -or $runtime.role -ne 'replica' -or $runtime.halted) { throw 'Wrong bundled runtime role/state' }
    $page = Invoke-WebRequest "http://127.0.0.1:$UiPort/" -UseBasicParsing
    if ($page.StatusCode -ne 200 -or $page.Content -notmatch 'ORVEXA') { throw 'Bundled web app unavailable' }
    $wasm = Invoke-WebRequest "http://127.0.0.1:$UiPort/wasm/orvexa_web_wallet_bg.wasm" -UseBasicParsing
    if ($wasm.StatusCode -ne 200 -or $wasm.Headers['Content-Type'] -ne 'application/wasm') { throw 'Bundled signing Wasm unavailable' }
    $unauthorized = $false
    try { Invoke-WebRequest "http://127.0.0.1:$MeshPort/v1/status" -UseBasicParsing | Out-Null }
    catch { if ($_.Exception.Response.StatusCode.value__ -eq 401) { $unauthorized = $true } else { throw } }
    if (-not $unauthorized) { throw 'Bundled mesh listener failed to authenticate' }
    $cli = Join-Path $extract 'bin\orvexa-cli.exe'
    & $cli --node "http://127.0.0.1:$UiPort" --expect-chain $manifest.chain_id network | Out-Null
    if ($LASTEXITCODE) { throw 'Bundled CLI failed' }
    $notices = Get-Content -LiteralPath (Join-Path $extract 'third-party-licenses\index.json') -Raw | ConvertFrom-Json
    if ($notices.Count -lt 1) { throw 'Missing bundled dependency notices' }
    $result = [ordered]@{ result = 'PASS'; app_archive_contains_no_wallets_or_join_credentials = $true; standalone_launcher_node_cli_web_wasm = $true; mesh_unauthorized_rejected = $true; copied_upstream_notice_packages = $notices.Count; transport_test = 'loopback only, no actual VPN or second computer'; bundle_sha256 = (Get-FileHash -LiteralPath $bundlePath -Algorithm SHA256).Hash.ToLowerInvariant() }
    [IO.File]::WriteAllText((Join-Path $testDir 'result.json'), ($result | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    $result | ConvertTo-Json
    Write-Host "Bundle evidence: $testDir"
} finally {
    if ($wrapper) {
        # Stop only the native child launched by this verification wrapper.
        $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($wrapper.Id)" | Where-Object { $_.Name -eq 'orvexa-node.exe' })
        foreach ($child in $children) {
            $process = Get-Process -Id $child.ProcessId -ErrorAction SilentlyContinue
            if ($process) { $process.Kill(); $process.WaitForExit(); $process.Dispose() }
        }
        if (-not $wrapper.HasExited -and -not $wrapper.WaitForExit(3000)) { $wrapper.Kill(); $wrapper.WaitForExit() }
        $wrapper.Dispose()
    }
}
