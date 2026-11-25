# nanobase

A PocketBase-style backend platform that runs on your own database.

One binary. It keeps a platform database of **apps**, and each app brings its
own database, auth providers, storage bucket and permission rules. The runtime
API is per-app: `/api/v1/apps/{appId}`.

**No crates.io dependencies.** JSON, the query builder, the in-memory engine
and permission evaluation come from [`mosql`](../rust) and [`uflite`](../uflite).
SQLite is compiled into the binary from the official amalgamation in
[`vendor/sqlite`](vendor/sqlite) (public domain, the same copy `rusqlite`
ships), with every optional feature omitted at the preprocessor level — that is
the lever a prebuilt library does not give. Building needs a C compiler; that
is the only tool beyond Rust itself.

Release binary today: **~1.9 MB** (SQLite is ~0.9 MB of it, the console
~0.1 MB), against a 3 MB budget that [`verify.sh`](verify.sh) enforces.
The serverless edition is **~0.8 MB**: no SQLite, no listener, no local
file, and no console.

## The console

`nanobase serve` serves the platform's own web interface at `/`:

```
nanobase 0.1.0 serving on http://127.0.0.1:8090
  api:  http://127.0.0.1:8090/api
  ui:   http://127.0.0.1:8090/
```

It is three files compiled into the binary — `ui/index.html`,
`ui/console.css` and `ui/console.js` — so there is nothing to install
and nothing to deploy beside the executable. There is no framework and no
build step: the console is a few hundred lines of plain JavaScript that
talks to the same HTTP API the command line talks to, so anything it can
do a script can do.

| page | what it does |
| --- | --- |
| **Apps** | the apps, searched by name or id, paged, with a card each |
| **New app** | creates an app and opens it |
| **Overview** | what the app is, as the platform sees it |
| **Database** | the engine, the URL and the token — with a **Test connection** button that tries the configuration before it is saved |
| **Auth** | email and password, and the identity providers |
| **CORS** | the origins allowed to read this app's answers, and the preflight cache |
| **Access** | the rules: permissions, an effect, and an optional condition |
| **Rows** | the collections an app has and the rows in them |

There is also an **API keys** page in the sidebar: make a key, limit it
to an app and to what it may do, make it expire, and remove it.

### API keys

A **session token** is a person. An **API key** is a program. The owner
makes a key for the thing that needs one, and the key can be limited:

| limit | what it does |
| --- | --- |
| a **name** | so the key can be found in a list six months later |
| its **app** | a key belongs to one app: it may only touch that app, and may only see that app in a list |
| a **scope** | `rows:read`, `rows:write`, `apps:read`, `apps:write`, `admin` |
| an **expiry** | the key stops working on its own |

```
nanobase key create todos --name "the deploy script" --scopes rows:read,rows:write --expires-in 30d
nanobase key list todos
nanobase key show todos key_djf2qK9rsqIK
nanobase key remove todos key_djf2qK9rsqIK
```

The key is shown once, when it is made. The platform keeps a hash of it,
so there is no second chance to read it — the same bargain as a
password, except that the hash is a fast SHA-256 rather than a slow
PBKDF2, because there is nothing to guess about 256 random bits.

The key itself lives in the app's own database, beside that app's rows,
and the platform keeps only a directory of which keys exist and which
app they belong to. A platform file that is read cannot be used to work
out anybody's key.

A key goes in a request like a token:

```
curl http://127.0.0.1:8090/api/apps/todos/db/todos -H "Authorization: Bearer nbk_…"
curl http://127.0.0.1:8090/api/apps/todos/db/todos -H "X-Nanobase-Key: nbk_…"
```

The console has the same thing on its **API keys** page.

### Security

Three things a browser needs and forgets, and the platform has all
three. None of them changes what a request *means* — a CORS header that
is missing changes nothing for `curl`, and a security header changes
nothing for any client at all.

