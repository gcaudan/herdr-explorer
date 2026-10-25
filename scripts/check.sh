#!/bin/sh
# Every check, in order, stopping at the first failure: format, lints (host and the 5 release targets, compiled
# without linking), then the cargo tests.
set -eu
cd "$(dirname "$0")/.."
CARGO=${CARGO:-$HOME/.cargo/bin/cargo}
"$CARGO" fmt --check
"$CARGO" clippy --all-targets -- -D warnings
for t in x86_64-unknown-linux-musl aarch64-unknown-linux-musl x86_64-apple-darwin aarch64-apple-darwin \
  x86_64-pc-windows-msvc; do
  "$CARGO" clippy --target $t -- -D warnings
done
"$CARGO" test
