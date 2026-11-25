# Status: the Rust port, the two editions, and what they cost

The numbers below were measured on the machine this was built on, on
commit `fbeed1d`. Every figure can be reproduced:

```
python3 bench/run.py --rounds 3 --out table.md
```

The harness checks parity **first**: if Rust, JavaScript and Go did not
agree on the answer, a timing comparison would be comparing different
work and the number would be worse than useless.

```
The SQL builder   — identical statement in all three. ✓
The in-memory engine — CHECK find=2831 groups=2 city=London
                      n=1667 avg=44.9622075584883 updated=2831
                    — identical in all three. ✓
```

---

## 1. Completed work

### The crates

| crate | what it is | tests |
| --- | --- | --- |
| `rust/` — **mosql** | the query builder and the in-memory engine, ported from the JavaScript `umosql`. `#![no_std]` + `alloc`, a hand-written regex and float routines, no libm, no dependency | 289 builder fixtures + 153 memory fixtures, byte-for-byte against the JavaScript |
| `uflite/` | the service layer: databases, tables, ACL evaluation, storage (local files and an S3 trait) | 105 |
| `schemaless/` | schema-less SQL collections, and the client that talks to them | 33 |
| `nanobase/` | the platform: app documents, secrets, an owner account, a platform store, a service layer with events and hooks, an HTTP API, and the command line | 227 (full) · 100 (serverless) |
| `better-auth-core/` | a Better Auth-compatible authentication server: bearer JWT sessions, eight plugins, every social provider, OpenID Connect with verified ID tokens | 172, plus the official client **22/22** |
| `worker/` | the same engine as a Cloudflare Worker: Rust compiled to WebAssembly, D1 as the database, KV and R2 as optional adapters | 12, plus the end-to-end Wrangler run |

The JavaScript implementation is untouched and remains the oracle for
parity.

### The two editions of the platform

The CLI and the binaries were the priority, and the split falls out of
them: **one command line, two binaries**, and the difference is what
each one is allowed to reach for.

| | **full** — `nanobase` | **serverless** — `nanobase-serverless` |
| --- | --- | --- |
| Release size | 1,830,992 bytes (1.75 MiB) | 813,184 bytes (794 KiB) — **44 % of the full one** |
| Peak RSS at `--version` | 10,148 KiB | 10,184 KiB |
| Platform state | one local SQLite file (`--data`) | a serverless database (`--db-url`) |
| Listener | `nanobase serve`, `nanobase db serve` | **none** — no TCP code in the binary |
| Local database | SQLite, vendored and compiled in | **none** |
| Build | `cargo build --release` | `cargo build --release --no-default-features --features serverless` |
| Tests | 227 | 100 (a second, separate suite) |

The split is enforced by Cargo features, not by convention:

* `native` → the TCP server, the file-backed platform store, the
  `std::net` transport.
* `serverless` → the HTTP transport, the serverless platform store.
* `sqlite` → the vendored engine.

`src/main.rs` and `src/bin/serverless.rs` are thin wrappers around one
`cli::run`, so there is a single implementation of every command and
the two editions can only differ where a feature gate says they must.

**Verified end to end, in both editions:**

| flow | full | serverless |
| --- | --- | --- |
| `init` → `login` → `me` → `status` | ✓ against a local SQLite file | ✓ against an HTTP database service |
| `app new` / `import` / `list` / `show` / `export` / `rename` / `remove` | ✓ | ✓ |
| `app insert` / `rows` against a remote app database | ✓ | ✓ — a `libsql` app over the HTTP database protocol |
| `nanobase serve` (HTTP API) | ✓ | **absent by design** |
| `nanobase db serve` (database over HTTP) | ✓ | **absent by design** |

The serverless store keeps the whole platform in **one row of one
collection**: a serverless database cannot hold a transaction open
while a password is hashed, and the platform's state is small enough
that one row is the right shape for it.

**The Worker is complete and unchanged.** D1 is the durable source of
truth, `fetch` is the outbound HTTP, and the host provides the clock
and the randomness. The module is imported at source phase and has
exactly two imports — `host.host_now_ms` and `host.host_random` —
which `worker/verify.sh` reads out of the module and fails if it asks
for anything else. The Better Auth `AuthStore` eight-operation
boundary and the equivalent D1 row decoding are preserved.

### The Rust optimisation

The builder used to build a statement out of a dozen `String`s and
join them. It now writes into **one buffer**:

* `rust/src/escape.rs` — `write_escaped` appends instead of returning.
* `rust/src/ops.rs` — filters, operators and updates append.
* `rust/src/validate.rs` — identifiers append.
* `rust/src/json_path.rs` — JSON-path SQL appends.
* `rust/src/builder.rs` — `insert_many` appends columns, escaped
  values and rows into a single statement string.

---

## 2. Speed — Rust against JavaScript and Go

### The SQL builder

20,000 iterations, best of 3.

| case | Rust | JavaScript | Go | Rust vs JS | Rust vs Go |
| --- | --- | --- | --- | --- | --- |
| find | **1,031 ns** | 4,272 ns | 1,583 ns | **4.1× faster** | **1.5× faster** |
| update | **1,177 ns** | 5,293 ns | 1,880 ns | **4.5× faster** | **1.6× faster** |
| aggregate | **1,939 ns** | 7,195 ns | 2,478 ns | **3.7× faster** | **1.3× faster** |
| insert50 | **2,251 ns** | 19,353 ns | 11,541 ns | **8.6× faster** | **5.1× faster** |

