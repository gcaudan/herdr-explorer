#!/usr/bin/env bash
# Installs the plugin: links it to herdr, adds the key bindings (prefix+f, prefix+a, prefix+d) and, with Claude Code,
# links its skill.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
command -v herdr >/dev/null || { echo "herdr not found" >&2; exit 1; }

herdr plugin unlink gcaudan.explorer >/dev/null 2>&1 || true
herdr plugin link "$here" >/dev/null
echo "  plugin  $here"

# Where herdr reads it: HERDR_CONFIG_PATH, else $XDG_CONFIG_HOME/herdr, else ~/.config/herdr (macOS included).
config="${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}"
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

# The herdr-preview skill, for Claude Code (~/.claude present): a link to the clone, so that updates reach it. A
# folder of that name that is not a link is yours, and left alone.
skills="$HOME/.claude/skills"
if [ -d "$HOME/.claude" ]; then
  link="$skills/herdr-preview"
  if [ -L "$link" ] || [ ! -e "$link" ]; then
    mkdir -p "$skills"
    ln -sfn "$here/skills/herdr-preview" "$link"
    echo "  skill   $link"
  else
    echo "  skill   $link exists and is not a link: left alone"
  fi
fi
