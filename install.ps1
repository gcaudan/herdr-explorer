# Installs the plugin: links it to herdr and adds the key bindings (prefix+f, prefix+a, prefix+d).
$ErrorActionPreference = 'Stop'
if (-not (Get-Command herdr -ErrorAction SilentlyContinue)) { Write-Host "herdr not found"; exit 1 }

herdr plugin unlink gcaudan.explorer 2>$null | Out-Null
herdr plugin link $PSScriptRoot | Out-Null
Write-Host "  plugin  $PSScriptRoot"

$config = Join-Path $env:APPDATA 'herdr\config.toml'
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
herdr server reload-config 2>$null | Out-Null
