#!/usr/bin/env bash
#
# Every check this crate must pass, in one place.
#
# Used by CI (.github/workflows/rust.yml) and by publish.sh, so the two can
# never drift apart. Exits non-zero on the first failure.
#
#   ./verify.sh           # full run
#   ./verify.sh --quick   # skip clippy and the package step
#
# The wasm and Node addon bridges are not part of this script (they need Node
# and a platform the addon builds on); CI runs them as a separate job with
#   ./build-wasm.sh && node wasm/test.mjs
#   ./build-node.sh && node node/test.mjs
#
set -euo pipefail
cd "$(dirname "$0")"

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

# The napi bridge declares Node's `napi_*` symbols as bare `extern "C"` and lets
# the dynamic linker resolve them when Node loads the addon. Only Linux links a
# shared object with undefined symbols as-is; macOS needs
# `-undefined dynamic_lookup` (build-node.sh passes it) and Windows would need an
# import library for node.exe. So the feature is fully *built* on Linux and
# *compile-checked* elsewhere, which still catches any cfg mistake in the bridge.
case "$(uname -s)" in
  Linux*) NAPI_MODE=build ;;
  *)      NAPI_MODE=check ;;
esac

# True when std for `$1` is installed, so a bare-metal build can be attempted.
target_installed() {
  if command -v rustup >/dev/null 2>&1; then
    rustup target list --installed 2>/dev/null | grep -qx "$1"
  else
    # A bare toolchain (no rustup): look for the target's `core`.
    ls "$(rustc --print target-libdir --target "$1" 2>/dev/null)"/libcore-*.rlib >/dev/null 2>&1
  fi
}

TARGET=wasm32-unknown-unknown
step() { printf '\n\033[1m==> %s\033[0m\n' "$1"; }
ok()   { printf '    ok\n'; }

# ---------------------------------------------------------------------------
step "toolchain"
rustc --version
cargo --version
if [ "$QUICK" -eq 0 ]; then
  cargo fmt --version
  cargo clippy --version
fi

# ---------------------------------------------------------------------------
step "formatting"
cargo fmt --check
cargo fmt --manifest-path no_std-check/Cargo.toml --check
ok

# ---------------------------------------------------------------------------
if [ "$QUICK" -eq 0 ]; then
  step "clippy (-D warnings)"
  cargo clippy --all-targets -- -D warnings
  ok
fi

# ---------------------------------------------------------------------------
step "build, all targets, warnings are errors"
RUSTFLAGS="-D warnings" cargo build --all-targets
ok

# ---------------------------------------------------------------------------
step "feature matrix"
# Each configuration must build clean on its own: the crate is `no_std`, so a
# feature that accidentally pulls in `std` only shows up without defaults.
# Note: `--no-default-features` is deliberately absent. On a hosted target the
# crate has no allocator or panic handler of its own, so building it standalone
# cannot link — that is the consumer's job, and is what the `no_std` step below
# proves with a real bare-metal binary.
for cfg in \
  "" \
  "--features napi" \
  "--no-default-features --features memory --target wasm32-unknown-unknown"
do
  if [ "$cfg" = "--features napi" ] && [ "$NAPI_MODE" = check ]; then
    printf '    cargo check --lib %s  (link-tested on Linux only)\n' "$cfg"
    RUSTFLAGS="-D warnings" cargo check --lib $cfg
    continue
  fi
  printf '    cargo build --lib %s\n' "${cfg:-(default)}"
  RUSTFLAGS="-D warnings" cargo build --lib $cfg
done
ok

# ---------------------------------------------------------------------------
step "no_std (bare metal)"
# `mosql` is `#![no_std]` + `alloc`. A consumer must supply the allocator and
# panic handler; no_std-check does exactly that and calls into the crate, so a
# hidden `std` dependency would fail to link here.
if target_installed thumbv7em-none-eabihf; then
  cargo build --manifest-path no_std-check/Cargo.toml \
    --target thumbv7em-none-eabihf --release
  ok
else
  printf '    skipped (thumbv7em-none-eabihf not installed; CI runs it)\n'
fi

# ---------------------------------------------------------------------------
step "wasm32 (no_std)"
# Builder-only and builder+engine. The builder-only module is what a JS consumer
# ships, so it must keep building without the memory engine.
RUSTFLAGS="-D warnings" cargo build --lib --target "$TARGET" --profile wasm \
  --no-default-features --features memory
RUSTFLAGS="-D warnings" cargo build --lib --target "$TARGET" --profile wasm \
  --no-default-features
ok

# ---------------------------------------------------------------------------
step "tests (parity suites + examples)"
cargo test --tests
ok

# ---------------------------------------------------------------------------
if command -v node >/dev/null 2>&1; then
  step "fixture drift"
  # The fixture corpora are generated from the JavaScript implementation, which
  # is the oracle. If a generator changes its output, the Rust port may have
  # silently drifted from JS — so regeneration must be a no-op on a clean tree.
  node tests/gen-fixtures.mjs
  node tests/gen-memory-fixtures.mjs
  if ! git diff --quiet -- tests/fixtures.json tests/memory-fixtures.json; then
    echo "    FAIL: regenerating the fixtures produced a diff." >&2
    echo "    The JavaScript implementation is the oracle; commit the new" >&2
    echo "    fixtures only if that change was intentional." >&2
    git --no-pager diff --stat -- tests/fixtures.json tests/memory-fixtures.json >&2 || true
    exit 1
  fi
  ok
else
  step "fixture drift"
  printf '    skipped (node not on PATH)\n'
fi

# ---------------------------------------------------------------------------
if [ "$QUICK" -eq 0 ]; then
  step "package"
  # Verifies the manifest, builds the crate from the packaged sources, and
  # reports what a consumer would download. `include` in Cargo.toml keeps the
  # compiled addons and benchmark scripts out of the tarball.
  cargo package --allow-dirty
  CRATE=$(ls -1 target/package/mosql-*.crate 2>/dev/null | head -1 || true)
  if [ -n "$CRATE" ]; then
    printf '    %s  (%s bytes)\n' "$CRATE" "$(wc -c <"$CRATE" | tr -d ' ')"
  fi
  ok
fi

printf '\n\033[1;32mAll checks passed.\033[0m\n'
