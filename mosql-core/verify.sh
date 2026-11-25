#!/usr/bin/env bash
# The gate for the serverless driver crate, mirroring worker/verify.sh:
# formatting, clippy, the tests, a dependency check, and a build for the
# WebAssembly target — the one a Cloudflare Worker runs.
#
#   ./verify.sh              every stage
#   ./verify.sh --quick      every stage except the WebAssembly build
#   ./verify.sh <stage>      just that stage
#
# Stages: fmt, clippy, test, deps, wasm, features
#
# The stages are separately addressable so CI can give each one its own
# step: a red X next to "clippy" is worth more than one next to "verify".
set -euo pipefail
cd "$(dirname "$0")"

STAGES=(fmt clippy test deps wasm features)
QUICK=0
ONLY=()

for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    fmt|clippy|test|deps|wasm|features) ONLY+=("$arg") ;;
    *) echo "verify: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done
(( ${#ONLY[@]} )) || ONLY=("${STAGES[@]}")

run() { [[ " ${ONLY[*]} " == *" $1 "* ]]; }

# Run one stage's command, keeping what it printed. On failure the lines
# that name the problem are repeated as step annotations, which can be read
# through the API even when the Actions log cannot.
stage() {
  local name=$1 log status
  shift
  log=$(mktemp)
  set +e
  "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  set -e
  if (( status != 0 )); then
    sed -i 's/\x1b\[[0-9;]*[a-zA-Z]//g' "$log"
    local important errors
    important='fatal error|error C[0-9]+|LNK[0-9]+|Could not find|cannot open|not recognized|^test .* \.\.\. FAILED|^---- |panicked at|assertion .*failed|^  (left|right):'
    errors='^(error|warning)(\[|:)'
    { grep -E -A1 "$important" "$log"; grep -E -A3 "$errors" "$log"; } \
      | grep -vE '^--$|^$|^ *note: run with|^ *= note:|^ *\||^[0-9]+ *\|' \
      | tr -d '`' \
      | awk '!seen[$0]++' | head -n 10 \
      | sed "s/^/::error title=$name::/" >&2 || true
    exit "$status"
  fi
  rm -f "$log"
}

if run fmt; then
  echo "== fmt =="
  stage fmt cargo fmt --check
fi

if run clippy; then
  echo "== clippy =="
  stage clippy cargo clippy --all-targets -- -D warnings
fi

if run test; then
  echo "== test =="
  # Every wire protocol is checked against a mock transport: the exact
  # URL, the exact headers and the exact body of every request, and the
  # exact answer of every provider.
  stage test cargo test
fi

if run deps; then
  echo "== no external dependencies =="
  # The one path dependency is in-repo and itself dependency-free, so the
  # crate has nothing in it that was not chosen line by line.
  if sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
      | grep -vE '^\[|^$|^#|path *=' \
      | grep -q .; then
    echo "verify: Cargo.toml gained an external dependency" >&2
    echo "the drivers are built without a registry; add what you need to" >&2
    echo "rust/ (the JSON model) instead." >&2
    exit 1
  fi
  echo "no external dependencies: ok"
fi

if run wasm; then
  if (( QUICK )); then
    echo "== wasm: skipped (--quick) =="
  else
    echo "== wasm =="
    # The crate is `#![no_std]` + `alloc` when the standard library is
    # switched off, so a Worker can link it without a TLS stack, a
    # thread, a clock or a filesystem.
    stage wasm cargo check --target wasm32-unknown-unknown --no-default-features
  fi
fi

if run features; then
  echo "== features =="
  # Every optional transport has to compile on its own, and the whole
  # crate has to compile with the standard library switched off.
  stage features cargo check --features transport-curl
  stage features cargo check --features transport-worker
  # The Worker's transport has to build for WebAssembly as well: that is
  # where a Worker runs it.
  stage features cargo check --target wasm32-unknown-unknown --no-default-features --features transport-worker
fi

echo "verify: ok"
