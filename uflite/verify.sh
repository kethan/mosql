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
  echo "== package: skipped (--quick) =="
else
  echo "== publish --dry-run =="
  cargo publish --dry-run
fi

echo "verify: ok"
