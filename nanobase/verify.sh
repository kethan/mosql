#!/usr/bin/env bash
# The gate for the nanobase platform crate, mirroring uflite/verify.sh and
# schemaless/verify.sh: formatting, clippy, the tests, a dependency check, a
# publish dry-run, and a cap on the release binary's size.
#
#   ./verify.sh              every stage
#   ./verify.sh --quick      every stage except the publish dry-run
#   ./verify.sh <stage>      just that stage
#
# Stages: fmt, clippy, test, deps, publish, size
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

if run fmt; then
  echo "== fmt =="
  cargo fmt --check
fi

if run clippy; then
  echo "== clippy =="
  cargo clippy --all-targets -- -D warnings
fi

if run test; then
  echo "== test =="
  cargo test --tests
fi

if run deps; then
  echo "== no external dependencies =="
  # A dependency on an external crate would make the binary depend on a
  # registry this project is built without, and would grow it. Both path
  # dependencies are in-repo and themselves dependency-free, which is what
  # keeps this true.
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
      if [[ "$output" == *"no matching package named \`mosql\` found"* ||
            "$output" == *"no matching package named \`uflite\` found"* ]]; then
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
fi

echo "verify: ok"
