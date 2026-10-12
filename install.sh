#!/usr/bin/env bash
# Installs the plugin: links it to herdr and adds the prefix+f key binding.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
command -v herdr >/dev/null || { echo "herdr not found" >&2; exit 1; }

herdr plugin unlink gcaudan.explorer >/dev/null 2>&1 || true
herdr plugin link "$here" >/dev/null
echo "  plugin  $here"

config="$HOME/.config/herdr/config.toml"
mkdir -p "$(dirname "$config")"
touch "$config"
if ! grep -q 'gcaudan.explorer.toggle' "$config"; then
  cat >> "$config" <<'EOF'

[[keys.command]]
key = "prefix+f"
type = "plugin_action"
command = "gcaudan.explorer.toggle"
description = "file explorer"
EOF
  echo "  key     prefix+f in $config"
fi
herdr server reload-config >/dev/null 2>&1 || true
