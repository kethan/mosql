# nanobase

A PocketBase-style backend platform that runs on your own database.

One binary. It keeps a platform database of **apps**, and each app brings its
own database, auth providers, storage bucket and permission rules. The runtime
API is per-app: `/api/v1/apps/{appId}`.

**This crate has no external dependencies.** Everything it needs — JSON, the
query builder, the in-memory engine, permission evaluation — comes from
[`mosql`](../rust) and [`uflite`](../uflite), which are also dependency-free.
That is a requirement, not an accident: the platform is meant to be small
enough to ship as a single binary, and it is built in an environment with no
registry access at all.

Release binary today: **~454 KB**, against a 3 MB budget that
[`verify.sh`](verify.sh) enforces.

## Status

| Piece | State |
| --- | --- |
| App documents (read, validate, write) | done — 44 tests |
| Secret handling (`SECRET_*`, redaction) | done |
| CLI: `new`, `validate`, `redact` | done |
| Platform database, first-run onboarding | next |
| Auth (email/password, sessions, JWT) | next |
| HTTP server and routes | next |

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

## CLI

```
nanobase new <name> [--out <file>]    write a new, empty app document
nanobase validate <file>              check a document, list every problem
nanobase redact <file> [--out <file>] copy a document without its secrets
```

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
a tripwire for an accidental dependency, not a target to shave bytes against.
