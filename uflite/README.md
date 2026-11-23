# uflite services, in Rust

A port of [uflite](https://github.com/kethan/uflite)'s `src/services` — the
Feathers-style service layer between a router and a database — on top of the
[`mosql`](../rust) crate. Documents are `mosql::Value` (JSON) in and out, every
method is `async fn` with `Send` futures, and the only dependency is `mosql`
itself: no runtime, no TLS, no AWS SDK, no regex crate.

| `uflite/src/services` | Rust | Status |
| --- | --- | --- |
| `db.js` | [`uflite::DbService`](src/db/mod.rs) | complete — CRUD, Feathers pagination, `multi` rules, over any [`Collection`](src/db/collection.rs) |
| `table.js` | [`uflite::table::TableService`](src/table.rs) | complete — DDL validation, delegates to a [`TableDb`](src/table.rs) |
| `acl.js` | [`uflite::acl::Acl`](src/acl.rs) | complete — policy validation, `can(context, permission)`, with uqry's `filter` ported in [`matcher.rs`](src/matcher.rs) |
| `local-storage.js` | [`uflite::storage::LocalStorage`](src/storage/local.rs) | complete — key safety, symlink refusal, `maxFileSize`, cursor paging, presign URLs |
| `storage.js` (S3) | [`uflite::storage::S3Storage`](src/storage/s3.rs) | complete over an [`S3Client`](src/storage/s3.rs) trait you implement |
| `auth.js`, `sql-adapter-factory.js` | — | not ported: better-auth adapters and SQL driver wiring (see [Not ported](#not-ported)) |

## Usage

```rust
use mosql::{parse_json, Value};
use uflite::{DbOptions, DbService, FindResult, Params};

# async fn run() -> Result<(), uflite::Error> {
// An isolated in-memory collection, like `createDBService()`.
let users = DbService::memory(vec![
    parse_json(r#"{"_id": 1, "name": "Ada", "age": 36}"#)?,
    parse_json(r#"{"_id": 2, "name": "Bob", "age": 17}"#)?,
], DbOptions::default());

// find: { total, limit, skip, data } — `$limit`/`$skip`/`$sort`/`$select` never reach the filter
let adults = users.find(&Params::new()
    .query_with(parse_json(r#"{"age": {"$gte": 18}, "$sort": {"age": -1}}"#)?)?).await?;
let FindResult::Paginated(page) = &adults else { unreachable!() };
assert_eq!(page.total, 1);

// get → 404 on a missing id, with the JavaScript message
assert_eq!(users.get(&Value::str("nope"), &Params::new()).await.unwrap_err().status(), 404);

// create returns the stored document with its generated `_id`
let created = users.create(parse_json(r#"{"name": "Cyd"}"#)?, &Params::new()).await?;
# let _ = created; Ok(()) }
```

Any storage works by implementing `Collection` — the subset of the Mongo
driver API db.js is written against — and `DbService::new(collection, options)`
uses it directly:

```rust
impl uflite::db::Collection for rusqlite::Connection { /* count_documents, find, find_one, insert_*, update_*, delete_* */ }
```

ACL checks are the same policy objects the JavaScript accepts, including
`$expr` ownership rules:

```rust
let acl = uflite::acl::Acl::new(&parse_json(r#"{
  "enabled": true,
  "rules": [{"id": "owner", "permissions": ["user:posts:update"],
             "check": {"$expr": {"$eq": ["$user.id", "$post.authorId"]}}}]
}"#)?)?;
let ctx = parse_json(r#"{"user": {"id": 10}, "post": {"authorId": 10}}"#)?;
assert!(acl.can(&ctx, "user:posts:update")?);
```

Table DDL is validated here and executed by your adapter; the default trait
methods return `The database adapter does not support <method>`, so implement
only what your database can do:

```rust
# async fn run() -> Result<(), uflite::Error> {
let tables = uflite::table::TableService::new(my_adapter); // or &adapter, Arc<adapter>
tables.create(&parse_json(r#"{"name": "users", "schema": {"properties": {"name": "string"}}}"#)?).await?;
# Ok(()) }
```

Local storage keeps the JavaScript service's safety rules — relative keys only,
`lstat` on every segment, `O_NOFOLLOW`, `0600`, `maxFileSize`:

```rust
# async fn run() -> Result<(), uflite::Error> {
let files = uflite::storage::LocalStorage::new(uflite::storage::LocalOptions::default().base_path("./data/files"))?;
let uploaded = files.upload(
    uflite::storage::UploadInput { key: "avatars/ada.png".into(), body: bytes, content_type: Some("image/png".into()) },
    &Params::new()).await?;
let page = files.find(&uflite::storage::StorageQuery::default().prefix("avatars/").limit(50)).await?; // sorted keys + nextCursor
# let _ = page; Ok(()) }
```

S3 is the same service with the wire protocol left to you. Implement four
calls — `HeadObject`, `DeleteObject`, `ListObjectsV2` and `getSignedUrl` — over
the AWS SDK or anything else; the crate does the validation, defaults and
result shapes:

```rust
# async fn run() -> Result<(), uflite::Error> {
struct MyClient(aws_sdk_s3::Client);
impl uflite::storage::S3Client for MyClient { /* head_object, delete_object, list_objects, presign */ }

let options = uflite::storage::S3Options::new("uploads", "us-east-1", "key", "secret");
let storage = uflite::storage::S3Storage::new(MyClient(client), options);
let grant = storage.presign_upload(&parse_json(r#"{"key": "a/b.png", "contentType": "image/png"}"#)?).await?;
// { key, url, method: "PUT", headers: {"Content-Type": "image/png"}, expiresIn: 900 }
# let _ = grant; Ok(()) }
```

Errors are one enum with the HTTP status uflite's REST layer would send and
the exact JavaScript message:

| `Error` | status | JS `name` | raised by |
| --- | --- | --- | --- |
| `BadRequest` | 400 | `TypeError` | validation (keys, identifiers, options, limits) |
| `NotFound` | 404 | `NotFound` | `get` on a missing record/file (`No record found for id '…'`) |
| `TooLarge` | 413 | `RangeError` | `upload` beyond `maxFileSize` |
| `Internal` | 500 | `Error` | `Multi … is not allowed`, unsupported adapter capability, provider errors, `contentLength` on a presigned PUT |
| `Query` | 500 | `Error` | the collection rejected a query/update (`mosql::Error`) |

`Value::from(error)` is the JSON body: `{ name, message, code, status }`.

## Fidelity

The port is line-for-line where behaviour is observable: every message,
default (`paginate: 10/100`, `limit: 100`, `expiresIn: 900`, `maxFileSize: 25 MiB`),
validation order and edge case the JavaScript has, including the ones that
look accidental — `$limit: 0` returns `{ total, limit: 0, skip, data: [] }`,
`update` unsets fields missing from the replacement, a falsy intermediate in a
dotted ACL path is returned as-is (`{ "zero.x": 0 }` matches), `$expr.$in`
compares against the *unevaluated* second argument when it is not an array,
`$avg` divides by the argument count, and `maxFileSize: "1024"` is rejected
because `Number.isSafeInteger("1024")` is false.

Deliberate deviations, all in the stricter direction:

* `/^\w+$/` and the column-type regex accept a trailing `\n` in JavaScript
  (`$` matches before a final newline). A newline in an identifier or type is
  refused here.
* uqry's `dlv` indexes strings (`"ab"[1]`) and reads `array.length`; ACL
  contexts never do, so those paths read as `undefined`.
* `$where` takes a JavaScript function and cannot run; it is an error.
* `presign` as an option is the JavaScript way of passing the signing function.
  Here it is `S3Client::presign`; the key is only used to detect an S3 config.

## Performance

Same workload through `createDBService` (node 22, `umosql/memory` 0.3.1) and
`DbService::memory` (rustc 1.97, release), 1 000 documents, single thread,
`examples/bench.rs`:

| operation | JavaScript | Rust | |
| --- | --- | --- | --- |
| `find` — `age ≥ 30`, `$sort`, `$limit: 20` | 2 768 µs | 421 µs | **6.6×** |
| `get(500)` | 236 µs | 14.8 µs | **16×** |
| `create` + `remove` | 748 µs | 42 µs | **18×** |
| `acl.can` — 3 rules, `$expr` check | 1.56 µs | 0.33 µs | **4.7×** |

## Testing

`tests/` ports the uflite specs (`db`, `table`, `acl`, `storage`) and uqry's
`filter`/`expression` suite — 84 tests, every `it` from the JavaScript files
except the ones that exercise `flite()` routing and hooks. The two REST-shaped
cases became assertions on the error the REST layer serialises. Run
`./verify.sh` (or `--quick` offline, which skips the publish step); CI runs
it on Linux, macOS and Windows and finishes with `cargo publish --dry-run`.
The crate depends on `mosql` `0.1` by path, so publish `mosql` first — until
it is on crates.io the dry-run reports that single expected failure and the
gate ignores only it.

## Not ported

* **`auth.js`** wraps [better-auth](https://better-auth.com) adapters; it is
  JavaScript-ecosystem code with no Rust counterpart.
* **`sql-adapter-factory.js`** builds umosql SQL adapters from a driver. A
  Rust app implements `Collection`/`TableDb` over rusqlite, sqlx or
  tokio-postgres instead — the traits are the contract.
* **`Symbol.for('uflite.routeMount')`** and service hooks are framework
  concerns: routes, hooks and ACL `before` hooks belong to the host router.
  The storage test shows the hook pattern (`can(params, "user:storage:…")`).
