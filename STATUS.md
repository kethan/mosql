# Status: the Rust port, the two editions, and what they cost

The numbers below were measured on the machine this was built on, on
commit `e6c4a97`. Every figure can be reproduced:

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
| `nanobase/` | the platform: app documents, secrets, an owner account, a platform store, a service layer with events and hooks, an HTTP API, a web console, security middleware, and the command line | 290 (full) · 112 (serverless) |
| `better-auth-core/` | a Better Auth-compatible authentication server: bearer JWT sessions, eight plugins, every social provider, OpenID Connect with verified ID tokens | 172, plus the official client **22/22** |
| `worker/` | the same engine as a Cloudflare Worker: Rust compiled to WebAssembly, D1 as the database, KV and R2 as optional adapters | 12, plus the end-to-end Wrangler run |
| `mosql-core/` | the serverless drivers: Turso, Cloudflare D1, Neon, TiDB Cloud and PlanetScale, from one connection URL. No crates.io dependency, no global state, and it builds for WebAssembly | 83 |

The JavaScript implementation is untouched and remains the oracle for
parity.

### The two editions of the platform

The CLI and the binaries were the priority, and the split falls out of
them: **one command line, two binaries**, and the difference is what
each one is allowed to reach for.

| | **full** — `nanobase` | **serverless** — `nanobase-serverless` |
| --- | --- | --- |
| Release size | 1,983,408 bytes (1.89 MiB) | 836,896 bytes (817 KiB) — **42 % of the full one** |
| Peak RSS at `--version` | 10,148 KiB | 10,184 KiB |
| Platform state | one local SQLite file (`--data`) | a serverless database (`--db-url`) |
| Listener | `nanobase serve`, `nanobase db serve` | **none** — no TCP code in the binary |
| Local database | SQLite, vendored and compiled in | **none** |
| Build | `cargo build --release` | `cargo build --release --no-default-features --features serverless` |
| Tests | 290 | 112 (a second, separate suite) |

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

### The console and the platform's security

The platform now has a web interface of its own, and the three
protections a browser needs. Both are in the binary: three files
compiled in with `include_str!`, so there is nothing to install and
nothing to deploy beside the executable.

**The console** is one page of plain JavaScript — no framework, no build
step — talking to the same HTTP API the command line talks to. It
creates apps, searches them, pages through them, and opens each app's
database, auth, CORS, access rules and rows. The database tab has a
**Test connection** button that tries the configuration *before* it is
saved, so a wrong URL is found while the operator is still looking at
it.

| file | what it is |
| --- | --- |
| `nanobase/ui/index.html` | the shell: header, sidebar, panel, dialog, toast |
| `nanobase/ui/console.css` | the design system — light and dark, responsive down to a phone |
| `nanobase/ui/console.js` | the client: sign-in, routing, apps, search, paging, database, auth, CORS, access rules, rows |
| `nanobase/src/console.rs` | compiles the three files in and serves them at `/`, `/console.css` and `/console.js` |

**Security headers** are on every answer, the console's HTML included:
`Content-Security-Policy`, `X-Content-Type-Options`, `X-Frame-Options`,
`Referrer-Policy`, `Cross-Origin-Resource-Policy`,
`Cross-Origin-Opener-Policy`, `X-Permitted-Cross-Domain-Policies` and a
`Permissions-Policy` that turns the device APIs off. The content policy
differs by what is answered: an API answer may load nothing at all, and
the console may load only its own script and stylesheet.

**CORS** is per origin at two levels. The platform's list comes from
`--cors` or `NANOBASE_CORS_ORIGINS`; an app may have its own in
`config.cors`, which is what the console's CORS tab writes. An app with
CORS of its own uses it; an app without one inherits the platform's. An
origin that is not on the list gets **no** `Access-Control-Allow-Origin`
header, and the browser refuses to hand it the answer.

