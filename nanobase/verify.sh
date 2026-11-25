#!/usr/bin/env bash
# The gate for the nanobase platform crate, mirroring uflite/verify.sh and
# schemaless/verify.sh: formatting, clippy, the tests, a dependency check, a
# publish dry-run, and a cap on the release binary's size.
#
#   ./verify.sh              every stage
#   ./verify.sh --quick      every stage except the publish dry-run
#   ./verify.sh <stage>      just that stage
#
# Stages: fmt, clippy, test, serverless, deps, publish, size
#
# The stages are separately addressable so CI can give each one its own step:
# a red X next to "clippy" is worth more than one next to "verify", and the
# sandbox this was written in cannot read Actions logs at all.
#
# The size check is the point of this crate. The whole platform is meant to
# ship as one small binary, and a dependency that quietly doubles it is a
# regression even when every test still passes.
set -euo pipefail
cd "$(dirname "$0")"

STAGES=(fmt clippy test serverless deps publish size)
QUICK=0
ONLY=()

for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    fmt|clippy|test|serverless|deps|publish|size) ONLY+=("$arg") ;;
    *) echo "verify: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done
(( ${#ONLY[@]} )) || ONLY=("${STAGES[@]}")

run() { [[ " ${ONLY[*]} " == *" $1 "* ]]; }

# Run one stage's command, keeping what it printed. On failure, the lines
# that name the problem are repeated as step annotations. Actions logs cannot
# be read from where this is developed; annotations can, through the API. Ten
# is the most a step may emit.
stage() {
  local name=$1 log status
  shift
  log=$(mktemp)
  set +e
  "$@" 2>&1 | tee "$log"
  status=${PIPESTATUS[0]}
  set -e
  if (( status != 0 )); then
    # The lines that say what went wrong come first — a failing test, a
    # panic and its message, a compiler diagnostic, a build-script marker —
    # then cargo's own error lines with context. Duplicates dropped, ten at
    # most: that is what a step may emit. Backticks and CRs do not survive
    # the trip as annotations.
    local important errors
    important='\[nanobase\]|fatal error|error C[0-9]+|LNK[0-9]+|Could not find|cannot open|not recognized|^test .* \.\.\. FAILED|^---- |panicked at|assertion .*failed|^  (left|right):'
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
  stage test cargo test --tests
fi

if run serverless; then
  echo "== serverless =="
  # The second edition is a different binary, so it gets its own compile
  # and its own tests: a feature gate that quietly removes the listener
  # from one build and not the other is exactly the kind of mistake these
  # two editions exist to prevent.
  stage serverless-clippy \
    cargo clippy --all-targets --no-default-features --features serverless \
    -- -D warnings
  stage serverless-test \
    cargo test --no-default-features --features serverless
fi

if run deps; then
  echo "== no external dependencies =="
  # Both path dependencies are in-repo and themselves dependency-free, and
  # SQLite is compiled from source rather than pulled in as a crate, so the
  # binary has nothing in it that was not chosen line by line. An external
  # dependency would change that, and would grow it.
  if sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
      | grep -vE '^\[|^$|^#|path *=' \
      | grep -q .; then
    echo "verify: Cargo.toml gained an external dependency" >&2
    echo "nanobase is built without a registry and sized to stay small;" >&2
    echo "add what you need to mosql/ or uflite/ instead." >&2
    exit 1
  fi
  echo "no external dependencies: ok"
fi

if run publish; then
  if (( QUICK )); then
    # `cargo publish` resolves the path dependencies against the crates.io
    # index, which an offline sandbox cannot reach; CI's publish dry-run is
    # the gate.
    echo "== publish: skipped (--quick) =="
  else
    echo "== publish --dry-run =="
    # Both path dependencies are unpublished, so this is the one expected
    # failure until they are on crates.io. Anything else is real.
    if ! output="$(cargo publish --dry-run 2>&1)"; then
      if [[ "$output" == *"no matching package named"* ||
            "$output" == *"does not specify a version"* ]]; then
        echo "publish: blocked only by mosql/uflite not being on crates.io yet"
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
  BIN=target/release/nanobase
  SIZE=$(wc -c < "$BIN")
  # The budget is deliberately loose: this build already carries the query
  # builder, the in-memory engine, ACL evaluation and storage. It is a
  # tripwire for a dependency, not a target to shave bytes against.
  BUDGET=$((3 * 1024 * 1024))
  echo "release binary: ${SIZE} bytes"
  if (( SIZE > BUDGET )); then
    echo "verify: ${SIZE} bytes exceeds the ${BUDGET}-byte budget" >&2
    exit 1
  fi
  echo "size: ok (${SIZE} bytes, budget ${BUDGET})"

  # The serverless edition is the one that ships to a small host, so it
  # gets its own budget: it must be smaller than the full one, and it
  # must not grow by accident either.
  cargo build --release --quiet --no-default-features --features serverless \
    --bin nanobase-serverless
  SERVERLESS=target/release/nanobase-serverless
  SL_SIZE=$(wc -c < "$SERVERLESS")
  SL_BUDGET=$((2 * 1024 * 1024))
  echo "serverless binary: ${SL_SIZE} bytes"
  if (( SL_SIZE > SL_BUDGET )); then
    echo "verify: ${SL_SIZE} bytes exceeds the ${SL_BUDGET}-byte budget" >&2
    exit 1
  fi
  if (( SL_SIZE >= SIZE )); then
    echo "verify: the serverless binary (${SL_SIZE}) is not smaller than the full one (${SIZE})" >&2
    exit 1
  fi
  echo "size: ok (${SL_SIZE} bytes, budget ${SL_BUDGET}, smaller than ${SIZE})"
fi

echo "verify: ok"
