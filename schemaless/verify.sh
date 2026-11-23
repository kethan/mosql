#!/usr/bin/env bash
# The gate for the schema-less collection crate, mirroring uflite/verify.sh:
# formatting, clippy, the tests, and a publish dry-run.
#
#   ./verify.sh           everything (needs the crates.io index for the last step)
#   ./verify.sh --quick   skip the publish dry-run (offline sandboxes)
set -euo pipefail
cd "$(dirname "$0")"

echo "== fmt =="
cargo fmt --check

echo "== clippy =="
cargo clippy --all-targets -- -D warnings

echo "== test =="
cargo test --tests

if [[ "${1:-}" == "--quick" ]]; then
  # `cargo package` resolves the `mosql` and `uflite` versions against the
  # crates.io index, which an offline sandbox cannot reach; CI's publish
  # dry-run is the gate.
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

echo "verify: ok"
