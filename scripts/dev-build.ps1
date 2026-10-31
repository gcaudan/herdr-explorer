# Builds the binary (cargo build --release) into bin\, marked as a development one (bin\.dev): fetch-bin
# and the launcher then leave it alone. Delete bin\.dev to go back to the release binaries.
$ErrorActionPreference = 'Stop'
$dir = Split-Path -Parent $PSScriptRoot
# cargo from PATH, else rustup's own folder.
$cargo = (Get-Command cargo -ErrorAction SilentlyContinue).Source
if (-not $cargo) {
    $home_ = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    $cargo = Join-Path $home_ 'bin\cargo.exe'
}
if (-not (Test-Path -LiteralPath $cargo)) {
    [Console]::Error.WriteLine('dev-build: cargo not found: install Rust (https://rustup.rs)')
    exit 1
}
# From the clone, so that rustup takes the toolchain of rust-toolchain.toml; into target\ whatever CARGO_TARGET_DIR
# says.
Push-Location -LiteralPath $dir
try { & $cargo build --release --target-dir (Join-Path $dir 'target') } finally { Pop-Location }
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$bin = Join-Path $dir 'bin'
New-Item -ItemType Directory -Force -Path $bin | Out-Null
$exe = Join-Path $bin 'herdr-explorer.exe'
Copy-Item -LiteralPath (Join-Path $dir 'target\release\herdr-explorer.exe') -Destination "$exe.new-$PID" -Force
# bin\.dev before the binary, so that no fetch replaces it in between; taken back if the binary cannot be put.
$dev = Join-Path $bin '.dev'
$hadDev = Test-Path -LiteralPath $dev
New-Item -ItemType File -Force -Path $dev | Out-Null
try {
    # The binary in use set aside first, which Windows allows while it runs (the binary removes these when it starts).
    if (Test-Path -LiteralPath $exe) {
        Move-Item -LiteralPath $exe -Destination "$exe.old-$(Get-Date -Format yyyyMMddHHmmss)-$PID" -Force
    }
    Move-Item -LiteralPath "$exe.new-$PID" -Destination $exe -Force
} catch {
    Remove-Item -LiteralPath "$exe.new-$PID" -Force -ErrorAction SilentlyContinue
    if (-not $hadDev) { Remove-Item -LiteralPath $dev -Force -ErrorAction SilentlyContinue }
    throw
}
# The binary put, bin\.dev marked anew: the explorer and preview panes restart on it once idle.
Set-Content -LiteralPath $dev -Value "$(Get-Date -Format o) $PID" -NoNewline
Write-Host "dev-build: $exe ($(& $exe --version)), bin\.dev written"
