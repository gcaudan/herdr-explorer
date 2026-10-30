#!/bin/sh
# Puts the plugin's binary for this machine in bin/: the release asset of the version in herdr-plugin.toml, checked
# against the release's SHA256SUMS. Run from the plugin's folder, or with its path as argument, by herdr plugin
# install and update ([[build]]) and the launcher. Nothing to do when bin/.dev exists (a binary
# built by scripts/dev-build.sh) or when the binary already has that version. On any failure, bin/ is left as it was.
set -eu

fail() {
  echo "fetch-bin: $1" >&2
  exit 1
}

dir=$(cd "${1:-.}" && pwd) || fail "no folder ${1:-.}"
bin="$dir/bin"
exe="$bin/herdr-explorer"
lock="$bin/.fetch.lock"

version=$(sed -n 's/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' "$dir/herdr-plugin.toml" 2>/dev/null | head -n 1)
[ -n "$version" ] || fail "no version in $dir/herdr-plugin.toml"

# Nothing to fetch: a development binary, or the binary of that version.
done_already() {
  [ -e "$bin/.dev" ] && return 0
  [ -x "$exe" ] && [ "$("$exe" --version 2>/dev/null || true)" = "$version" ]
}
done_already && exit 0

case "$(uname -s)" in
  Linux) os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  *) os= ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  aarch64 | arm64) arch=aarch64 ;;
  *) arch= ;;
esac
[ -n "$os" ] && [ -n "$arch" ] ||
  fail "no release binary for $(uname -s) $(uname -m): build one with scripts/dev-build.sh (Rust needed)"
asset="herdr-explorer-$arch-$os"

# The releases of the clone's origin when it is on GitHub (a fork follows its own), else gcaudan/herdr-explorer: the
# rule of the update check.
repo=gcaudan/herdr-explorer
origin=$(git -C "$dir" config --get remote.origin.url 2>/dev/null || true)
found=$(printf '%s\n' "$origin" |
  sed -nE 's#^[[:space:]]*(https://|ssh://git@|git@)github\.com[:/]([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)/?[[:space:]]*$#\2 \3#p')
if [ -n "$found" ]; then
  owner=${found% *}
  name=${found#* }
  [ "${name%.git}" = "" ] || name=${name%.git}
  case "$owner" in *[!.]*) ;; *) owner= ;; esac
  case "$name" in *[!.]*) ;; *) name= ;; esac
  [ -n "$owner" ] && [ -n "$name" ] && repo="$owner/$name"
fi
url="https://github.com/$repo/releases/download/v$version"

command -v curl >/dev/null 2>&1 || fail "curl not found: install it, then run scripts/fetch-bin.sh again"
if command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  fail "neither sha256sum nor shasum found"
fi

mkdir -p "$bin"
tmp="$bin/.fetch-$$"
mine=
cleanup() {
  rm -f "$tmp" "$tmp.sums"
  [ -z "$mine" ] || rm -f "$lock"
}
trap cleanup EXIT
trap 'exit 1' INT TERM HUP

# One fetch at a time: another one running, its result is waited for and checked again. A lock older than 2 minutes
# is left by a fetch that died: taken by a rename, which only one fetch wins, then removed if it is still that old
# (else another fetch's new lock, put back).
tries=0
while :; do
  if (set -C && : >"$lock") 2>/dev/null; then
    mine=1
    break
  fi
  if [ ! -e "$lock" ]; then
    # No lock, yet none could be made: bin/ is not writable.
    tries=$((tries + 1))
    [ "$tries" -lt 3 ] || fail "cannot create $lock"
    continue
  fi
  if [ -n "$(find "$lock" -mmin +2 2>/dev/null)" ] && mv "$lock" "$lock.$$" 2>/dev/null; then
    if [ -n "$(find "$lock.$$" -mmin +2 2>/dev/null)" ]; then
      rm -f "$lock.$$"
    else
      mv "$lock.$$" "$lock" 2>/dev/null || rm -f "$lock.$$"
    fi
    continue
  fi
  sleep 1
  done_already && exit 0
done
done_already && exit 0

curl -fsSL -o "$tmp" "$url/$asset" || fail "cannot download $url/$asset (a release just tagged may still be building: retry in a few minutes)"
curl -fsSL -o "$tmp.sums" "$url/SHA256SUMS" || fail "cannot download $url/SHA256SUMS"
want=$(awk -v a="$asset" '$2 == a || $2 == "*" a { print $1; exit }' "$tmp.sums")
case "$want" in
  *[!0-9a-f]* | "") fail "$asset is not in SHA256SUMS" ;;
esac
[ ${#want} -eq 64 ] || fail "$asset is not in SHA256SUMS"
[ "$(sha "$tmp")" = "$want" ] || fail "$asset does not match its SHA256SUMS: not installed"

# The binary replaced by renames: the old one first set aside (the binary removes these when it starts).
chmod +x "$tmp"
[ ! -e "$exe" ] || mv -f "$exe" "$exe.old-$(date +%Y%m%d%H%M%S)-$$"
mv -f "$tmp" "$exe"
echo "fetch-bin: herdr-explorer $version ($asset)"