**Security headers** are on every answer, the console's HTML included:
`Content-Security-Policy`, `X-Content-Type-Options: nosniff`,
`X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`,
`Cross-Origin-Resource-Policy`, `Cross-Origin-Opener-Policy`,
`X-Permitted-Cross-Domain-Policies` and a `Permissions-Policy` that
turns the device APIs off. The content policy differs by what is being
answered: an API answer may load nothing at all, and the console may load
only its own script and stylesheet.

**CORS** is per origin, at two levels. The platform's list comes from
`--cors` or `NANOBASE_CORS_ORIGINS`; an app may have its own in
`config.cors`:

```json
{"enabled": true, "origins": ["http://localhost:3000"],
 "allowCredentials": false, "maxAge": 600}
```

An app with CORS of its own uses it; an app without one inherits the
platform's. An origin that is not on the list gets no
`Access-Control-Allow-Origin` header, and the browser refuses to hand it
the answer — which is the right answer, not a missing one.

**The rate limit** is off by default. `--rate-limit 120` (or
`NANOBASE_RATE_LIMIT`) allows that many requests a minute per client
address, over a sliding window, and answers `429` with a `Retry-After`
when a client goes over.

**A password hash never leaves the platform.** The owner's PBKDF2 hash
and salt are kept in the platform's file and are not part of any answer:
`POST /api/auth/login` and `GET /api/auth/me` return the id, the email
and the date, and nothing else.

```
nanobase serve --host 127.0.0.1 --port 8090 --cors "http://localhost:3000" --rate-limit 120
```

## Two editions

The platform ships as two binaries built from the same crate. They share one
command line and one implementation; they differ in what they are allowed to
reach for.

| | **full** (`nanobase`) | **serverless** (`nanobase-serverless`) |
| --- | --- | --- |
| Release size | ~1.8 MB | ~0.8 MB |
| Platform state | one local SQLite file (`--data`) | a serverless database (`--db-url`) |
| Listener | yes — `nanobase serve` | **no** — there is no TCP code in the binary |
| Local database | yes — SQLite, vendored and compiled in | no |
| Build | `cargo build --release` | `cargo build --release --no-default-features --features serverless` |

The split is enforced by Cargo features, not by convention: `native` brings
the TCP server, the file-backed platform store and the `std::net` transport;
`serverless` brings the HTTP transport and the serverless platform store;
`sqlite` brings the vendored engine. The serverless binary contains no
listener, no SQLite and no local file — the platform's own state lives in a
serverless database, and the app's data lives wherever the app says.

The serverless edition needs to be told where the platform's state is:

```
$ nanobase-serverless init --email ada@example.com --password '…' \
    --db-url http://127.0.0.1:8091
Owner created: ada@example.com
Platform database: http://127.0.0.1:8091

$ nanobase-serverless app list --db-url http://127.0.0.1:8091
```

`--db-url` (or `$NANOBASE_DB_URL`) and `--db-token` (or
`$NANOBASE_DB_TOKEN`) are the serverless edition's `--data` and `--token`.
`nanobase db serve` — the full edition's database-over-HTTP server — is what
a serverless deployment points at.

## Status

| Piece | State |
| --- | --- |
| App documents (read, validate, write) | done — 44 tests |
| Secret handling (`SECRET_*`, redaction) | done |
| Owner account: first run, sign in, tokens | done — 43 tests |
| Creating apps: new, import, list, show, export, rename, clone, remove | done — 26 tests |
| Platform store (SQLite: owner, apps, signing key) | done — 27 tests |
| Service layer: events, hooks, custom methods | done — 7 tests |
| HTTP API and the server | done — 19 tests |
| Command line as an API client | done |
| Two editions: full (TCP, local SQLite) and serverless (no TCP, no file) | done — one command line, two binaries |
| The console: create, search, page, configure and test apps | done — three files inside the binary |
| Security headers, CORS (platform and per app), rate limit | done |
| API keys with a scope, an app and an expiry | done — the command line and the console |
| App database: memory, SQLite, HTTP, TCP | done — 6 tests |
| Serving an app's rows over the API | done — 8 tests |
| SQL engines (Postgres, MySQL) | done — the drivers speak Neon, TiDB Cloud and PlanetScale over HTTP |
| Naming the cloud instead of reading it out of the URL | done — `provider` in the document, `--provider` on the command line, a dropdown in the console |

