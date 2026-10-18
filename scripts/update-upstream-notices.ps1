param([string]$Target = 'all', [switch]$IncludeWasm)
# Build-time maintenance only. No node/launcher calls GitHub or SPDX at runtime.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path -Parent $PSScriptRoot
Push-Location $projectRoot
try {
    $metadata = (cargo metadata --format-version 1 --locked | Out-String) | ConvertFrom-Json
    if ($LASTEXITCODE) { throw 'Metadata failed' }
    $selected = @{}
    $treeArguments = @('tree','--locked','--workspace','--target',$Target,'--edges','normal,build,dev','--prefix','none','--format','{p}')
    if ($IncludeWasm) { $treeArguments += @('--target','wasm32-unknown-unknown') }
    cargo @treeArguments | ForEach-Object {
        if ($_ -match '^([^\s]+) v([^\s]+)') { $selected["$($Matches[1])@$($Matches[2])"] = $true }
    }
    if ($LASTEXITCODE) { throw 'Dependency tree failed' }
    $trees = @{}
    foreach ($package in $metadata.packages) {
        if (-not $package.source -or -not $selected.ContainsKey("$($package.name)@$($package.version)")) { continue }
        $folder = Split-Path -Parent $package.manifest_path
        $files = @(Get-ChildItem -LiteralPath $folder -File | Where-Object { $_.Name -match '^(licen[cs]e|copying|notice|copyright|authors)([._-].*)?$' })
        if ($files.Count -or $package.license_file) { continue }
        $destination = Join-Path $projectRoot "docs\third-party-notices\$($package.name)-$($package.version)"
        if (Test-Path -LiteralPath (Join-Path $destination 'source.json')) { continue }
        $vcs = Get-Content -LiteralPath (Join-Path $folder '.cargo_vcs_info.json') -Raw | ConvertFrom-Json
        if ($package.repository -notmatch '^https://github.com/([^/]+/[^/]+?)(?:\.git)?/?$') { throw "Unsupported notice repository: $($package.name)" }
        $repository = $Matches[1]
        $commit = $vcs.git.sha1
        if ($commit -notmatch '^[0-9a-f]{40}$') { throw 'Invalid upstream source commit' }
        if ($package.name -eq 'yamux' -and $package.version -eq '0.13.10') {
            # Published Cargo repository/VCS points to the former repository.
            # Pin the maintained repository's release tag commit and verify its
            # entire package manifest matches the published crate byte-for-byte.
            $repository = 'libp2p/rust-yamux'
            $commit = '70db05dc63e8368bd0559a5ec0dba6e5fc2bdd41'
            $remoteManifest = (Invoke-WebRequest -UseBasicParsing -Uri "https://raw.githubusercontent.com/$repository/$commit/yamux/Cargo.toml").Content
            $localManifest = Get-Content -LiteralPath (Join-Path $folder 'Cargo.toml.orig') -Raw
            if ($remoteManifest.Replace("`r`n","`n") -cne $localManifest.Replace("`r`n","`n")) { throw 'Yamux release manifest mismatch' }
        }
        $treeKey = "$repository@$commit"
        if (-not $trees.ContainsKey($treeKey)) { $trees[$treeKey] = Invoke-RestMethod -Uri "https://api.github.com/repos/$repository/git/trees/${commit}?recursive=1" }
        $licenses = @($trees[$treeKey].tree | Where-Object { $_.type -eq 'blob' -and $_.path -match '^(licen[cs]e|copying|notice|copyright|authors)([._-].*)?$' })
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        $sources = [System.Collections.Generic.List[object]]::new()
        foreach ($license in $licenses) {
            $url = "https://raw.githubusercontent.com/$repository/$commit/$($license.path)"
            $out = Join-Path $destination $license.path
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $out
            $sources.Add([ordered]@{url=$url;file=$license.path;sha256=(Get-FileHash -LiteralPath $out -Algorithm SHA256).Hash.ToLowerInvariant()})
        }
        if (-not $licenses.Count) {
            if ($package.license -notin @('MIT','MIT OR Apache-2.0','Apache-2.0 OR MIT')) { throw "No standalone upstream license for $($package.name)" }
            # Preserve the exact published declaration. Do not invent an author
            # or describe the SPDX reference as an upstream copyright notice.
            Copy-Item -LiteralPath (Join-Path $folder 'Cargo.toml.orig') -Destination (Join-Path $destination 'Cargo.toml.orig')
            $url = 'https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/MIT.txt'
            $out = Join-Path $destination 'LICENSE-SPDX-MIT.txt'
            Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $out
            $sources.Add([ordered]@{url=$url;file='LICENSE-SPDX-MIT.txt';sha256=(Get-FileHash -LiteralPath $out -Algorithm SHA256).Hash.ToLowerInvariant()})
            [IO.File]::WriteAllText((Join-Path $destination 'NOTICE.txt'), "The published $($package.name) $($package.version) declares $($package.license) in Cargo.toml.orig. No standalone LICENSE/copyright file exists at the exact source tree root $commit. The SPDX MIT text is a reference for the MIT option with its original placeholders, not an invented upstream notice. Published authors/declaration are preserved in Cargo.toml.orig.`n",[Text.UTF8Encoding]::new($false))
        }
        $source = [ordered]@{package=$package.name;version=$package.version;repository=$package.repository;commit=$commit;declared_license=$package.license;sources=$sources.ToArray()}
        [IO.File]::WriteAllText((Join-Path $destination 'source.json'),($source|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
        Write-Host "Pinned notice: $($package.name) $($package.version)"
    }
} finally { Pop-Location }
