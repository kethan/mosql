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

Release binary today: **~1.4 MB** (SQLite is ~0.9 MB of it), against a 3 MB
budget that [`verify.sh`](verify.sh) enforces.

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
| App database: memory, SQLite, HTTP, TCP | done — 6 tests |
| Serving an app's rows over the API | next |
| SQL engines (Postgres, MySQL) | next — needs a driver |

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
| `postgres`, `mysql` | not built into this binary yet — they need a driver |

`AppDb` is the seam: the operations an app needs, and nothing else. The
filters are documents, not SQL — `{"done": false}` finds the rows that are
not done — and the matcher is the same one the in-process engine uses, so
a query means the same thing in every engine.

The two remote engines speak one protocol: a JSON request per operation,
a JSON reply per operation, over HTTP or over a socket. `DbServer` is the
other end of it, over any `AppDb`, so a deployment can put an app's rows
on another machine.

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
nanobase serve   [--host H] [--port P]              run the HTTP API
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
nanobase new <name> [--out <file>]           write a new, empty app document
nanobase validate <file>                     check a document, list every problem
nanobase redact <file> [--out <file>]        copy a document without its secrets
```

All of them take `--data <file>` for the platform's state, defaulting to
`$NANOBASE_DATA` and then `./data/nanobase.db`. Passwords can come from
`NANOBASE_PASSWORD` instead of a flag, which keeps them out of a shell history.

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

## Develop

```
./verify.sh            fmt, clippy, tests, publish dry-run, size budget
./verify.sh --quick    same, minus the dry-run (offline sandboxes)
```

The last step of `verify.sh` fails if the release binary grows past 3 MB. It is
a tripwire for an accidental dependency, not a target to shave bytes against;
the room left is for the HTTP server and the query runtime.
