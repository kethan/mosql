#!/usr/bin/env bash
# Build the wasm modules used by wasm/mosql.js.
#
#   mosql.wasm      full: SQL builder + in-memory engine
#   mosql-core.wasm SQL builder only (~40% smaller)
set -euo pipefail
cd "$(dirname "$0")"

TARGET=wasm32-unknown-unknown

build() {
  local out="$1"; shift
  cargo build --lib --target "$TARGET" --profile wasm --no-default-features "$@" >/dev/null
  cp "target/$TARGET/wasm/mosql.wasm" "wasm/$out"
  printf '%-16s %8s bytes (%6s gzip)\n' "$out" \
    "$(wc -c < "wasm/$out" | tr -d ' ')" \
    "$(gzip -9 -c "wasm/$out" | wc -c | tr -d ' ')"
}

build mosql.wasm --features memory
build mosql-core.wasm
