# mosql-schemaless

A port of umosql's `schemaless.js` and `client.js`: a Mongo-shaped collection
over a SQL table that creates and migrates its own schema as documents arrive.

Nothing is declared up front. The first document creates the table, a document
with a new field adds the column, a wider value widens the column, and a
statement that fails because the table or a column is missing is retried after
the DDL runs.

## The driver is one trait

`createSQLAdapter({ database, execute })` takes a closure that runs one
statement — that closure is the whole driver contract, and it is why umosql
bundles no driver at all. [`SqlExecutor`](src/executor.rs) is the same seam in
Rust:

```rust
pub trait SqlExecutor: Send + Sync {
    fn execute<'a>(&'a self, sql: &'a str, params: &'a [Value])
        -> BoxFuture<'a, Result<ExecResult, SqlError>>;
}
```

Implement it over rusqlite, tokio-postgres, mysql_async, a serverless HTTP
driver, or a test double, and every collection in this crate works. The future
is boxed so the trait stays object-safe: one client holds one
`Arc<dyn SqlExecutor>`, whatever the driver is. That allocation is noise next
to the round trip it wraps.

## Usage

```rust
use mosql::parse_json;
use mosql_schemaless::{Client, CollectionOptions};
use uflite::db::Collection;

# async fn run(executor: impl mosql_schemaless::SqlExecutor + 'static) -> Result<(), mosql::Error> {
let client = Client::with_executor(mosql::Db::Pg, executor);
let users = client.db("app").collection_with(
    "users",
    CollectionOptions {
        schema: parse_json(r#"{"age": "INTEGER"}"#)?,
        ..CollectionOptions::default()
    },
);

// Creates the table, then the column `name` did not exist for.
let id = users.insert_one(parse_json(r#"{"name": "Ada", "age": 36}"#)?).await?;

// Reads go through the same Mongo-shaped API the services expect.
let adult = users.find_one(&parse_json(r#"{"age": {"$gte": 18}}"#)?, None).await?;
# let _ = (id, adult); Ok(()) }
```

`Client::memory()` is the in-memory backend, which needs no driver: one isolated
store per database name, so `db("a")` and `db("b")` never share documents.

## The service layer runs unchanged over SQL

A collection implements `uflite::db::Collection` — the Mongo-driver subset the
service layer is written against — so `DbService` works over any backend with
no adapter of its own:

```rust
let service = DbService::new(client.db("app").collection("users"), DbOptions::default());
let page = service.find(&params).await?;   // paginated, sorted, projected
```

## What is ported

| `schemaless.js` | here |
| --- | --- |
| `SQLCollection` | [`SqlCollection`](src/collection.rs) — `initialize`, `createTable`, `migrate`, `classifyError` + DDL retry, `insertOne/Many`, `find`, `findOne`, `updateOne/Many`, `upsertOne`, `deleteOne/Many`, `countDocuments`, `distinct`, `aggregate`, `createIndex`, `dropColumn` |
| `createSQLAdapter` | [`SqlAdapter`](src/adapter.rs) — `execute`, `tableExists`, `getTableSchema`, `listCollections`, `dropCollection`, `createTableWithSchema`, `addColumn`, `renameColumn`, `modifyColumn`, `dropIndex` |
| `createSchemalessClient` | [`Client`](src/client.rs) — `db(name).collection(col)` over memory or any executor |
| `inferSQLType`, `widenType`, `escapeValue`, `baseType` | [`infer`](src/infer.rs) |
| `normalizeResult` | [`ExecResult`](src/executor.rs) — the four driver shapes unwrapped once, by the driver |

Faithful to the JavaScript, including the parts that look accidental: the table
is created with only the id and the timestamps and every column is added with
its own `ALTER TABLE`; `$limit: 0`-style edge cases in the builder behave as
before; and `migrate` replaces its schema cache with the snapshot it read
*before* migrating, so a column added by a migration can be re-added on the next
statement (the duplicate `ALTER` fails and is ignored — observable only in the
SQL log).

Deliberate deviations:

* `loadDriver` has no equivalent. Rust has no runtime `import()`, so a driver is
  a dependency chosen in `Cargo.toml`, and the "driver not installed" message
  has nothing to port.
* The MongoDB backend is not ported: it is a driver, and not a SQL dialect.
* `schemaDef.default` cannot be a function — `Value` holds data, not closures.
  Apply a computed default before the insert instead.
* MySQL identifier quoting covers reserved words only, as `qi` does. An invalid
  name is passed through so the database reports it, exactly like the adapter.

## Testing

`tests/collection.rs` (27) walks the DDL: what each dialect creates, type
inference and widening, `NOT NULL`/`UNIQUE`/`DEFAULT`, the retry-after-DDL paths
for a missing table and a missing column, id strategies, hidden fields, and
defaults. `tests/client.rs` (6) covers the factory, memory isolation, and the
service layer running over SQL.

The fake in `tests/common/mod.rs` maintains a real table/column map from the
generated `CREATE TABLE` / `ALTER TABLE` statements, so a test asserting "the
column was added" is checking the DDL round-trip rather than a string that
happens to appear in a log.

Run `./verify.sh` (or `--quick` offline); CI runs it on Linux, macOS and Windows
and finishes with `cargo publish --dry-run`.