## What an app document is

The JSON file `nanobase import` reads and `nanobase export` writes:

```json
{
  "meta":   { "nanobaseVersion": "1.0.0", "includesData": true },
  "app":    { "name": "todos", "type": "local" },
  "config": {
    "auth":     { "features": { "emailPassword": true } },
    "database": { "type": "sqlite" },
    "storage":  { "type": "local",
                  "local": { "basePath": "./data/files",
                             "publicUrl": "http://localhost:3000/files" } },
    "email": null,
    "acl":   { "enabled": true, "rules": [] }
  },
  "schema": { "todos": { "columns": {
                "id":    { "type": "integer", "primary": true, "autoIncrement": true },
                "title": { "type": "text", "notNull": true } } } },
  "data":   { "todos": [ { "title": "Launch" } ] }
}
```

Three rules the validator enforces, because each one catches a mistake that
would otherwise surface later as a query error:

* **`data` must fit `schema`.** An unknown field, an integer in a text column,
  or a missing `notNull` value is refused with the exact field path —
  `data.todos[0].title: an integer does not fit a text column`.
* **A column type is what it says.** `1` is not text, `"1"` is not an integer,
  `"true"` is not a boolean. Seed data that only works because an engine
  coerces it is a bug.
* **`data` may name a collection `schema` does not declare.** The app creates
  the table on first write, so that is legal — but it is reported, because a
  typo in `schema` otherwise produces a silent new table.

Column types: `integer`, `real`, `text`, `boolean`, `datetime`, `json` — plus
the aliases a user is likely to write (`string`, `number`, `timestamp`, …).
Databases: `sqlite` (default), `libsql`/`turso`, `postgres`/`neon`/`supabase`,
`mysql`/`planetscale`, `d1`, `memory`.

## Secrets

A config file is committed, shared and exported. A secret is none of those.
So a document may leave one out and have it supplied by the environment:

```
SECRET_JWT_SECRET=…
SECRET_GOOGLE_CLIENT_ID=…      SECRET_GOOGLE_CLIENT_SECRET=…
SECRET_DATA_DB_URL=…           SECRET_DATA_DB_TOKEN=…
SECRET_S3_ENDPOINT=…           SECRET_S3_ACCESS_KEY=…  SECRET_S3_SECRET_KEY=…
```

Only `SECRET_*` is honoured, so an unrelated variable cannot silently configure
a platform. The environment fills in what the file leaves **empty**; a value
written in the file is a deliberate choice for that app and wins.

`nanobase redact` writes the same document with every secret replaced by
`null` and `meta.includesSecrets` set to `false` — the version that is safe to
commit or hand to someone.

## First run

```
$ nanobase init
Email: ada@example.com
Password: ················
Owner created: ada@example.com
Platform database: /tmp/nbtest/nanobase.db
Next: nanobase login ada@example.com

$ nanobase login ada@example.com --password '…'
eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1Y…

$ nanobase me --token eyJhbGci…
user:  ada@example.com (owner)
id:    ubf1e7459446e5cc1c4cbe8b9b8f62b17
expires: 2026-11-25T09:41:19Z
```

V1 has exactly one account — the owner — and it can do everything. A second
`init` is refused with a 409.

**The password is never stored.** What lands in the platform database's
`users` table is a PBKDF2 hash with a random 16-byte salt and 600,000 rounds,
as three columns — `iterations`, `salt`, `hash` (base64url):

The iteration count is stored beside the hash, so it can be raised later
without invalidating passwords already issued.

**A failed login does not say why.** An unknown email and a wrong password
return the same status and the same message, and an unknown email still runs a
hash — otherwise the response time reveals which addresses have accounts.

**Tokens are signed, not encrypted**, so they carry only who the caller is:
id, email, role, issue and expiry. No database URL, no storage key, no rule set
— a token is handed to a browser, and anyone holding it can read every byte.
Per-app configuration is resolved server-side from the app id.

