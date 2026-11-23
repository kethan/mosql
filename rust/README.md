# mosql — Rust port

A zero-dependency Rust port of [`umosql`](https://github.com/kethan/mosql): build
PostgreSQL / MySQL / SQLite SQL from MongoDB-style queries, plus the in-memory
Mongo-style engine. It lives in this folder so the JavaScript package is
untouched.

```
rust/
├── src/
│   ├── lib.rs            crate root, `Db` dialect enum, re-exports
│   ├── builder.rs        `Builder` / `Collection` / `FindQuery` (the query builder)
│   ├── ops.rs            filter + expression operators
│   ├── stages.rs         aggregation stage handlers for the SQL builder
│   ├── json_path.rs      `jsonPath` / `jsonUpdate` / `cast`
│   ├── escape.rs         identifier escaping per dialect
│   ├── validate.rs       identifier validation, MySQL reserved words
│   ├── value.rs          `Value`, ordered `Object`, JSON parse/stringify
│   ├── float.rs          `no_std` replacements for libm (`floor`, `pow`, `sqrt`, …)
│   ├── dispatch.rs       embedding-neutral request dispatcher (shared by wasm + N-API)
│   ├── memory/           the in-memory engine
│   │   ├── eval.rs       filter/expression evaluation, `getPath`/`setPath`
│   │   ├── update.rs     `$set`/`$inc`/`$push`/… update operators
│   │   ├── stages.rs     `$match`/`$group`/`$project`/`$bucket`/`$unwind`/…
│   │   ├── collection.rs `Collection`, `FindQuery` cursor, `Database`
│   │   └── regex.rs      dependency-free backtracking regex engine
│   ├── wasm.rs           hand-written wasm ABI (no wasm-bindgen)
│   ├── wasm_alloc.rs     bump + free-list allocator for the wasm build
│   ├── node_api.rs       hand-written Node-API FFI (no napi-rs, no build script)
│   └── napi.rs           the addon surface
├── wasm/                 `mosql.js` bridge + built modules (gitignored)
├── node/                 `mosql.js` bridge + built addon (gitignored)
├── bench/                JS / addon / wasm / Rust benchmark scripts
└── tests/                differential parity suites + fixture generators
```

## Verdict

**A complete migration is feasible, and the port is done and verified** — but the
speed answer depends entirely on *where* you run it.

| Surface | Ported | Parity |
| --- | --- | --- |
| Core query builder (`src/index.js`, 1095 lines) | ✅ | **289/289** fixtures, byte-identical SQL *and* error messages |
| `lite` / `tiny` operator allowlists | ✅ `Builder::lite()` / `Builder::tiny()` | included in the 289 |
| In-memory engine (`src/adapter/memory/memory.js`, 1100 lines) | ✅ | **153/153** fixtures, identical JSON |
| `$where` (takes a JS function) | ❌ rejected | by design — cannot run JS in Rust |
| `src/drivers.js`, `src/client.js`, `src/schemaless.js` | ⚠️ not ported | JS *runtime adapters* (MySQL2/pg/sql.js, HTTP client). Out of scope by decision — see [Caveats](#caveats) |

Everything is `#![no_std]` + `alloc` (native builds link `std`), **zero
dependencies**, and builds for `wasm32-unknown-unknown` without `wasm-bindgen` or
`wasm-pack`, and as a Node addon without `napi-rs` or a build script.

## Benchmarks

Every case is **best of 5**; the machine is shared, so single readings swing by
±25%. Ratios are more trustworthy than absolute numbers. Node v22.22.3, Rust
1.97.0, release profile.

### SQL builder — 20,000 iterations

| op | Rust (native) | JavaScript | Node addon | WASM |
| --- | ---: | ---: | ---: | ---: |
| `find` | **2.7 µs** | 4.5 µs | 12.6 µs | 18.1 µs |
| `update` | **3.0 µs** | 7.0 µs | 12.2 µs | 21.1 µs |
| `aggregate` | **3.5 µs** | 8.0 µs | 15.3 µs | 23.8 µs |
| `insertMany(50)` | 23.8 µs | **22.6 µs** | 80.5 µs | 80.3 µs |

### In-memory engine — 10,000 documents

| op | Rust (native) | JavaScript | Node addon | WASM |
| --- | ---: | ---: | ---: | ---: |
| `find` (scan, return ~5k) | **3.9 ms** | 10.7 ms | 14.1 ms | 24.0 ms |
| `find` + sort + limit 10 | 5.3 ms | 19.7 ms | **5.3 ms** | 14.9 ms |
| `aggregate` (`$match`+`$group`+`$sort`) | 4.6 ms | 6.5 ms | **4.7 ms** | 10.2 ms |
| `updateMany` | **3.8 ms** | 18.7 ms | — | — |
| `insertOne` | 893 ns | **777 ns** | 5.6 µs | 15.1 µs |

### Binary size

| artefact | raw | gzip |
| --- | ---: | ---: |
| `src/index.js` (JS builder) | 43.6 KB | **10.6 KB** |
| `src/adapter/memory/memory.js` | 39.8 KB | 8.4 KB |
| `mosql-core.wasm` (builder only) | 187 KB | 71.6 KB |
| `mosql.wasm` (builder + memory engine) | 301 KB | 106.2 KB |
| `mosql.node` (addon, links `std`) | 716 KB | 339.1 KB |

### Reading the numbers

1. **Native Rust is 2–5× faster than JavaScript — not 10×.** `updateMany` 5.0×,
   `find+sort` 3.7×, `find` 2.7×, `$group` aggregate 1.4×, builder 1.5–2.3×.
   10× is not available in the SQL builder: generating a ~90-byte `SELECT` is
   ~3 µs of work in Rust and ~4.5 µs in V8, and the remaining time is string
   allocation that both runtimes do at similar cost. The 10× lives in the
   *engine* — filtering, grouping and updating documents — where the work per
   call is large enough for Rust's lack of GC and tighter memory layout to
   compound. If you need 10×, that is where to spend effort, and the current
   engine is still a naive `Value`-tree implementation (an arena or `Rc`-based
   document type would go further).
2. **The Node addon is only faster when little data crosses the boundary.**
   `find+sort` returning 10 of 10,000 documents is **3.7× faster** than JS;
   `find` returning ~5,000 documents is *slower*, because building 5,000 JS
   objects through N-API costs more than the scan saves. `insertOne` is 7×
   slower for the same reason — the per-call conversion overhead dwarfs a
   500 ns insert.
3. **The addon is a clear loss on the SQL builder.** Building a query is ~3 µs
   of Rust, and converting the JS request object into a Rust `Value` graph costs
   ~9 µs — three times the work it saves. The builder is too small to amortise
   any boundary.
4. **WASM loses everywhere.** 2.5–5× slower than JS and 6.7× larger gzipped.

**So the honest recommendation is narrower than "Rust makes it fast":**

- Use the **Rust crate** where you control the runtime (services, CLIs, data
  pipelines) — that is where the 2–5× is real.
- Use the **Node addon** for the in-memory engine on large collections with
  selective queries. Do **not** route the SQL builder through it.
- Treat **WASM** as a portability option only.

## Using mosql from another Rust app

mosql is a **library that emits SQL strings** — it does not contain a database
driver, and it does not need one. Your application executes the SQL with
whatever driver it already uses. That keeps this table short:

```toml
[dependencies]
mosql = "0.1"        # zero dependencies, no features to turn on
```

### Measured footprint

Marginal cost over an otherwise empty Rust binary, measured on this machine:

| configuration | raw | gzip | cold build |
| --- | ---: | ---: | ---: |
| empty binary (`std` only) — baseline | 437,960 B | 185,544 B | 0.2 s |
| **+ mosql query builder** | **+232,184 B** | **+99,849 B** | +7.5 s |
| **+ mosql builder + memory engine** | **+376,144 B** | **+160,683 B** | +7.2 s |
| **+ mosql + SQLite** (system lib, dynamic) | **+238,008 B** | **+102,129 B** | +6.6 s |

With `lto = "fat"` + `codegen-units = 1` + `strip = true` the same numbers drop
to:

| configuration | raw | gzip | cold build |
| --- | ---: | ---: | ---: |
| empty binary — baseline | 300,864 B | 152,536 B | 3.9 s |
| **+ mosql query builder** | **+147,368 B** | **+71,987 B** | +8.4 s |
| **+ mosql builder + memory engine** | **+234,888 B** | **+112,516 B** | +10.2 s |
| **+ mosql + SQLite** (system lib, dynamic) | **+152,960 B** | **+74,438 B** | +9.9 s |

**Turn LTO on — it removes 37% of mosql's binary size**, because a generic
`Value`-walking library leaves a lot of monomorphized dead code behind otherwise.
The compile-time cost is the same order as the saving, so it is a size/time
trade, not a free win.

The memory engine alone adds **+87 KB raw / +41 KB gz** on top of the builder.

### Adding SQLite / Postgres / MySQL

The size is the *driver's*, not mosql's. mosql adds nothing when you add a
driver, because it never links one. Concretely, measured here:

| approach | in-binary cost | notes |
| --- | ---: | --- |
| **link the system `libsqlite3`** | **+5.6 KB** | measured; `libsqlite3.so.0` is 1.4 MB but shared with the OS |
| bundle SQLite (`rusqlite` `bundled`) | ~1.2–1.6 MB | amalgamation compiled in; no system dependency |
| `tokio-postgres` | ~1 MB + tokio | async only |
| `mysql_async` | ~1.5 MB + tokio | async only |
| `sqlx` with all three drivers | ~5–7 MB | ~200 transitive crates, async runtime included |

The last four rows are **estimates from published crate sizes, not measured** —
this sandbox has no crates.io access, so I could not build them. The first row is
measured (`/tmp/sz/sqlite`, a real query through `sqlite3_prepare_v2` with the C
ABI declared by hand).

So the cheapest possible "mosql + SQLite" is **+153 KB raw / +74 KB gz** over an
empty binary, if you link the system SQLite. If you bundle SQLite instead, budget
roughly **+1.5 MB raw** for the pair.

For Postgres and MySQL the pragmatic answer is: **do not put them in mosql.** Use
`sqlx` or `tokio-postgres`/`mysql_async` in your app for the connection, and
mosql only for the SQL. mosql's contribution stays at +147 KB no matter how many
drivers your app has.

### API note

`Builder` is cheap to clone (its allowlists are `Rc<Vec<String>>`), and
`Collection`/`FindQuery` own their builder rather than borrowing it, so queries
chain in a single expression with no `let` binding:

```rust
let sql = Builder::full()
    .collection("users", Db::Pg)?
    .find(&query, None)?
    .sort(&sort)?
    .limit(&Value::Int(10))?
    .to_sql()?;
```

## What was hard

- **`no_std` has no libm.** `f64::floor`, `powf`, `sqrt`, `ceil`, `round`,
  `fract`, `rem_euclid` are all `std`-only. They are reimplemented in
  `float.rs` (`ln` via atanh series + exponent reduction, `exp` via
  range-reduced Taylor, `sqrt` via Newton on a bit-halved seed).
- **JS semantics that are easy to get subtly wrong.** Ordered objects (integer
  keys sort ascending before string keys), `x + y` string coercion, `NaN`
  sorting, `null` ordering — and the fact that the JS engine has *two different*
  projection implementations: `find` uses Mongo inclusion semantics while
  `$project` has its own exclusion/inclusion rules. Both are ported separately
  (`stages::project` and `stages::find_projection`) and the difference is
  covered by fixtures.
- **The JS engine's `$group` evaluates `_id` through `exprOp`,** so a compound
  `_id` document throws `Unknown expression operator: active`. Reproduced rather
  than "fixed", because parity was the goal.
- **`$map` / `$filter` / `$reduce` take positional arrays**, not
  `{input, as, in}` objects. Reproduced.
- **Regex.** `$regex` needs JS-flavoured patterns. `regex.rs` is a
  dependency-free backtracking engine supporting literals, `.`, `*`, `+`, `?`,
  `{m,n}`, `^`, `$`, `|`, groups, classes, `\d \D \w \W \s \S`, lazy quantifiers,
  and the `i` flag.
- **Node-API without a binding crate.** `node_api.rs` declares the `napi_*`
  symbols directly; Node exports them and resolves them at `dlopen` time. Three
  ABI details cost real debugging time: `napi_get_cb_info`'s `argc` is
  `size_t*` (not `int*`), and `napi_set_named_property` /
  `napi_throw_error` take **NUL-terminated** C strings while the `_utf8`
  functions take counted ones.
- **The wasm allocator.** `wasm32-unknown-unknown` has none. `wasm_alloc.rs` is
  a bump allocator with an exact-size free cache: every block is rounded to a
  16-byte granule and bucketed by granule count, so a lookup is one array index
  and can never return a wrong-size block. While no collection is open it stays
  in *transient* mode and skips the free lists entirely.

## Usage

### As a Rust crate

```rust
use mosql::{parse_json, Builder, Db, Value};

// Mongo-style query and sort, exactly as you would write them in JSON.
let query = parse_json(r#"{"age": {"$gte": 18}, "role": "admin"}"#)?;
let sort = parse_json(r#"{"age": -1}"#)?; // -1 = descending, 1 = ascending

let sql = Builder::full()
    .collection("users", Db::Pg)?
    .find(&query, None)?
    .sort(&sort)?
    .limit(&Value::Int(10))?
    .to_sql()?;
// SELECT * FROM users WHERE age >= 18 AND role = 'admin' ORDER BY age DESC NULLS LAST LIMIT 10
```

Every chain method returns `Result`, so `?` works at every step. `None` is the
projection; pass a `parse_json(r#"{"name": 1}"#)?` to select fields.

```rust
use mosql::memory::Collection;
use mosql::{parse_json, Value};

let query = parse_json(r#"{"age": {"$gte": 18}}"#)?;
let sort = parse_json(r#"{"age": -1}"#)?;

let coll = Collection::new(
    "users",
    vec![parse_json(r#"{"name": "Ada", "age": 36}"#)?, parse_json(r#"{"name": "Bob", "age": 17}"#)?],
    Default::default(),
);
let hits: Vec<Value> = coll.find(&query).sort(&sort).limit(10).to_vec()?;
assert_eq!(hits.len(), 1);
```

The engine's chain is infallible (`sort(&Value) -> Self`, `limit(n) -> Self`);
only `to_vec()` returns a `Result`.

### As a Node addon

`node/mosql.js` mirrors `wasm/mosql.js`, so the same code drives either backend.

```js
import { loadMosql } from './mosql.js';

const mosql = loadMosql();

// Same shape as the JS builder.
mosql.collection('users', 'pg').find({ age: { $gte: 18 } }).toSQL();

// Stateful in-memory engine — documents never leave Rust.
const mem = mosql.openMemory();
mem.insert([{ name: 'Ada', age: 36 }, { name: 'Bob', age: 17 }]);
mem.find({ age: { $gte: 18 } }, { sort: { age: -1 }, limit: 10 });
mem.aggregate([{ $group: { _id: '$active', n: { $sum: 1 } } }]);
```

### As a WASM module

`wasm/mosql.js` is a **zero-dependency** bridge — no `wasm-bindgen`, no
`wasm-pack`, no npm install. `loadMosql()` accepts a URL, a `URL`, or a
`Uint8Array`, and the module imports nothing, so it loads in Node, Deno,
browsers, and edge runtimes.

```js
import { loadMosql } from './wasm/mosql.js';

const mosql = await loadMosql('./wasm/mosql-core.wasm');
mosql.collection('users', 'pg').find({ age: { $gte: 18 } }).toSQL();

const mem = mosql.openMemory();
mem.insert([{ name: 'Ada', age: 36 }]);
```

## Running it

```bash
cd rust

./verify.sh                         # every check CI runs, in one place
./verify.sh --quick                 # skip clippy and packaging
./publish.sh --dry-run              # verify, then a dry-run publish
./publish.sh                        # verify, then publish for real

cargo test --tests                  # 289 builder + 153 memory parity fixtures
cargo run --release --bin bench     # native builder benchmark
cargo run --release --bin mem-bench # native engine benchmark

./build-wasm.sh                     # -> wasm/mosql.wasm, wasm/mosql-core.wasm
./build-node.sh                     # -> node/mosql.node
node wasm/test.mjs                  # replay the corpus through the wasm ABI
node node/test.mjs                  # replay the corpus through the addon

ROUNDS=5 ./bench/run-all.sh         # one comparison table, Rust vs JS vs addon vs wasm
```

`verify.sh` is the single source of truth — CI and `publish.sh` both call it, so
they cannot drift:

| step | what it proves |
| --- | --- |
| `cargo fmt --check` | formatting |
| `cargo clippy --all-targets -- -D warnings` | no lints |
| `cargo build --all-targets` with `-D warnings` | zero warnings, every target |
| feature matrix | default, `napi` (link-tested on Linux, compile-checked elsewhere), and `no_std`+`memory` each build clean |
| `no_std` (bare metal) | links a real thumbv7em binary that supplies its own allocator |
| `wasm32` (both variants) | the `no_std` wasm modules still build |
| `cargo test --tests` | parity + documented examples |
| fixture drift | regenerating the corpus from JS is a no-op |
| `cargo package` | the manifest is publishable, and the tarball stays small |

CI (`.github/workflows/rust.yml`) runs the same script on Linux, macOS and
Windows, plus a dedicated `no_std` job, a job that builds the wasm modules and
the Node addon and replays the corpus through both, and a `cargo publish
--dry-run` gate against the live crates.io index.

Regenerate the fixture corpus from the JavaScript implementation:

```bash
node tests/gen-fixtures.mjs        # -> tests/fixtures.json
node tests/gen-memory-fixtures.mjs # -> tests/memory-fixtures.json
```

## How parity is proven

Both suites are **differential**: the JavaScript implementation is the oracle.

- `tests/parity.rs` replays `tests/fixtures.json` — 289 cases covering every
  operator, all three dialects, and the `full`/`lite`/`tiny` allowlists. It
  compares SQL text **and** error strings.
- `tests/memory_parity.rs` replays `tests/memory-fixtures.json` — 153 cases
  across filter, expression, update, aggregate, and an 18-step collection script
  (insert → find → update → upsert → replace → delete). Results are compared as
  JSON, so key *order* is part of the contract.
- `wasm/test.mjs` and `node/test.mjs` replay **both** corpora through their
  respective ABIs, using the shared `tests/replay.mjs`, so the wasm boundary and
  the N-API boundary are held to the same bar as the native crate.

If the JS implementation changes, re-run the generators and the Rust suite will
tell you exactly which fixture drifted.

## Caveats

- **No `LICENSE` file exists in the repository.** The manifest declares
  `license = "ISC"` (matching `package.json`), so `cargo publish` succeeds, but a
  published crate normally ships the licence text too.
- The driver/client layer (`src/drivers.js`, `src/client.js`,
  `src/schemaless.js`) is not ported. It is I/O glue rather than query logic and
  would pull in async + TLS + socket crates, which conflicts with the
  zero-dependency goal.
- **The addon is not a universal win.** Measure your own workload: it pays off
  when a call does a lot of work and returns few values, and costs you when a
  call is small or returns many values.
- The wasm build is single-threaded by construction; `static mut` holds the
  collection registry. That is sound here but must not be shared across threads
  if the module is ever instantiated with shared memory.
- `mosql.node` links `std`, so it is 716 KB raw where the `no_std` wasm build is
  187 KB. Making the native addon `no_std` too would need a `#[global_allocator]`
  and a `#[panic_handler]` on a platform that already provides `malloc`, which
  is a poor trade.
- The memory engine's aggregate pipeline has no `$sample`. That matches the JS
  engine, which only implements `$sample` in the *SQL* builder (ported in
  `src/stages.rs`).
- A pipeline whose first stage is not `$match` still clones its input documents
  once. A shared `Rc<Value>` document type would remove that clone entirely.
- Memory usage of the wasm engine is ~1 KB per small document, because `Vec`
  growth and the exact-size free cache both over-allocate. A slab allocator
  would bring this down if it matters.