**The rate limit** is off by default. `--rate-limit N` (or
`NANOBASE_RATE_LIMIT`) allows that many requests a minute per client
address over a sliding window, and answers `429` with a `Retry-After`.

The comparison that started this — the Node `helmet` package — was
useful for the *list* of headers and for the shape of the answer, and
then unnecessary for the rest: the Rust platform has no `express`, no
`helmet`, no `cors` and no `express-rate-limit`, so there is nothing to
configure and nothing to keep in step with a dependency. The protections
are the platform's own code, in `nanobase/src/middleware.rs`, with the
headers, the CORS rules and the sliding window each in one place and a
test beside it.

**A second leak found and closed.** An app's document holds its database
password, its token and its storage keys, and the API handed the whole
document to anybody who could read the app list: a program holding
`apps:read` could read every app's credentials. The export was
redacted; the API was not.

The document the API hands out now has its secrets taken out, the same
way the shareable export does — the URL is still there, because it says
which database it is, but not the password in it. Beside the document
the answer says which fields hold a secret (`config.database.token`, and
so on), so an editor can say "there is a token here" without being told
the token. The **owner** — the platform's own root credential — may ask
for them with `?includeSecrets=true`; a program with a key may not, and
is told so. The console sends only the fields a person actually
changed, so a save that did not mean to touch a secret does not wipe it,
and the connection test uses the platform's own password rather than the
client's.

**One leak found and closed.** `User::to_value` said "never the password
hash" in its own comment and then put the hash — and the salt — into
every answer that carried a user. `POST /api/auth/login` and
`GET /api/auth/me` were handing the owner's PBKDF2 hash to anybody who
asked. The stored shape and the public shape are now two different
functions, and a test asserts that no answer contains `"hash"` or
`"salt"`.

**Windows, with a todo app.** `nanobase/examples/todo/` is the
walkthrough: build, `init`, `serve`, create the app, put rows in it with
`curl`, and open a plain HTML page that talks to those rows. It ships a
`serve.ps1` — a static file server small enough to read before running —
so the page is served over HTTP rather than from the disk, because a
`file://` page cannot read an `http://` answer.

### An app's own rules

An app may say who may do what with its rows. The rules are JSON in the
app's document (`config.acl`), the evaluator is `uflite`'s, and the
answer is the same in every engine — the rules are about the rows, not
about the database they are in.

| rule | what it does |
| --- | --- |
| `role:resource:method` | `editor:tasks:update` — the role may update the `tasks` resource. A `*` is a wildcard |
| `{{user}}` and `{{role}}` | the caller's own id and role, substituted before the rule is judged |
| a **deny** rule | beats every allow: the rules are OR-ed for allows, and a deny is final |

A check is made against **the row**, not the request: a rule that says
"you may change a task that is not done" is about the row's state now.
A list is filtered row by row **before** the page is taken, so the page
a caller sees is a page of rows they may see. The platform owner is
above the rules — the owner wrote them — and so is a credential with the
`admin` scope. An app with no rules, or with them switched off, is open.

| file | what it is |
| --- | --- |
| `nanobase/src/acl.rs` | the policy, the caller, the substitutions, the checks, the list filter |
| `nanobase/src/service.rs` | the enforced methods: every row operation is made through them |
| `uflite/src/acl.rs` | the evaluator both editions share |
| `nanobase/tests/acl.rs` | 6 tests: hidden rows, unmatched rules, the owner, refused writes, ownership, an open app |

### API keys

A **session token** is a person. An **API key** is a program. Until now
the platform had only the first, so a front end on another origin, a
script, a cron job — anything that is not the owner — had to borrow the
owner's token, which is the platform's most powerful credential and the
one with the longest life.

**A key belongs to exactly one app.** It is made for that app, it may
only see that app, it lists that app only, and it lives in that app's
own database — in the internal collection `_api_keys`, which is not one
of the app's collections and which the row routes refuse. The platform
keeps a **directory**: the app, the key's preview and the id of the row
that holds the key. The hash, the scopes and the expiry are in the app's
database and nowhere else, so a platform file that is read cannot be
used to work out anybody's key.

