$script:OrvexaRoot = Split-Path -Parent $PSScriptRoot

function Get-OrvexaBinary {
    param([string]$Name, [switch]$NativeP2P)
    $folders = if ($NativeP2P) { @('bin','target\friends\release','target\friends\debug','target\release','target\debug') } else { @('bin', 'target\friends\debug', 'target\friends\release', 'target\debug', 'target\release') }
    foreach ($folder in $folders) {
        $candidate = Join-Path $script:OrvexaRoot "$folder\$Name.exe"
        if (Test-Path -LiteralPath $candidate) { return $candidate }
    }
    throw 'ORVEXA binaries not found. Use the Windows bundle or run cargo build --workspace --locked.'
}

function Write-OrvexaUtf8 {
    param([string]$Path, [string]$Text)
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
