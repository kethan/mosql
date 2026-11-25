#!/usr/bin/env bash
# The gate for the Worker crate, mirroring better-auth-core/verify.sh:
# formatting, clippy, the tests, a dependency check, and a build of the
# WebAssembly module with a check that it asks the host for nothing but
# the clock and the randomness.
#
#   ./verify.sh              every stage
#   ./verify.sh --quick      every stage except the module build
#   ./verify.sh <stage>      just that stage
#
# Stages: fmt, clippy, test, deps, module
#
# The stages are separately addressable so CI can give each one its own
# step: a red X next to "clippy" is worth more than one next to "verify".
set -euo pipefail
cd "$(dirname "$0")"

STAGES=(fmt clippy test deps module)
QUICK=0
ONLY=()

for arg in "$@"; do
  case "$arg" in
    --quick) QUICK=1 ;;
    fmt|clippy|test|deps|module) ONLY+=("$arg") ;;
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
  # The protocol test plays the part of the host and answers the engine's
  # questions from a real SQLite database, so the statements it writes are
  # checked against a real engine.
  stage test cargo test
fi

if run deps; then
  echo "== no external dependencies =="
  # The three path dependencies are in-repo and themselves dependency-free,
  # so the module has nothing in it that was not chosen line by line.
  if sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml \
      | grep -vE '^\[|^$|^#|path *=' \
      | grep -q .; then
    echo "verify: Cargo.toml gained an external dependency" >&2
    echo "the Worker is built without a registry; add what you need to" >&2
    echo "mosql/, uflite/ or nanobase/ instead." >&2
    exit 1
  fi
  echo "no external dependencies: ok"
fi

if run module; then
  if (( QUICK )); then
    echo "== module: skipped (--quick) =="
  else
    echo "== module =="
    # The module is built for the WebAssembly target and checked for what
    # it asks the host for. A Worker has no socket, no file and no clock
    # of its own, so the only imports it may have are the two the host
    # provides: the clock and the randomness.
    stage module ./build.sh
    python3 - <<'PY'
import struct, sys

path = "js/mosql_worker.wasm"
data = open(path, "rb").read()
if data[:4] != b"\x00asm":
    sys.exit(f"{path} is not a WebAssembly module")


def uleb(d, i):
    result = 0
    shift = 0
    while True:
        byte = d[i]
        i += 1
        result |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return result, i
        shift += 7


pos = 8
imports = []
while pos < len(data):
    section = data[pos]
    pos += 1
    size, pos = uleb(data, pos)
    end = pos + size
    if section == 2:
        count, p = uleb(data, pos)
        for _ in range(count):
            length, p = uleb(data, p)
            module = data[p : p + length].decode()
            p += length
            length, p = uleb(data, p)
            name = data[p : p + length].decode()
            p += length
            kind = data[p]
            p += 1
            if kind == 0:
                _, p = uleb(data, p)
            elif kind == 1:
                p += 1
                flags = data[p]
                p += 1
                _, p = uleb(data, p)
                if flags & 1:
                    _, p = uleb(data, p)
            elif kind == 2:
                flags = data[p]
                p += 1
                _, p = uleb(data, p)
                if flags & 1:
                    _, p = uleb(data, p)
            elif kind == 3:
                p += 2
            imports.append((module, name))
    pos = end

allowed = {("host", "host_now_ms"), ("host", "host_random")}
unexpected = [i for i in imports if i not in allowed]
for module, name in imports:
    print(f"  import {module}.{name}")
if unexpected:
    for module, name in unexpected:
        print(f"verify: the module imports {module}.{name}, which the host does not provide")
    sys.exit(1)
missing = allowed - set(imports)
if missing:
    for module, name in sorted(missing):
        print(f"verify: the module does not import {module}.{name}")
    sys.exit(1)
print("the module asks the host for the clock and the randomness, and for nothing else")
PY
  fi
fi

echo "verify: ok"
