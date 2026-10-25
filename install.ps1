# Installs the plugin: puts its binary in bin\ (scripts\fetch-bin), links it to herdr, adds the key bindings
# (prefix+f, prefix+a, prefix+d) and, with Claude Code, links its skill.
$ErrorActionPreference = 'Stop'
if (-not (Get-Command herdr -ErrorAction SilentlyContinue)) { Write-Host "herdr not found"; exit 1 }

# The binary of the manifest's version, before the link: on failure, herdr is left as it was.
& (Join-Path $PSScriptRoot 'scripts\fetch-bin.ps1') $PSScriptRoot
if ($LASTEXITCODE) { exit 1 }

# Windows PowerShell 5.1 turns a native command's stderr into an error, which 'Stop' would make fatal: not linked
# yet, or no server to reload, is fine.
$ErrorActionPreference = 'Continue'
herdr plugin unlink gcaudan.explorer 2>$null | Out-Null
$ErrorActionPreference = 'Stop'
herdr plugin link $PSScriptRoot | Out-Null
Write-Host "  plugin  $PSScriptRoot"

# Where herdr reads it: HERDR_CONFIG_PATH, else %APPDATA%\herdr.
$config = if ($env:HERDR_CONFIG_PATH) { $env:HERDR_CONFIG_PATH } else { Join-Path $env:APPDATA 'herdr\config.toml' }
if (-not (Test-Path $config)) { New-Item -ItemType File -Force -Path $config | Out-Null }
function Bind($key, $action, $description) {
    if (Select-String -Path $config -SimpleMatch "gcaudan.explorer.$action`"" -Quiet) { return }
    Add-Content -Path $config -Value @"

[[keys.command]]
key = "$key"
type = "plugin_action"
command = "gcaudan.explorer.$action"
description = "$description"
"@
    Write-Host "  key     $key in $config"
}
Bind 'prefix+f' 'toggle' 'file explorer'
Bind 'prefix+a' 'search' 'go to file'
Bind 'prefix+d' 'scm' 'source control'
$ErrorActionPreference = 'Continue'
herdr server reload-config 2>$null | Out-Null
$ErrorActionPreference = 'Stop'

# The herdr-preview skill, for Claude Code (~\.claude present): a junction to the clone (no admin right needed), so
# that updates reach it. A folder of that name that is not a link is yours, and left alone.
$claude = Join-Path $env:USERPROFILE '.claude'
if (Test-Path $claude) {
    $skills = Join-Path $claude 'skills'
    $link = Join-Path $skills 'herdr-preview'
    $item = Get-Item $link -Force -ErrorAction SilentlyContinue
    if (-not $item -or $item.LinkType) {
        if ($item) { $item.Delete() }
        New-Item -ItemType Directory -Force -Path $skills | Out-Null
        New-Item -ItemType Junction -Path $link -Target (Join-Path $PSScriptRoot 'skills\herdr-preview') | Out-Null
        Write-Host "  skill   $link"
    } else { Write-Host "  skill   $link exists and is not a link: left alone" }
}
