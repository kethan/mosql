#!/usr/bin/env bash
# Run every benchmark and print one comparison table.
#
# Every case is best-of-N (ROUNDS, default 3) because the machine is shared.
set -euo pipefail
cd "$(dirname "$0")/.."
export ROUNDS="${ROUNDS:-3}"

echo "## SQL builder — 20k iterations, best of $ROUNDS"
echo
echo "### Rust (native)"
cargo run --release --bin bench 2>/dev/null
echo
echo "### JavaScript"
node --expose-gc bench/node-bench.mjs
echo
echo "### Node addon (Rust via N-API)"
node --expose-gc bench/native-bench.mjs
echo
echo "### WASM (builder-only module)"
WASM=../wasm/mosql-core.wasm node --expose-gc bench/wasm-bench.mjs | tail -n +2
echo
echo "## In-memory engine — 10k documents, best of $ROUNDS"
echo
echo "### Rust (native)"
cargo run --release --bin mem-bench 2>/dev/null
echo
echo "### JavaScript"
node --expose-gc bench/node-mem-bench.mjs
echo
echo "### Node addon (Rust via N-API)"
node --expose-gc bench/native-mem-bench.mjs
echo
echo "### WASM"
node --expose-gc bench/wasm-mem-bench.mjs