A key is made in the console (**API keys** in the sidebar) or from the
command line (`nanobase key create APP --name …`), and it can be limited
in four ways:

| limit | what it does |
| --- | --- |
| a **name** | so the key can be found in a list six months later |
| **scopes** | `apps:read`, `apps:write`, `rows:read`, `rows:write`, `admin` — a key that may only read rows may not read the app list, and says so with `403` |
| its **app** | a key belongs to one app: it sees that app only, and lists that app only |
| an **expiry** | a date after which the key is refused with `401 EXPIRED_KEY` |

The secret is shown **once**, at the moment it is made, and only its
SHA-256 hash is stored: a leaked database of platform state does not
leak a usable key. A key can be revoked, and a revoked key is refused
with `401 REVOKED_KEY`. The owner's token still exists and still works
for the privileged operations — making and revoking keys, and everything
else — but it is no longer the credential a program is expected to hold.

| route | what it does |
| --- | --- |
| `GET /api/apps/{app}/keys` | the app's keys, without their hashes |
| `POST /api/apps/{app}/keys` | make a key; the secret is in the answer and nowhere else |
| `GET, PATCH, DELETE /api/apps/{app}/keys/{id}` | one key: read it, change it, remove it |

**A platform from before this change is not stranded.** Its keys are
read into a legacy list and moved into the apps they named — the hash
comes with them, so a secret that was shown once still works. A key that
named no app was made for the whole platform, which a key no longer is:
it waits, the platform says so when it starts, and
`nanobase key bind APP ID` gives it an app.

| file | what it is |
| --- | --- |
| `nanobase/src/apikey.rs` | the key: its hash, its scopes, its app, its expiry, its revocation |
| `nanobase/src/keys.rs` | the key's row in the app's own database, and the collection it lives in |
| `nanobase/src/platform.rs` | the directory: which app a presented key belongs to, and which row holds it |
| `nanobase/src/service.rs` | the key lifecycle, and the migration of an older platform's keys |
| `nanobase/src/api.rs` | the principal: the owner token or an API key, and what each may do |
| `nanobase/src/store.rs` | the native SQLite store: the directory in `meta`, the old keys in `api_keys` until they are moved |
| `nanobase/src/store_serverless.rs` | the serverless store, with the same shape |
| `nanobase/tests/apikeys.rs` | 20 tests: the lifecycle, the scopes, the app binding, the expiry, the revocation, the restart, the migration |

### The serverless drivers

`mosql-core/` is a crate that talks to the five serverless databases
that matter, **from one connection URL**, so a caller does not have to
know which one it is talking to.

| provider | dialect | endpoint |
| --- | --- | --- |
| Turso / libSQL | SQLite | `https://{host}/v2/pipeline` |
| Cloudflare D1 | SQLite | `https://api.cloudflare.com/client/v4/accounts/{acct}/d1/database/{db}/query` |
| Neon / Vercel Postgres | PostgreSQL | `https://{host}/sql` |
| TiDB Cloud Serverless | MySQL | `https://http-{host}/v1beta/sql` |
| PlanetScale | MySQL (Vitess) | `https://{host}/psdb.v1alpha1.Database/Execute` |

```rust
let driver = AnyServerlessDriver::from_url(
    "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb",
    transport,
)?;
let rows = driver.execute("SELECT id FROM users WHERE age > $1", &[SqlValue::Int(18)]).await?;
```

**No crates.io dependencies.** The JSON model, the parser and the writer
come from the repository's own `mosql` crate, so there is no `sqlx`, no
Diesel, no OpenSSL, no Tokio, no Serde, no `url` crate, no base64 crate
and no `chrono` — and the crate builds for a WebAssembly module as
happily as for a server.