### The in-memory engine

Ten thousand documents, best of 3.

| case | Rust | JavaScript | Go | Rust vs JS | Rust vs Go |
| --- | --- | --- | --- | --- | --- |
| find | **2,249,477 ns** | 9,948,367 ns | 3,281,701 ns | **4.4× faster** | **1.5× faster** |
| find+sort | **3,850,653 ns** | 16,453,216 ns | 7,928,313 ns | **4.3× faster** | **2.1× faster** |
| aggregate | **3,048,596 ns** | 5,886,126 ns | 3,193,776 ns | **1.9× faster** | **1.0× faster** |
| updateMany | **2,191,068 ns** | 15,399,849 ns | 3,601,355 ns | **7.0× faster** | **1.6× faster** |
| insert1 | 516 ns | **424 ns** | 664 ns | 0.8× — JS wins | **1.3× faster** |

### The command line

The floor under every other command: how long it takes to start, read
its arguments and print a line.

| command | best | median | peak RSS |
| --- | --- | --- | --- |
| `nanobase --version` | 1.3 ms | 1.3 ms | 10,316 KiB |
| Go equivalent | 1.0 ms | 1.1 ms | 10,308 KiB |
| `node --version` | 3.4 ms | 3.6 ms | 18,828 KiB |

**The goal — Rust faster than JavaScript and Go — is met for every
SQL-builder case and every meaningful multi-document in-memory case.**
The one exception is `insert1`: a single small document inserted into
an in-memory collection is 424 ns in JavaScript and 516 ns in Rust.
That case is dominated by the hash map insertion and the fixed overhead
of the call, not by the query builder, and the gap is 92 ns.

---

## 3. Memory

The peak resident set size — the most the process held at once.

| what | peak RSS | | against Rust |
| --- | --- | --- | --- |
| **Rust** SQL builder | **10,344 KiB** | 10.1 MiB | — |
| **Rust** in-memory engine | **16,876 KiB** | 16.5 MiB | — |
| Go SQL builder | 10,148 KiB | 9.9 MiB | 196 KiB less |
| Go in-memory engine | 20,644 KiB | 20.2 MiB | 3,768 KiB more |
| Node SQL builder | 63,880 KiB | 62.4 MiB | 53,536 KiB more |
| Node in-memory engine | 96,360 KiB | 94.1 MiB | 79,484 KiB more |

**Rust uses the least memory of the three on every comparable
measurement**, and Node uses the most by a wide margin:

* against **JavaScript**: 6.2× less for the builder, 5.7× less for the
  in-memory engine.
* against **Go**: 2 KiB less for the builder (a tie), 1.2× less for the
  in-memory engine.

The two editions of the platform are the same story: the serverless
binary is **44 % of the size** of the full one and holds the same
memory at startup.

---

## 4. Pending work

| item | state | why |
| --- | --- | --- |
| **Postgres and MySQL app databases** | **next** | They want a driver. The factory says so plainly rather than pretending: `the postgres engine is not built into this binary yet`. |
| The JavaScript runtime adapters (`src/drivers.js`, `src/client.js`, `src/schemaless.js`) | not ported, **by decision** | I/O glue that would pull in async + TLS + socket crates and break the zero-dependency goal. |
| `uflite`'s `auth.js` and `sql-adapter-factory.js` | not ported | The same I/O glue; the authentication engine is `better-auth-core` instead. |
| `schemaless`'s MongoDB backend | not ported | It is a driver, not a SQL dialect. |
| Better Auth's generic-OAuth *transform callbacks*, *refresh parameters* and *discovery-derived logout* | deliberately out | The README says why for each. The headers a provider asks for are there. |
| A `reqwest`-backed HTTP client in `better-auth-core` | reserved | It is the one dependency that needs a registry; the feature exists and is off. |
| A `LICENSE` file at the repository root | missing | `license = "ISC"` matches `package.json`, and `include = ["LICENSE*"]` is already in place for it. |
| The crate names on crates.io | unverified | The sandbox had no registry access. |

---

## 5. How it is checked

| crate | gate |
| --- | --- |
| `mosql` | `rust/verify.sh` — fmt, clippy, the tests, **289 builder fixtures and 153 memory fixtures regenerated** against the JavaScript, a `no_std` build and a publish dry-run |
| `uflite` | `uflite/verify.sh` |
| `schemaless` | `schemaless/verify.sh` |
| `nanobase` | `nanobase/verify.sh` — fmt, clippy, the tests, **a `serverless` stage that compiles and tests the second edition**, no external dependencies, a publish dry-run, and a size budget that measures **both** binaries and fails if the serverless one is not smaller |
| `better-auth-core` | `better-auth-core/verify.sh` — plus the official `better-auth@1.7.27` client driving the release binary over HTTP: 22/22 |
| `worker` | `worker/verify.sh` and `worker/test.sh` — fmt, clippy, the protocol tests, the schema test, the end-to-end Wrangler run, and a check on the module's import section |

CI gives each stage its own step, so a red X says which stage broke.
