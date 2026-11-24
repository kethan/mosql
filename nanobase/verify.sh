#!/usr/bin/env bash
# The gate for the nanobase platform crate, mirroring uflite/verify.sh and
# schemaless/verify.sh: formatting, clippy, the tests, a publish dry-run, and a
# check on the release binary's size.
#
#   ./verify.sh           everything (needs the crates.io index for the dry-run)
#   ./verify.sh --quick   skip the publish dry-run (offline sandboxes)
#
# The size check is the point of this crate. The whole platform is meant to
# ship as one small binary, and a dependency that quietly doubles it is a
# regression even when every test still passes.
set -euo pipefail
cd "$(dirname "$0")"

echo "== fmt =="
cargo fmt --check

echo "== clippy =="
cargo clippy --all-targets -- -D warnings

echo "== test =="
cargo test --tests

echo "== no external dependencies =="
# A dependency on an external crate would make the binary depend on a registry
# this project is built without, and would grow it. Both path dependencies are
# in-repo and themselves dependency-free, which is what keeps this true.
if sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml | grep -vE '^\[|^$|^#|path *=' | grep -q .; then
  echo "verify: Cargo.toml gained an external dependency" >&2
  echo "nanobase is built without a registry and sized to stay small;" >&2
  echo "add what you need to mosql/ or uflite/ instead." >&2
  exit 1
fi
echo "no external dependencies: ok"

if [[ "${1:-}" == "--quick" ]]; then
  # `cargo publish` resolves the path dependencies against the crates.io
  # index, which an offline sandbox cannot reach; CI's publish dry-run is the
  # gate.
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

echo "== size =="
cargo build --release --quiet
BIN=target/release/nanobase
SIZE=$(wc -c < "$BIN")
# The budget is deliberately loose: this build already carries the query
# builder, the in-memory engine, ACL evaluation and storage. It is a tripwire
# for a dependency, not a target to shave bytes against.
BUDGET=$((3 * 1024 * 1024))
echo "release binary: ${SIZE} bytes"
if (( SIZE > BUDGET )); then
  echo "verify: ${SIZE} bytes exceeds the ${BUDGET}-byte budget" >&2
  exit 1
fi
echo "size: ok (${SIZE} bytes, budget ${BUDGET})"

echo "verify: ok"
