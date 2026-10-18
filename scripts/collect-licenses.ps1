param([Parameter(Mandatory = $true)][string]$OutputDir, [string]$Target = 'all', [switch]$IncludeWasm)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
$entries = [System.Collections.Generic.List[object]]::new()

function Copy-Notices {
    param([string]$Kind, [string]$Name, [string]$Version, [string]$Folder, [string]$License, [string]$LicenseFile = '')
    $files = @(Get-ChildItem -LiteralPath $Folder -File | Where-Object { $_.Name -match '^(licen[cs]e|copying|notice|copyright|authors)([._-].*)?$' })
    if ($LicenseFile -and (Test-Path -LiteralPath $LicenseFile -PathType Leaf)) { $files += Get-Item -LiteralPath $LicenseFile }
    $files = @($files | Sort-Object FullName -Unique)
    if (-not $files.Count -and $Kind -eq 'cargo') {
        $pinned = Join-Path $projectRoot "docs\third-party-notices\$Name-$Version"
        $sourcePath = Join-Path $pinned 'source.json'
        if (Test-Path -LiteralPath $sourcePath) {
            $source = Get-Content -LiteralPath $sourcePath -Raw | ConvertFrom-Json
            if ($source.package -ne $Name -or $source.version -ne $Version -or $source.declared_license -ne $License) { throw 'Pinned license metadata mismatch' }
            foreach ($entry in $source.sources) {
                if ((Get-FileHash -LiteralPath (Join-Path $pinned $entry.file) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.sha256) { throw 'Pinned upstream notice hash mismatch' }
            }
            $files = @(Get-ChildItem -LiteralPath $pinned -File)
        }
    }
    if (-not $files.Count) { throw "Missing upstream license notices: $Kind $Name $Version" }
    $id = "$Kind-$Name-$Version" -replace '[^A-Za-z0-9._-]', '_'
    $destination = Join-Path $OutputDir $id
    New-Item -ItemType Directory -Path $destination | Out-Null
    foreach ($file in $files) { Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $destination $file.Name) }
    $entries.Add([ordered]@{ package = $Name; version = $Version; ecosystem = $Kind; declared_license = $License; notices = @($files.Name) })
}

Push-Location $projectRoot
try {
    New-Item -ItemType Directory -Path $OutputDir -Force | Out-Null
    $OutputDir = (Resolve-Path -LiteralPath $OutputDir).Path
    $metadata = (& cargo metadata --format-version 1 --locked | Out-String) | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Cargo license metadata failed' }
    # metadata includes optional/unselected protocols (e.g. QUIC) from lockfile
    # resolution. Collect the actual enabled graph, including build/test/targets.
    $selected = @{}
    $treeArguments = @('tree','--locked','--workspace','--target',$Target,'--edges','normal,build,dev','--prefix','none','--format','{p}')
    if ($IncludeWasm) { $treeArguments += @('--target','wasm32-unknown-unknown') }
    $tree = & cargo @treeArguments
    if ($LASTEXITCODE) { throw 'Cargo enabled dependency graph failed' }
    foreach ($line in $tree) { if ($line -match '^([^\s]+) v([^\s]+)') { $selected["$($Matches[1])@$($Matches[2])"] = $true } }
    foreach ($package in $metadata.packages) {
        if ($package.source -and $selected.ContainsKey("$($package.name)@$($package.version)")) {
            Copy-Notices 'cargo' $package.name $package.version (Split-Path -Parent $package.manifest_path) $package.license $package.license_file
        }
    }
    # Windows PowerShell 5.1 cannot parse npm lockfiles' empty root key. Let
    # the installed build-time Node normalize only installed package metadata.
    $npmMetadata = & node -e "const fs=require('fs'),path=require('path');const lock=JSON.parse(fs.readFileSync('web/package-lock.json','utf8'));const items=[];for(const key of Object.keys(lock.packages)){if(!key.startsWith('node_modules/'))continue;const folder=path.resolve('web',key),file=path.join(folder,'package.json');if(!fs.existsSync(file))continue;const p=JSON.parse(fs.readFileSync(file,'utf8'));items.push({folder,name:p.name,version:p.version,license:p.license||''});}process.stdout.write(JSON.stringify(items));"
    if ($LASTEXITCODE) { throw 'npm license metadata failed' }
    foreach ($package in (($npmMetadata -join "`n") | ConvertFrom-Json)) {
        $licenseFile = ''
        if ($package.name -eq '@rolldown/binding-win32-x64-msvc') {
            # This platform artifact identifies the parent rolldown project but
            # omits its MIT text. Copy it from the identical installed release.
            $parent = Get-Content -LiteralPath 'web\node_modules\rolldown\package.json' -Raw | ConvertFrom-Json
            if ($parent.version -ne $package.version) { throw 'Rolldown notice version mismatch' }
            $licenseFile = Join-Path $projectRoot 'web\node_modules\rolldown\LICENSE'
        }
        Copy-Notices 'npm' $package.name $package.version $package.folder $package.license $licenseFile
    }
    [IO.File]::WriteAllText((Join-Path $OutputDir 'index.json'), ($entries.ToArray() | ConvertTo-Json -Depth 5), [Text.UTF8Encoding]::new($false))
    Write-Host "Copied upstream license notices for $($entries.Count) packages (includes build/test dependencies)."
} finally { Pop-Location }
