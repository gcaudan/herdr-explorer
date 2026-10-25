#!/bin/sh
# Builds the binary from rust/ (cargo build --release) into bin/, marked as a development one (bin/.dev): fetch-bin,
# the updater and the shims then leave it alone. Delete bin/.dev to go back to the release binaries.
set -eu
dir=$(cd "$(dirname "$0")/.." && pwd)
# cargo from PATH, else rustup's own folder (installed without touching the shell's profile).
cargo=$(command -v cargo 2>/dev/null || true)
[ -n "$cargo" ] || cargo="${CARGO_HOME:-$HOME/.cargo}/bin/cargo"
[ -x "$cargo" ] || {
  echo "dev-build: cargo not found: install Rust (https://rustup.rs)" >&2
  exit 1
}
# From rust/, so that rustup takes the toolchain of rust-toolchain.toml; into rust/target whatever CARGO_TARGET_DIR
# says.
(cd "$dir/rust" && "$cargo" build --release --target-dir "$dir/rust/target")
mkdir -p "$dir/bin"
exe="$dir/bin/herdr-explorer"
cp "$dir/rust/target/release/herdr-explorer" "$exe.new-$$"
chmod +x "$exe.new-$$"
# bin/.dev before the binary, so that no fetch replaces it in between; taken back if the binary cannot be put.
had_dev=
[ ! -e "$dir/bin/.dev" ] || had_dev=1
: >"$dir/bin/.dev"
mv -f "$exe.new-$$" "$exe" || {
  rm -f "$exe.new-$$"
  [ -n "$had_dev" ] || rm -f "$dir/bin/.dev"
  exit 1
}
echo "dev-build: $exe ($("$exe" --version)), bin/.dev written"
