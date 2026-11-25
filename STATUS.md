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
| Release size | 2,199,184 bytes (2.10 MiB) | 996,112 bytes (973 KiB) — **45 % of the full one** |
| Peak RSS at `--version` | 10,148 KiB | 10,184 KiB |
| Platform state | one local SQLite file (`--data`) | a serverless database (`--db-url`) |
| Listener | `nanobase serve`, `nanobase db serve` | **none** — no TCP code in the binary |
| Local database | SQLite, vendored and compiled in | **none** |
| Build | `cargo build --release` | `cargo build --release --no-default-features --features serverless` |
| Tests | 364 | 150 (a second, separate suite) |

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

### The database matrix, and a clone that says what it does

An app's rows used to live in a local SQLite file, with a remote
escape hatch behind an HTTP service. They now live in whatever engine
the app's document names, behind one `AppDb` and one contract: a
collection of documents, the filters are `mosql`'s matcher, and the
operations are the same in every engine.

| engine | how it is reached | what it is |
| --- | --- | --- |
| `sqlite` | a local file | the default; vendored and compiled in |
| `memory` | in the process | gone when the process stops |
| `libsql` / `turso` | the libSQL pipeline | Turso, in its own protocol |
| `d1` | the D1 query API | Cloudflare D1 |
| `postgres` | the `/sql` API | Neon, Vercel Postgres |
| `mysql` | TiDB's proxy, Vitess' `Execute` | TiDB Cloud, PlanetScale |
| `http://` | a service speaking this platform's protocol | anything with a service in front |
| `tcp://` | a socket | the same protocol, over a socket |

The provider adapter is the odd one out: it speaks the *database's*
protocol rather than this platform's, which is what lets an app's rows
live in Neon or Turso with no database service in front of them. It
needs no new dependency — `mosql-core/` already speaks all five
protocols over HTTP — and the rows are stored the way the local file
stores them: one table per collection, an `id` and a `data` column
holding the row as JSON.

**The platform's own state is the owner's choice, and is chosen at run
time.** `--db-url postgres://…` puts the catalog in a serverless
PostgreSQL; with no such URL the local file is used.

**Cloning an app says what happens to the rows.** `omit` — the
default — starts the clone without copied rows. `copy` asks for a
destination database and copies the rows into it, and copying into the
source's own URL is refused. `share` keeps the source's database, and
so its token with it. A clone of an app in a serverless database gets
**no database at all** until somebody names one: `DbKind::None` is
that state — a schema, rules and auth, and nowhere to put the rows
yet — rather than a clone that quietly shares production rows.

**The clone's document is the source's document.** The same schema,
the same rules, the same auth settings — and the same signing secret,
because the secret is in the document and the document is what is
copied. The *database* is the exception: it is chosen by the clone's
own policy, and a clone that does not share the source's database does
not inherit its database token either. Whether a clone should have a
secret **of its own** is a question about what a clone is for, and it
is open: a clone that shares the source's database is the same app
under a second name, and a clone that does not is a new app — and the
two may want different answers.

**A clone that cannot be finished is taken back out.** The rows are
copied one at a time, and a copy that fails half way is undone: the
rows it wrote are removed and the clone is removed from the platform,
so the person who asked for it gets an error and no app rather than an
app that is quietly incomplete.

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

**Two versions, kept apart on purpose.** The *directory* has a version,
and it goes up when the app's **set** of keys changes: one is made, or
one is removed. A *key* has a version of its own, and it goes up when
the key **changes** — renamed, its scopes cut, revoked — and it does not
go up when the key is used, because the time a key was last used is not
a permission. The key's version lives in the key's own row, so the
change and the version that says so are one write and cannot disagree:
the key is in the app's database and the directory is somewhere else
entirely, and a promise that spans two stores is a promise that can be
broken by one of them failing.

**A preview may be shared.** The preview is six characters of a
256-bit key, so two keys sharing one is a one-in-sixty-eight-billion
accident. The platform keeps every candidate for a preview rather than
the first one it finds, and the secret itself decides between them —
a key is not refused because another key happened to look like it.

#### Where the key lives, and what that costs

