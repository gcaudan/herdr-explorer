# Installs the plugin: links it to herdr and adds the prefix+f key binding.
$ErrorActionPreference = 'Stop'
if (-not (Get-Command herdr -ErrorAction SilentlyContinue)) { Write-Host "herdr not found"; exit 1 }

herdr plugin unlink gcaudan.explorer 2>$null | Out-Null
herdr plugin link $PSScriptRoot | Out-Null
Write-Host "  plugin  $PSScriptRoot"

$config = Join-Path $env:APPDATA 'herdr\config.toml'
if (-not (Test-Path $config)) { New-Item -ItemType File -Force -Path $config | Out-Null }
if (-not (Select-String -Path $config -SimpleMatch 'gcaudan.explorer.toggle' -Quiet)) {
    Add-Content -Path $config -Value @'

[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "gcaudan.explorer.toggle"
description = "file explorer"
'@
    Write-Host "  key     prefix+f in $config"
}
herdr server reload-config 2>$null | Out-Null