## Platform state

The platform keeps its own state in **one SQLite file** (`--data`, default
`./data/nanobase.db`), with three tables:

| table | holds |
| --- | --- |
| `meta` | the key tokens are signed with |
| `users` | the owner: id, email, PBKDF2 parameters and hash, created time |
| `apps` | one row per app: `id`, `name` (unique, case-insensitive), `created_at`, and the app document as JSON |

The app document is stored whole rather than split into columns, because the
platform never queries inside it — it hands it to whatever serves the app. The
database runs in WAL mode with a 5 s busy timeout, so the CLI and a server can
use it at the same time. A save is one transaction that rewrites all three
tables; a crash midway leaves the previous state intact.

Installs from the first release kept their state in a JSON file. If a
`nanobase.json` sits beside the database and the database has no owner yet, it
is imported once on the next command and the file is renamed to
`nanobase.json.migrated`. It is never imported over a database that is in use.

## Apps

An app is one document plus an identity: an id, a name, and a directory of its
own for whatever data it holds.

```
$ nanobase app new todos
Created todos
  id:        app_e553fa2d3fb4fabf580ff7be
  data:      /tmp/nb2/apps/app_e553fa2d3fb4fabf580ff7be
  database:  sqlite (default — edit the document to change it)

$ nanobase app import notes.json
Imported notes
  id: app_e4ad43bf1f84c6c5a2bfa212

$ nanobase app list
NAME                           ID                       TYPE       DATABASE
notes                          app_e4ad43bf1f84c6c5a2bfa212 local      sqlite
todo-list                      app_e553fa2d3fb4fabf580ff7be local      sqlite
```

**Each app gets its own directory** under the platform's root — that is what
"each app gets its own database" means for the engines that are files rather
than servers. For SQLite with no explicit path the app's database is
`apps/<id>/app.db`.

**Names are unique, ids are forever.** Renaming keeps the id, so a token, a
config or a URL path referring to the app keeps working. Name comparisons are
case-insensitive: `Todos` and `todos` are one app.

**`clone` copies the shape, not the rows.** It is for trying a schema change or
standing up a staging copy of the structure; duplicating a live table is the
surprise, not the expectation.

**Deleting needs the id, not the name.** Deleting removes data, and a name is
something a person types by accident — an id is not.

```
$ nanobase app remove notes
nanobase: refusing to delete app_e4ad43bf1f84c6c5a2bfa212:
          pass --confirm app_e4ad43bf1f84c6c5a2bfa212
```

The app's data directory is deliberately left in place, so a deletion can be
undone by hand if it was a mistake.

**Importing over an existing name replaces it** rather than creating a second
app called the same thing — that is what a user with an edited export means.

## The API

Everything the platform can do is an HTTP route, so the command line is
one client among others and a program that embeds the platform reaches
the same operations.

```
nanobase serve [--host H] [--port P]        run the API (default 0.0.0.0:8090)
```

| route | what it does |
|---|---|
| `GET /api/health` | is it up |
| `POST /api/auth/login` | email and password in, a bearer token out |
| `GET /api/auth/me` | who the token belongs to |
| `GET /api/apps` | the apps |
| `POST /api/apps` | make one — `{"name": "todos"}` or a whole document |
| `GET /api/apps/{id}` | one app, by id or name |
| `PUT /api/apps/{id}` | replace its document |
| `PATCH /api/apps/{id}` | change part of its document |
| `DELETE /api/apps/{id}` | remove it — `{"confirm": "<id>"}` |
| `GET /api/apps/{id}/methods` | the custom methods |
| `POST /api/apps/{id}/methods/{name}` | call one — `{"args": ["…"]}` |
| `GET /api/apps/{id}/db` | the collections an app has |
| `GET /api/apps/{id}/db/{collection}` | the rows — `?filter={…}&limit=…&offset=…` |
| `POST /api/apps/{id}/db/{collection}` | put a row in |
| `PATCH /api/apps/{id}/db/{collection}` | change the rows that match — `{"filter": …, "changes": …}` |
| `DELETE /api/apps/{id}/db/{collection}` | take the rows that match out |
| `GET /api/events` | the events a successful change fires |

