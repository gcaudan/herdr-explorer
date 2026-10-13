#!/usr/bin/env bash
# Installs the plugin: links it to herdr and adds the key bindings (prefix+f, prefix+a, prefix+d).
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
command -v herdr >/dev/null || { echo "herdr not found" >&2; exit 1; }

herdr plugin unlink gcaudan.explorer >/dev/null 2>&1 || true
herdr plugin link "$here" >/dev/null
echo "  plugin  $here"

config="$HOME/.config/herdr/config.toml"
mkdir -p "$(dirname "$config")"
touch "$config"
bind() { # key action description
  grep -q "gcaudan.explorer.$2\"" "$config" && return
  printf '\n[[keys.command]]\nkey = "%s"\ntype = "plugin_action"\ncommand = "gcaudan.explorer.%s"\ndescription = "%s"\n' "$1" "$2" "$3" >> "$config"
  echo "  key     $1 in $config"
}
bind prefix+f toggle "file explorer"
bind prefix+a search "go to file"
bind prefix+d scm "source control"
herdr server reload-config >/dev/null 2>&1 || true
