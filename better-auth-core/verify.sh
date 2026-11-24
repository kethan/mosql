#!/usr/bin/env bash
# The gate for the better-auth-core crate, mirroring nanobase/verify.sh:
# formatting, clippy, the tests, a dependency check and a cap on the
# release binary's size.
#
#   ./verify.sh              every stage
#   ./verify.sh --quick      every stage except the publish dry-run
#   ./verify.sh <stage>      just that stage
#
# Stages: fmt, clippy, test, deps, publish, size
#
# The stages are separately addressable so CI can give each one its own
# step: a red X next to "clippy" is worth more than one next to "verify".
set -euo pipefail
cd "$(dirname "$0")"

STAGES=(fmt clippy test deps publish size)
QUICK=0
ONLY=()

for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    fmt|clippy|test|deps|publish|size) ONLY+=("$arg") ;;
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
    local important errors
    important='\[better-auth-core\]|fatal error|error C[0-9]+|LNK[0-9]+|Could not find|cannot open|not recognized|^test .* \.\.\. FAILED|^---- |panicked at|assertion .*failed|^  (left|right):'
    errors='^(error|warning)(\[|:)'
    { grep -E -A1 "$important" "$log"; grep -E -A2 "$errors" "$log"; } \
      | grep -vE '^--$|^$|^ *note: run with' | tr -d '`\r' \
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
  # The library's own tests and the eight integration suites, which drive
  # the real route table.
  stage test cargo test
fi

if run deps; then
  echo "== no external dependencies =="
  # The three path dependencies are in-repo and themselves dependency-free,
  # so the binary has nothing in it that was not chosen line by line.
  if sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
      | grep -vE '^\[|^$|^#|path *=' \
      | grep -q .; then
    echo "verify: Cargo.toml gained an external dependency" >&2
    echo "better-auth-core is built without a registry and sized to stay" >&2
    echo "small; add what you need to mosql/, uflite/ or nanobase/ instead." >&2
    exit 1
  fi
  echo "no external dependencies: ok"
fi

if run publish; then
  if (( QUICK )); then
    echo "== publish: skipped (--quick) =="
  else
    echo "== publish --dry-run =="
    # The path dependencies are unpublished, so this is the one expected
    # failure until they are on crates.io. Anything else is real.
    if ! output="$(cargo publish --dry-run 2>&1)"; then
      if [[ "$output" == *"no matching package named"* ||
            "$output" == *"does not specify a version"* ]]; then
        echo "publish: blocked only by mosql/uflite/nanobase not being on crates.io yet"
      else
        echo "$output"
        exit 1
      fi
    fi
  fi
fi

if run size; then
  echo "== size =="
  cargo build --release --quiet
  # Windows names the binary .exe; both are the same file.
  BIN=target/release/better-auth-core
  [[ -f "$BIN" ]] || BIN=target/release/better-auth-core.exe
  SIZE=$(wc -c < "$BIN")
  # The budget is loose: this build carries the HTTP server, the JWT
  # signer, TOTP, AES-GCM, the OAuth client and the whole route table.
  # It is a tripwire for a dependency, not a target to shave bytes against.
  BUDGET=$((3 * 1024 * 1024))
  echo "release binary: ${SIZE} bytes"
  if (( SIZE > BUDGET )); then
    echo "verify: ${SIZE} bytes exceeds the ${BUDGET}-byte budget" >&2
    exit 1
  fi
  echo "size: ok (${SIZE} bytes, budget ${BUDGET})"
fi

echo "verify: ok"