Every route but `health` and `login` wants `Authorization: Bearer <token>`.

```console
$ curl -s -X POST localhost:8090/api/auth/login \
    -H 'content-type: application/json' \
    -d '{"email":"ada@example.com","password":"correct horse"}'
{"token":"eyJ…","user":{"id":"…","email":"ada@example.com",…}}
```

## Where an app's rows live

An app's document says which engine its rows are in. The rows themselves
are collections of documents — the same shape everywhere — so the code
that serves an app does not change when the engine does.

| engine | where the rows are |
|---|---|
| `memory` | in the process, and gone when it stops |
| `sqlite` | a local file, one table per collection |
| `libsql`, `d1` | a serverless database behind an HTTP API |
| `postgres`, `mysql` | Neon, TiDB Cloud and PlanetScale, in their own APIs over HTTPS |

### Naming the cloud

A URL says a protocol more often than it says a cloud. `postgres://`
names PostgreSQL — and Neon, Vercel Postgres and Supabase all speak it,
in different ways. `mysql://` names MySQL — and TiDB Cloud and
PlanetScale both speak it over HTTP, and they are not interchangeable.

So the cloud may be named, and the name is what the platform uses:

```json
"database": { "type": "postgres", "provider": "neon",
              "url": "https://ep-cool.us-east-2.aws.neon.tech/mydb",
              "token": "…" }
```

The names are `turso`, `d1`, `neon`, `tidb` and `planetscale`, with the
spellings people use (`libsql`, `cloudflare`, `pscale`, `tidbcloud`,
`vercel`, `supabase`) accepted as well. A named cloud says which engine
it is, so `type` may be left out; a `type` that disagrees with the cloud
is refused, with both names in the message. The same name may be given
on the command line — `--provider NAME` for an app's database,
`--db-provider NAME` for the platform's own — and in the console, where
it is a dropdown beside the engine.

`AppDb` is the seam: the operations an app needs, and nothing else. The
filters are documents, not SQL — `{"done": false}` finds the rows that are
not done — and the matcher is the same one the in-process engine uses, so
a query means the same thing in every engine.

The two remote engines speak one protocol: a JSON request per operation,
a JSON reply per operation, over HTTP or over a socket. `DbServer` is the
other end of it, over any `AppDb`, so a deployment can put an app's rows
on another machine.

A service that holds an app's rows can ask for a token, and over a socket
the token rides in a header line before the request, the way HTTP would
carry it:

```text
authorization: Bearer s3cret
{"op":"find","collection":"todos","query":{"done":false}}
```

A request without the right token is refused, over HTTP and over a
socket alike.

### Reading an app's rows from the service

The service reaches an app's rows through a resolver: the host says how an
app's document turns into an open database, and the service does the rest.
The service never opens an engine itself, so the platform and a program
that embeds it can reach the same rows the same way.

```rust
use std::sync::Arc;
use nanobase::db::AppDb;
use nanobase::service::DbResolver;

let resolver: DbResolver = Arc::new(|record| {
    let db = nanobase::db::factory::open(&record.doc.config.database, &path)?;
    Ok(db as Arc<dyn AppDb>)
});
let service = Service::new("apps", store)?.with_dbs(resolver);

service.insert_row("todos", "notes", &mosql::Value::str("hello"))?;
let rows = service.find_rows("todos", "notes", &filter, Some(10), None)?;
```

A row that goes in, changes or comes out fires the same events as any
other change — `created`, `updated`, `removed` — so a listener sees them
whatever route made them. The context names the app the rows belong to
(`app`), the row's own id (`id`) and the collection in the method name
(`db.insert:notes`).

## Events, hooks and custom methods

The service is the one path every change takes, whether it comes from the
command line, the API, or a program that embeds the platform. It fires an
event after a successful change — `created`, `updated`, `patched`,
`removed` — runs `before` and `after` hooks around a call, and hosts
custom methods.

