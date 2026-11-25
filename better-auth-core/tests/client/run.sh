#!/usr/bin/env bash
# The client compatibility tests: the published Better Auth client against
# the server this crate builds.
#
#   ./run.sh            build if needed, install if needed, run the tests
#   ./run.sh --keep     leave the server running afterwards (for poking at)
#
# The server is the release binary — the one that ships — started on a
# free port with a throwaway database.
set -euo pipefail
cd "$(dirname "$0")"

CRATE="$(cd ../.. && pwd)"

if ! command -v node >/dev/null 2>&1; then
  echo "client tests: node is not installed" >&2
  exit 2
fi

if [[ ! -d node_modules ]]; then
  echo "== npm install =="
  npm install --no-audit --no-fund
fi

if [[ ! -x "$CRATE/target/release/better-auth-core" ]]; then
  echo "== cargo build --release =="
  (cd "$CRATE" && cargo build --release)
fi

echo "== node --test =="
exec node --test compatibility.test.mjs
