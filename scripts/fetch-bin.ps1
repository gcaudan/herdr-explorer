# Puts the plugin's binary for this machine in bin\: the release asset of the version in herdr-plugin.toml, checked
# against the release's SHA256SUMS. Run from the plugin's folder, or with its path as argument, by herdr plugin
# install and update ([[build]]), install.ps1, the updater and the shims. Nothing to do when bin\.dev exists (a binary
# built by scripts\dev-build.ps1) or when the binary already has that version. On any failure, bin\ is left as it was.
param([string]$Dir = '.')
$ErrorActionPreference = 'Stop'

function Fail($message) {
    [Console]::Error.WriteLine("fetch-bin: $message")
    exit 1
}

$Dir = (Resolve-Path -LiteralPath $Dir).Path
$bin = Join-Path $Dir 'bin'
$exe = Join-Path $bin 'herdr-explorer.exe'
$lock = Join-Path $bin '.fetch.lock'

$version = $null
$manifest = Join-Path $Dir 'herdr-plugin.toml'
if (Test-Path -LiteralPath $manifest) {
    foreach ($line in [IO.File]::ReadAllLines($manifest)) {
        if ($line -match '^version\s*=\s*"([^"]*)"') { $version = $Matches[1]; break }
    }
}
if (-not $version) { Fail "no version in $manifest" }

# Nothing to fetch: a development binary, or the binary of that version.
function DoneAlready {
    if (Test-Path -LiteralPath (Join-Path $bin '.dev')) { return $true }
    if (-not (Test-Path -LiteralPath $exe)) { return $false }
    try { return ((& $exe --version 2>$null) -join "`n") -eq $version } catch { return $false }
}
if (DoneAlready) { exit 0 }

# Windows on ARM64 runs the x86_64 binary in emulation. A 32-bit PowerShell on a 64-bit Windows gives its own
# architecture in PROCESSOR_ARCHITECTURE, the machine's in PROCESSOR_ARCHITEW6432.
$machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
if ($machine -ne 'AMD64' -and $machine -ne 'ARM64') {
    Fail "no release binary for Windows $machine`: build one with scripts\dev-build.ps1 (Rust needed)"
}
$asset = 'herdr-explorer-x86_64-pc-windows-msvc.exe'

# The releases of the clone's origin when it is on GitHub (a fork follows its own), else gcaudan/herdr-explorer: the
# rule of the update check.
$repo = 'gcaudan/herdr-explorer'
if (Get-Command git -ErrorAction SilentlyContinue) {
    $ErrorActionPreference = 'Continue'
    $origin = (& git -C $Dir config --get remote.origin.url 2>$null) -join ''
    $ErrorActionPreference = 'Stop'
    if ($origin.Trim() -cmatch '^(?:https://|ssh://git@|git@)github\.com[:/]([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?/?$') {
        if ($Matches[1] -notmatch '^\.+$' -and $Matches[2] -notmatch '^\.+$') { $repo = "$($Matches[1])/$($Matches[2])" }
    }
}
$url = "https://github.com/$repo/releases/download/v$version"

# curl.exe, not curl: Windows PowerShell 5.1 has curl as an alias of Invoke-WebRequest.
if (-not (Get-Command curl.exe -ErrorAction SilentlyContinue)) {
    Fail 'curl.exe not found: install it, then run scripts\fetch-bin.ps1 again'
}

New-Item -ItemType Directory -Force -Path $bin | Out-Null
$tmp = Join-Path $bin ".fetch-$PID"
$mine = $null
try {
    # One fetch at a time: another one running, its result is waited for and checked again. A lock older than 2
    # minutes is left by a fetch that died: taken by a rename, which only one fetch wins, then removed if it is still
    # that old (else another fetch's new lock, put back).
    $tries = 0
    while ($true) {
        try {
            $mine = [IO.File]::Open($lock, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Delete)
            break
        } catch {
            $item = Get-Item -LiteralPath $lock -Force -ErrorAction SilentlyContinue
            if (-not $item) {
                # No lock, yet none could be made: bin\ is not writable.
                $tries++
                if ($tries -ge 3) { Fail "cannot create $lock" }
                continue
            }
            $old = (Get-Date).AddMinutes(-2)
            if ($item.LastWriteTime -lt $old) {
                $taken = "$lock.$PID"
                try { [IO.File]::Move($lock, $taken) } catch { $taken = $null }
                if ($taken) {
                    if ((Get-Item -LiteralPath $taken -Force).LastWriteTime -lt $old) {
                        Remove-Item -LiteralPath $taken -Force -ErrorAction SilentlyContinue
                    } else {
                        try { [IO.File]::Move($taken, $lock) }
                        catch { Remove-Item -LiteralPath $taken -Force -ErrorAction SilentlyContinue }
                    }
                    continue
                }
            }
            Start-Sleep -Seconds 1
            if (DoneAlready) { exit 0 }
        }
    }
    if (DoneAlready) { exit 0 }

    & curl.exe -fsSL -o $tmp "$url/$asset"
    if ($LASTEXITCODE) { Fail "cannot download $url/$asset" }
    & curl.exe -fsSL -o "$tmp.sums" "$url/SHA256SUMS"
    if ($LASTEXITCODE) { Fail "cannot download $url/SHA256SUMS" }
    $want = $null
    foreach ($line in [IO.File]::ReadAllLines("$tmp.sums")) {
        $parts = $line -split '\s+'
        if ($parts.Count -ge 2 -and ($parts[1] -ceq $asset -or $parts[1] -ceq "*$asset")) { $want = $parts[0]; break }
    }
    if ($want -cnotmatch '^[0-9a-f]{64}$') { Fail "$asset is not in SHA256SUMS" }
    $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash.ToLowerInvariant()
    if ($got -ne $want) { Fail "$asset does not match its SHA256SUMS: not installed" }

    # The binary replaced by renames: the old one first set aside, which Windows allows while it runs (the binary
    # removes these when it starts).
    if (Test-Path -LiteralPath $exe) {
        Move-Item -LiteralPath $exe -Destination "$exe.old-$(Get-Date -Format yyyyMMddHHmmss)-$PID" -Force
    }
    Move-Item -LiteralPath $tmp -Destination $exe -Force
    Write-Host "fetch-bin: herdr-explorer $version ($asset)"
} finally {
    Remove-Item -LiteralPath $tmp, "$tmp.sums" -Force -ErrorAction SilentlyContinue
    if ($mine) {
        $mine.Dispose()
        Remove-Item -LiteralPath $lock -Force -ErrorAction SilentlyContinue
    }
}