The key's record — the hash, the scopes, the expiry, the revoked flag —
is in the app's own database, in the internal collection `_api_keys`.
The platform keeps a directory: the app, the preview, and the id of the
row. Nothing in the platform can be used as a key, and nothing in the
platform says what a key may do.

**What it buys.** A platform file that is read — a backup, a copy in a
bucket, a log — cannot be used to work out anybody's key: the hash is
in the app's database, beside that app's rows. The keys travel with the
app: an app cloned with its database shares its keys deliberately, and
an app cloned without its database starts with none. And the platform's
own store stays small — a directory, not a list of secrets.

**What it costs.**

- **Verifying a key opens the app's database.** A key is checked by
  reading the app's own row, so an app whose database is unreachable
  cannot authenticate its keys: the answer is a `500`, not a `401`. A
  platform-wide list of hashes would have kept working. That is the
  price of the platform not holding them.
- **Two stores, so no transaction across them.** Making a key writes the
  row first and the directory second, and takes the row back out again
  if the second write fails. The order is chosen so that the failure
  that cannot be undone is the harmless one: a key that exists but is
  not in the directory cannot be *used*, and a directory pointing at a
  row that is not there is a `404` at worst.
- **The key's own version is in the key's own row**, so a change and
  the version that says so are one write. A version that spanned the
  two stores would be a promise one of them could break by failing.
- **Removing an app removes its keys.** The rows are in the app's
  database and go with it, and the directory forgets them. A key that
  outlived its app would be a key with no app, which is the thing a key
  no longer is.

**Migration.** A platform from before this change is read into a legacy
list and moved into the apps the keys named. The hash comes with them,
so a secret that was shown once still works. The move is idempotent — a
row that is already there is not written twice — and it *reads* the
legacy keys rather than taking them, because an app's database that
cannot be opened stops the move with an error, and keys that had been
taken out of the platform would have been written out of it by the next
save. A key that named no app was made for the whole platform, which a
key no longer is: it waits, the platform says so when it starts, and
`nanobase key bind APP ID` gives it an app. A key whose app is gone
waits too, and is reported as orphaned.

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
| **A clone's own signing secret** | **open** | The clone's document is the source's document, so the clone keeps the source's signing secret, its provider credentials and its storage keys — and a session token for the source works on the clone. The database token *is* treated differently (a clone that does not share the database does not inherit it). Whether a clone that does not share the source's database should get a secret of its own is a question about what a clone is for; it is not answered here, and nothing about it is silent: `STATUS.md` says what happens today. |
| **An app's own sign-in endpoint** | **next** | The app document has the shape for it (`config.auth.features.emailPassword`) and the console writes it, but the platform still serves only the owner's token. A front end has to borrow the owner's token today. |
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
| `nanobase` | `nanobase/verify.sh` — fmt, clippy, the tests, **a `serverless` stage that compiles and tests the second edition**, no external dependencies, a publish dry-run, and a size budget that measures **both** binaries and fails if the serverless one is not smaller. **365 tests**: `nanobase/tests/console.rs` covers the console, the security headers, the CORS rules, the rate limit, the search and paging, and the database connection test; `nanobase/tests/acl.rs` covers an app's own rules; `nanobase/tests/apikeys.rs` covers the keys and the migration; `nanobase/tests/redaction.rs` covers the app's secrets; `nanobase/tests/keydirectory.rs` covers the key directory, its versions and the preview collisions; `nanobase/tests/engines.rs` covers the same app in every engine, and a clone whose copy fails |
| `better-auth-core` | `better-auth-core/verify.sh` — plus the official `better-auth@1.7.27` client driving the release binary over HTTP: 22/22 |
| `worker` | `worker/verify.sh` and `worker/test.sh` — fmt, clippy, the protocol tests, the schema test, the end-to-end Wrangler run, and a check on the module's import section |
| `mosql-core` | `mosql-core/verify.sh` — fmt, clippy, the tests, a dependency check that refuses a registry dependency, a **wasm32 build with the standard library switched off**, and a stage that compiles every optional transport |

CI gives each stage its own step, so a red X says which stage broke.