```rust
use std::sync::Arc;
use nanobase::{Event, Service};

let service = Arc::new(Service::new("apps", store)?);
service.on(Event::Created, Arc::new(move |ctx| {
    println!("created {:?}", ctx.id);
}));
service.method("count", Arc::new(|svc, _args| {
    Ok(mosql::Value::Int(svc.list()?.len() as i64))
}));
```

## CLI

```
nanobase serve   [--host H] [--port P]              run the HTTP API and the console
nanobase serve   --cors ORIGINS --rate-limit N     allow browser origins, limit requests
nanobase init   [--email E] [--password P]   create the owner (prompts if omitted)
nanobase login  <email> [--password P]       print a session token
nanobase me     [--token T]                  show who a token belongs to
nanobase status                            setup state, app count
nanobase app new <name>                    create an empty app
nanobase app import <file>                 create an app from a document
nanobase app list                          id, name, engine
nanobase app show <id|name>                everything about one app
nanobase app export <id|name> [--out F] [--redact]
nanobase app rename <id|name> <new-name>
nanobase app clone <id|name> <new-name>
nanobase app remove <id|name> --confirm <id>
nanobase app rows <id|name> <collection> [--filter JSON] [--limit N]
nanobase app insert <id|name> <collection> --json ROW
nanobase app update <id|name> <collection> --filter JSON --json CHANGES
nanobase app delete-rows <id|name> <collection> [--filter JSON]
nanobase new <name> [--out <file>]           write a new, empty app document
nanobase validate <file>                     check a document, list every problem
nanobase redact <file> [--out <file>]        copy a document without its secrets
```

All of them take `--data <file>` for the platform's state, defaulting to
`$NANOBASE_DATA` and then `./data/nanobase.db`. Passwords can come from
`NANOBASE_PASSWORD` instead of a flag, which keeps them out of a shell history.

In the serverless edition there is no `--data`: the platform's state is a
serverless database named with `--db-url` (`$NANOBASE_DB_URL`), and
`--db-token` (`$NANOBASE_DB_TOKEN`) is the bearer token for it. `nanobase
serve` and `nanobase db serve` are not in that binary at all — there is no
listener to serve.

The app commands can also talk to a running platform instead of the local
file, which makes them thin API clients:

```
nanobase app list --url http://localhost:8090 --token "$TOKEN"
```

`--url` (or `$NANOBASE_URL`) picks the platform; `--token` (or
`$NANOBASE_TOKEN`) is the bearer token to send. Without them the command
works on the local database.

`validate` reports **all** problems in one pass, from both reading and
checking, each with its field path:

```
$ nanobase validate broken.json
nanobase: 5 problem(s)
  config.database.type: unknown database "weaviate"; expected one of memory, sqlite, …
  config.storage.type: unknown storage type "ftp"; expected "local" or "s3"
  app.name: may only contain letters, digits, spaces, - and _
  schema.todos.columns.userId: autoIncrement is only valid on an integer primary key
  data.todos[0].title: an integer does not fit a text column
```

## A todo app, end to end

[`examples/todo/`](examples/todo) is a walkthrough for Windows: build the
binary, start it, create an app, put rows in it with `curl`, and open a
plain HTML page that talks to those rows. It also has a
[`serve.ps1`](examples/todo/serve.ps1) — a static file server small enough
to read before running, so the page is served over HTTP rather than from
the disk.

## Develop

```
./verify.sh            fmt, clippy, tests, serverless edition, publish dry-run, size budget
./verify.sh --quick    same, minus the dry-run (offline sandboxes)
./verify.sh serverless only the serverless edition's compile and tests
```

The last step of `verify.sh` fails if the release binary grows past 3 MB. It is
a tripwire for an accidental dependency, not a target to shave bytes against;
the room left is for the HTTP server and the query runtime. The same step
measures the serverless binary and fails if it is not *smaller* than the full
one — a feature gate that quietly stops excluding the listener is exactly the
regression the two editions exist to prevent.