**No global state.** No `static`, no `OnceLock`, no environment variable
read inside the driver: a driver owns its configuration and its
transport, and the caller owns the driver. That is what makes it
possible to check a wire protocol with a mock transport and nothing else.

The transport is a trait, and the concrete ones are behind features: a
`curl`-backed transport for a native build — the platform's own answer
to "HTTPS without a registry" — and a host-provided `fetch` for a Worker,
which is what `worker::Fetch::Request` is.

The parameters are dealt with per dialect. PostgreSQL renumbers them to
`$1`, `$2`, …; MySQL inlines them, escaped, so a value cannot change
the statement, and refuses a float that is not finite; SQLite sends them
as arguments, with a whole number as a string, because a JSON number
cannot hold every `i64` exactly and a database row can. A `?` inside a
string, a quoted identifier or a comment is not a parameter, and the
count has to match or the statement is refused before it is sent.

TiDB and PlanetScale keep a session, and it travels with the request, so
`BEGIN` and `COMMIT` reach the same conversation; a statement that fails
gets a `ROLLBACK` before the error is returned.

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
| find | **1,048 ns** | 4,272 ns | 1,685 ns | **4.1× faster** | **1.6× faster** |
| update | **1,168 ns** | 5,417 ns | 2,056 ns | **4.6× faster** | **1.8× faster** |
| aggregate | **1,979 ns** | 7,825 ns | 2,354 ns | **4.0× faster** | **1.2× faster** |
| insert50 | **2,458 ns** | 19,563 ns | 11,518 ns | **8.0× faster** | **4.7× faster** |

### The in-memory engine

Ten thousand documents, best of 3.

| case | Rust | JavaScript | Go | Rust vs JS | Rust vs Go |
| --- | --- | --- | --- | --- | --- |
| find | **2,352,891 ns** | 9,554,465 ns | 3,169,767 ns | **4.1× faster** | **1.3× faster** |
| find+sort | **3,622,289 ns** | 16,169,308 ns | 7,728,377 ns | **4.5× faster** | **2.1× faster** |
| aggregate | **2,852,604 ns** | 5,754,169 ns | 3,165,584 ns | **2.0× faster** | **1.1× faster** |
| updateMany | **1,967,707 ns** | 13,942,502 ns | 3,593,877 ns | **7.1× faster** | **1.8× faster** |
| insert1 | 548 ns | **257 ns** | 623 ns | 0.5× — JS wins | **1.1× faster** |

### The command line

The floor under every other command: how long it takes to start, read
its arguments and print a line.

| command | best | median | peak RSS |
| --- | --- | --- | --- |
| `nanobase --version` | 1.3 ms | 1.4 ms | 10,260 KiB |
| Go equivalent | 1.0 ms | 1.1 ms | 10,292 KiB |
| `node --version` | 3.3 ms | 3.4 ms | 18,816 KiB |

**The goal — Rust faster than JavaScript and Go — is met for every
SQL-builder case (1.2× to 4.7× faster than Go, 4.0× to 8.0× faster
than JavaScript) and every meaningful multi-document in-memory case
(1.1× to 2.1× faster than Go, 2.0× to 7.1× faster than JavaScript).**
The one exception is `insert1`: a single small document inserted into
an in-memory collection is 257 ns in JavaScript and 548 ns in Rust.
That case is dominated by the hash map insertion and the fixed overhead
of the call, not by the query builder, and the gap is 291 ns.

---

## 3. Memory

The peak resident set size — the most the process held at once.

| what | peak RSS | | against Rust |
| --- | --- | --- | --- |
| **Rust** SQL builder | **10,384 KiB** | 10.1 MiB | — |
| **Rust** in-memory engine | **16,968 KiB** | 16.6 MiB | — |
| Go SQL builder | 10,436 KiB | 10.2 MiB | 52 KiB more |
| Go in-memory engine | 20,656 KiB | 20.2 MiB | 3,688 KiB more |
| Node SQL builder | 62,600 KiB | 61.1 MiB | 52,216 KiB more |
| Node in-memory engine | 98,356 KiB | 96.1 MiB | 81,388 KiB more |

