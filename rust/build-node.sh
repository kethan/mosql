#!/usr/bin/env bash
# Build the Node-API addon -> node/mosql.node
set -euo pipefail
cd "$(dirname "$0")"

case "$(uname -s)" in
  Darwin*)
    # The `napi_*` symbols live in the node executable and are resolved when
    # Node dlopens the addon. Apple's linker rejects a dylib with undefined
    # symbols unless told to look them up at load time — the same flag pyo3 and
    # napi-rs pass for their host-executable-resolved symbols.
    # Untested: this sandbox has no macOS runner.
    export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-Wl,-undefined,dynamic_lookup"
    src=target/release/libmosql.dylib ;;
  *) src=target/release/libmosql.so ;;
esac

cargo build --release --features napi >/dev/null

cp "$src" node/mosql.node
printf '%-16s %10s bytes (%7s gzip)\n' "mosql.node" \
  "$(wc -c < node/mosql.node | tr -d ' ')" \
  "$(gzip -9 -c node/mosql.node | wc -c | tr -d ' ')"
