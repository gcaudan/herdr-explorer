#!/bin/sh
# Runs a herdr of its own on this clone, to try a development build without touching the herdr in use: its own
# config, plugin list, socket, sessions and plugin state, under HX_TEST_HOME (default ~/.cache/herdr-explorer-test).
# The first run copies your config.toml there (key bindings, theme); delete the folder to start afresh.
#   scripts/test-herdr.sh [--no-build] [herdr arguments…]   build (dev-build), link this clone, attach
#   scripts/test-herdr.sh --stop                            stop the test herdr's server
set -eu
dir=$(cd "$(dirname "$0")/.." && pwd)
home=${HX_TEST_HOME:-${XDG_CACHE_HOME:-$HOME/.cache}/herdr-explorer-test}
fail() {
  echo "test-herdr: $*" >&2
  exit 1
}
command -v herdr >/dev/null 2>&1 || fail "herdr not found in PATH"
# The config in use, read before the variables below change where herdr looks.
config=${HERDR_CONFIG_PATH:-${XDG_CONFIG_HOME:-$HOME/.config}/herdr/config.toml}
# herdr keeps its sockets in its config folder, and the system refuses a socket path over ~104 bytes.
sock="$home/config/herdr/herdr-client.sock"
[ "${#sock}" -lt 100 ] || fail "$home is too long a path for herdr's sockets: set HX_TEST_HOME to a shorter one"
# Run from a herdr pane, the HERDR_* variables would lead to the herdr in use: none is passed on.
for v in $(env | sed -n 's/^\(HERDR_[A-Za-z0-9_]*\)=.*/\1/p'); do unset "$v"; done
export XDG_CONFIG_HOME="$home/config" XDG_STATE_HOME="$home/state"
if [ "${1:-}" = --stop ]; then
  exec herdr server stop
fi
build=1
if [ "${1:-}" = --no-build ]; then
  build=
  shift
fi
[ -z "$build" ] || "$dir/scripts/dev-build.sh"
[ -x "$dir/bin/herdr-explorer" ] || fail "no bin/herdr-explorer: run without --no-build"
mkdir -p "$home/config/herdr" "$home/state"
# A copy, not a link: setup may add key bindings to it, never to yours.
if [ ! -e "$home/config/herdr/config.toml" ] && [ -f "$config" ]; then
  cp "$config" "$home/config/herdr/config.toml"
fi
if ! herdr plugin list 2>/dev/null | grep -qF "[local:$dir]"; then
  herdr plugin unlink gcaudan.explorer >/dev/null 2>&1 || true
  herdr plugin link "$dir" >/dev/null
fi
exec herdr "$@"
