# The authentication engine, as a Cloudflare Worker

The same engine the native binary runs — every route, the same SQL, the
same records — compiled to WebAssembly and deployed as a Worker. D1 is
the database. KV is an optional cache for the sessions. R2 is an optional
bucket for blobs.

Nothing here is a rewrite. [`better-auth-core`](../better-auth-core) is
the engine; this crate is the part that makes it run somewhere it has no
operating system.

```
worker/
  src/            the Rust half, compiled to WebAssembly
  js/index.js     the Worker script: the half that holds the bindings
  schema.sql      the database schema
  wrangler.toml   the bindings
  build.sh        build the module
  test.sh         the whole thing, end to end, locally
  verify.sh       the gate CI runs
```

## Running it

```sh
./build.sh                                   # the module
wrangler d1 execute mosql-auth --local --file=schema.sql
wrangler dev --var AUTH_SECRET:... --var AUTH_BASE_URL:http://127.0.0.1:8787
```

Then sign up:

```sh
curl -X POST http://127.0.0.1:8787/api/auth/sign-up/email \
  -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correct-horse-battery","name":"Ada"}'
```

`./test.sh` does all of that and checks the answers: a sign-up, a sign-in,
the session that comes back, a blob written and read, and a sign-out that
makes the session null.

For a deployment:

```sh
wrangler d1 create mosql-auth          # then put the id in wrangler.toml
wrangler kv namespace create mosql-auth-cache
wrangler r2 bucket create mosql-auth
wrangler d1 execute mosql-auth --remote --file=schema.sql
wrangler secret put AUTH_SECRET
wrangler secret put ALLOWED_ORIGINS     # comma-separated
wrangler deploy
```

## How it works

A Worker is single-threaded, and the database is a promise. Rust cannot
await a JavaScript promise without a binding layer, and the binding layer
is not something this repository can fetch — it has no registry access,
and the engine is built from in-repo crates on purpose.

So the engine does not await. It **asks**.

The engine's request handling is an ordinary Rust future. When it needs
the database it stops, and hands the host a question:

```json
{"kind": "query", "sql": "SELECT * FROM \"user\" WHERE \"email\" = ? LIMIT 1",
 "params": ["ada@example.com"], "rows": true}
```

The Worker script runs the statement against D1, hands back the rows, and
the engine continues where it left off. The same conversation carries
outbound HTTP, the cache and the bucket.

That is the whole trick, and it is why the engine did not have to change:
the store is still `AuthStore`, the routes are still `async fn`, and the
SQL is still built by the same code the native store uses.

The module is imported at source phase:

```js
import wasmModule from "./mosql_worker.wasm";
```

The platform compiles it when the Worker is built and hands the script a
`WebAssembly.Module`. Compiling bytes at run time is forbidden by the
runtime, so this is the only way in.

### What the host provides

Two imports, and nothing else:

| import | what it is |
| --- | --- |
| `host_now_ms() -> f64` | milliseconds since the Unix epoch |
| `host_random(ptr: *mut u8, len: usize)` | fill the buffer with random bytes |

They are the clock and the randomness, which a Worker has no other way to
get. `worker/verify.sh` reads the module's import section and fails if it
asks for anything else — a Worker has no socket, no file and no clock of
its own, and a module that reached for one would fail at run time rather
than at build time.

## What is in the store

`D1Store` is the eight operations of `AuthStore`, and it is the same
store the native binary uses with the same SQL. The difference is one
level down: a statement is run by the host rather than by a connection.

The rows come back in the same shape the native store reads them, and the
extra `data` column is expanded the same way, so a record is a record.

## The cache

With a KV namespace bound, sessions are cached there, keyed by session
id — which is what every authenticated request looks a session up by.

The invalidation has to be exact, because a stale session is a revoked
session that still works. A write that names a session invalidates that
one; a write that does not — a bulk revocation by user, say — invalidates
every cached session, because it may have touched any of them. The TTL is
a minute, which is the shortest lifetime the cache accepts.

Without KV the cache is not used at all, and every lookup goes to the
database.

## The bucket

With an R2 bucket bound, `/api/blob/<key>` stores and serves bodies:

```sh
curl -X PUT http://127.0.0.1:8787/api/blob/notes/hello.txt \
  -H 'content-type: text/plain' -d 'hello from the worker'
curl http://127.0.0.1:8787/api/blob/notes/hello.txt
curl -X DELETE http://127.0.0.1:8787/api/blob/notes/hello.txt
```

Without a bucket the routes answer 404, and nothing else changes.

## The feature switch

The engine is the same crate in both deployments, and a feature decides
which half is compiled:

| | native | Worker |
| --- | --- | --- |
| database | local SQLite | D1 |
| HTTP out | `std::net` and `curl` | `fetch` |
| clock | the operating system | `host_now_ms` |
| randomness | `/dev/urandom` | `host_random` |
| storage | a JSON file | nothing |

`better-auth-core`'s `native` feature carries the first four rows, and
the Worker build turns it off. That is what keeps the WebAssembly module
free of SQLite, of sockets, of subprocesses, of threads and of a clock of
its own: those parts are not compiled in, rather than compiled in and
avoided.

## Tests

* `worker/tests/protocol.rs` plays the part of the host and answers the
  engine's questions from a real SQLite database, so the statements the
  engine writes are checked against a real engine, and the rows it reads
  back are the rows it would read from D1.
* `worker/tests/schema.rs` checks `schema.sql` against the schema the
  engine expects, so the file and the engine cannot drift apart.
* `worker/test.sh` runs the whole deployment locally, through Wrangler,
  and checks the answers.