**Rust uses the least memory of the three on every comparable
measurement**, and Node uses the most by a wide margin:

* against **JavaScript**: 6.0× less for the builder, 5.8× less for the
  in-memory engine.
* against **Go**: 52 KiB less for the builder (a tie), 1.2× less for the
  in-memory engine.

The two editions of the platform are the same story: the serverless
binary is **42 % of the size** of the full one and holds the same
memory at startup. The console and the security middleware are in the
full edition only, and they are what the full edition grew by.

---

## 4. Pending work

| item | state | why |
| --- | --- | --- |
| **An app's own sign-in endpoint** | **next** | The app document has the shape for it (`config.auth.features.emailPassword`) and the console writes it, but the platform still serves only the owner's token. A front end has to borrow the owner's token today. |
| **Postgres and MySQL app databases** | **next** | The drivers are written — `mosql-core/` speaks to Neon, TiDB and PlanetScale — but the platform's own database factory still only offers the generic HTTP adapter and says so plainly rather than pretending: `the postgres engine is not built into this binary yet`. Wiring the new crate into the factory is the remaining half. |
| The JavaScript runtime adapters (`src/drivers.js`, `src/client.js`, `src/schemaless.js`) | not ported, **by decision** | I/O glue that would pull in async + TLS + socket crates and break the zero-dependency goal. |
| `uflite`'s `auth.js` and `sql-adapter-factory.js` | not ported | The same I/O glue; the authentication engine is `better-auth-core` instead. |
| `schemaless`'s MongoDB backend | not ported | It is a driver, not a SQL dialect. |
| Better Auth's generic-OAuth *transform callbacks*, *refresh parameters* and *discovery-derived logout* | deliberately out | The README says why for each. The headers a provider asks for are there. |
| A `reqwest`-backed HTTP client in `better-auth-core` | reserved | It is the one dependency that needs a registry; the feature exists and is off. |
| A `ureq`-backed transport in `mosql-core` | reserved | The same reason. The `transport-curl` transport covers the same ground without a registry, and `transport-worker` covers a Worker. |
| A `LICENSE` file at the repository root | missing | `license = "ISC"` matches `package.json`, and `include = ["LICENSE*"]` is already in place for it. |
| The crate names on crates.io | unverified | The sandbox had no registry access. |

---

## 5. How it is checked

| crate | gate |
| --- | --- |
| `mosql` | `rust/verify.sh` — fmt, clippy, the tests, **289 builder fixtures and 153 memory fixtures regenerated** against the JavaScript, a `no_std` build and a publish dry-run |
| `uflite` | `uflite/verify.sh` |
| `schemaless` | `schemaless/verify.sh` |
| `nanobase` | `nanobase/verify.sh` — fmt, clippy, the tests, **a `serverless` stage that compiles and tests the second edition**, no external dependencies, a publish dry-run, and a size budget that measures **both** binaries and fails if the serverless one is not smaller. **347 tests**: `nanobase/tests/console.rs` covers the console, the security headers, the CORS rules, the rate limit, the search and paging, and the database connection test; `nanobase/tests/acl.rs` covers an app's own rules; `nanobase/tests/apikeys.rs` covers the keys; `nanobase/tests/redaction.rs` covers the app's secrets |
| `better-auth-core` | `better-auth-core/verify.sh` — plus the official `better-auth@1.7.27` client driving the release binary over HTTP: 22/22 |
| `worker` | `worker/verify.sh` and `worker/test.sh` — fmt, clippy, the protocol tests, the schema test, the end-to-end Wrangler run, and a check on the module's import section |
| `mosql-core` | `mosql-core/verify.sh` — fmt, clippy, the tests, a dependency check that refuses a registry dependency, a **wasm32 build with the standard library switched off**, and a stage that compiles every optional transport |

CI gives each stage its own step, so a red X says which stage broke.
