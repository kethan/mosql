#!/usr/bin/env bash
# The gate for the uflite service-layer crate, mirroring rust/verify.sh:
# formatting, clippy, the spec ports, and a publish dry-run.
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
  # `cargo package` resolves the `mosql` version against the crates.io index,
  # which an offline sandbox cannot reach; CI's publish dry-run is the gate.
  echo "== publish: skipped (--quick) =="
else
  echo "== publish --dry-run =="
  # uflite depends on the mosql crate, which is not on crates.io until it is
  # published. Until then this is the one expected failure; anything else is real.
  if ! output="$(cargo publish --dry-run 2>&1)"; then
    if [[ "$output" == *"no matching package named \`mosql\` found"* ]]; then
      echo "publish: blocked only by mosql not being on crates.io yet"
    else
      echo "$output"
      exit 1
    fi
  fi
fi

echo "verify: ok"
