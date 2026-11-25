#!/usr/bin/env bash
#
# Build the Worker: the Rust engine into WebAssembly, and the module into
# the place the script imports it from.
#
#   ./build.sh            # release
#   ./build.sh --debug    # a debug build, which is much bigger but says
#                         # more when it goes wrong
#
set -euo pipefail
cd "$(dirname "$0")"

PROFILE=release
CARGO_FLAGS=(--release)
if [ "${1:-}" = "--debug" ]; then
  PROFILE=debug
  CARGO_FLAGS=()
fi

TARGET=wasm32-unknown-unknown

# Is the target there? A rustup toolchain answers that directly; a bare
# one has to be asked for the directory the standard library would live in.
target_installed() {
  if command -v rustup >/dev/null 2>&1 && rustup target list --installed 2>/dev/null | grep -qx "$1"; then
    return 0
  fi
  ls "$(rustc --print target-libdir --target "$1" 2>/dev/null)"/libcore-*.rlib >/dev/null 2>&1
}

if ! target_installed "$TARGET"; then
  echo "The $TARGET target is not installed. Run:" >&2
  echo "  rustup target add $TARGET" >&2
  exit 1
fi

cargo build "${CARGO_FLAGS[@]}" --target "$TARGET"

ARTIFACT="target/$TARGET/$PROFILE/mosql_worker.wasm"
cp "$ARTIFACT" js/mosql_worker.wasm

printf 'built %s (%s bytes)\n' "js/mosql_worker.wasm" "$(wc -c < js/mosql_worker.wasm)"
