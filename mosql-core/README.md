# mosql-core

Serverless databases over HTTP, from one connection URL.

A serverless database is not a socket. It is an HTTPS endpoint that takes a
JSON request and answers with JSON, and the five that matter most speak five
different dialects of that:

| provider | dialect | endpoint |
| --- | --- | --- |
| Turso / libSQL | SQLite | `https://{host}/v2/pipeline` |
| Cloudflare D1 | SQLite | `https://api.cloudflare.com/client/v4/accounts/{acct}/d1/database/{db}/query` |
| Neon / Vercel Postgres | PostgreSQL | `https://{host}/sql` |
| TiDB Cloud Serverless | MySQL | `https://http-{host}/v1beta/sql` |
| PlanetScale | MySQL (Vitess) | `https://{host}/psdb.v1alpha1.Database/Execute` |

The point of this crate is that a caller does not have to know which one it is
talking to. It hands over a URL:

```rust
use mosql_core::driver::{AnyServerlessDriver, SqlValue};

let driver = AnyServerlessDriver::from_url(
    "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb",
    transport,
)?;

let rows = driver
    .execute("SELECT id FROM users WHERE age > $1", &[SqlValue::Int(18)])
    .await?;
```

and gets back a driver that takes SQL and parameters.

**No crates.io dependencies.** The JSON model, the parser and the writer come
from [`mosql`](../rust), which has none either. There is no `sqlx`, no Diesel,
no OpenSSL, no Tokio, no Serde, no `url` crate, no base64 crate, no `chrono` —
so the crate builds for a WebAssembly module as happily as for a server, and
the whole thing is a few tens of kilobytes.

## The URLs

| URL | provider | notes |
| --- | --- | --- |
| `libsql://db.turso.io?authToken=…` | Turso | the token may also be the user info: `libsql://token@db.turso.io` |
| `turso://db.turso.io` | Turso | |
| `d1://{account}:{token}@api.cloudflare.com/{database}` | D1 | |
| `d1://{token}@api.cloudflare.com/{account}/{database}` | D1 | the other documented layout |
| `postgres://user:pass@ep-….neon.tech/db` | Neon | `postgresql://` is the same |
| `mysql://user:pass@gateway-….tidbcloud.com/db` | TiDB | `tidb://` is the same |
| `mysql://user:pass@aws.connect.psdb.cloud/db` | PlanetScale | `pscale://` is the same |

`?provider=tidb`, `?provider=planetscale`, `?provider=neon`, `?provider=d1`
or `?provider=turso` settles any ambiguity, on any scheme. A `mysql://` URL
whose host is neither a TiDB Cloud host nor a PlanetScale host is **refused**,
with a message that says so — the two are not interchangeable, and guessing
would send the caller to the wrong place with no warning.

A caller that knows which cloud it is talking to does not have to be
guessed at either. `DriverConfig::from_url_with_provider` takes the
provider as given, and then the URL is only the connection string:

```rust
// A console URL, a proxy, a host that looks like nothing in particular.
let config = DriverConfig::from_url_with_provider(
    "https://ep-cool.us-east-2.aws.neon.tech/mydb",
    Some(Provider::Neon),
)?;
assert_eq!(config.endpoint, "https://ep-cool.us-east-2.aws.neon.tech/sql");
```

A URL that names a provider of its own and a provider the caller names
have to agree; two instructions that contradict each other are a mistake,
and the error says which is which.

Neon's endpoint is `https://{host}/sql` unless `?fetch_endpoint=` says
otherwise, and the whole connection string goes back out in a
`Neon-Connection-String` header, because Neon is a proxy and the pool behind
it is chosen by that string.

## What is in here

| module | what it does |
| --- | --- |
| `driver::url` | `DriverConfig::from_url` — the connection string parser and the provider detector |
| `driver::sql` | the quote-aware scanner, `rewrite_pg_placeholders`, `inline_mysql_params` |
| `driver::encoding` | base64, hex and percent-decoding, written here because a URL has all three in it |
| `driver::turso` | the libSQL pipeline |
| `driver::d1` | the D1 query endpoint |
| `driver::neon` | the Neon HTTP API, with the Postgres type OIDs |
| `driver::tidb` | the TiDB proxy, with its session and its rollback |
| `driver::planetscale` | the Vitess `Execute` endpoint, with its session and its rollback |
| `driver::transport` | the transports: `CurlTransport`, `HostTransport` |

## The API

```rust
pub enum SqlValue { Null, Bool(bool), Int(i64), Float(f64), Text(String), Blob(Vec<u8>) }
pub struct QueryResult { pub columns: Vec<String>, pub rows: Vec<Vec<SqlValue>>,
                         pub rows_affected: u64, pub last_insert_id: Option<i64> }
pub struct DbError { pub code: Option<String>, pub message: String, pub status: Option<u16> }
pub enum Dialect { Sqlite, Postgres, Mysql }
pub enum Provider { Turso, D1, Neon, Tidb, PlanetScale }

pub trait HttpTransport {
    async fn post_json(&self, url: &str, headers: &[(&str, &str)], body: String)
        -> Result<(u16, String), DbError>;
    async fn post_json_with_headers(&self, url: &str, headers: &[(&str, &str)], body: String)
        -> Result<(u16, String, Vec<(String, String)>), DbError> { … }
}

pub trait ServerlessDriver {
    fn dialect(&self) -> Dialect;
    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError>;
    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError>;
}

impl<T: HttpTransport> AnyServerlessDriver<T> {
    pub fn from_url(url: &str, transport: T) -> Result<Self, DbError>;
}
```

`batch` means what the provider can do: one pipeline for Turso, one request
with `Neon-Batch` for Neon, one transaction for TiDB and PlanetScale, and one
statement at a time for D1, which has no batch endpoint.

## The transports

The driver does not care how the bytes travel — the transport is a trait, so a
wire protocol can be tested with a mock and nothing else. The concrete
transports are behind features:

| feature | transport | where it runs |
| --- | --- | --- |
| `transport-curl` | `CurlTransport` | a native build: the system `curl`, which brings TLS with it |
| `transport-worker` | `HostTransport` | a Worker, or any host with its own `fetch` |
| `transport-ureq` | — | reserved: it needs the `ureq` crate from a registry |

`CurlTransport` is the platform's own answer to "HTTPS without a registry":
every server has `curl`. The body goes in through stdin, so nothing is spelled
out on a command line, and the reply's headers come back too — which is what
TiDB's session needs.

`HostTransport` takes a function, so the host owns the networking:

```rust
use mosql_core::driver::HostTransport;

let transport = HostTransport::new(|url: &str, headers: &[(&str, &str)], body: String| async move {
    let mut request = worker::Fetch::Request::new(url)?;
    for (name, value) in headers {
        request.headers().append(name, value)?;
    }
    request = request.body(body.into_bytes())?;
    let response = request.send().await?;
    let status = response.status_code();
    let headers = response.headers()?.iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    Ok((status, response.text().await?, headers))
});
```

## No global state

There is no `static`, no `OnceLock`, no environment variable read inside the
driver. A driver owns its configuration and its transport, and the caller owns
the driver. That is what makes it possible to test a wire protocol with a mock
transport and nothing else, and it is what makes it safe to run two drivers for
two databases in one process.

## Parameters

A `?` is a parameter only when it is not inside `'…'`, not inside `"…"` or
`` `…` ``, and not inside a `--` or `/* … */` comment. The count has to match
the number of values, or the statement is refused before it is sent.

* **PostgreSQL** — the placeholders are renumbered: `?` becomes `$1`, `$2`, …
  and the values travel as `params`.
* **MySQL** — the values are put into the statement, escaped the way MySQL
  escapes them, so a value cannot change the statement. A float that is not
  finite has no SQL spelling and is refused.
* **SQLite** — the values travel as arguments, and a whole number goes as a
  string, because a JSON number cannot hold every `i64` exactly and a database
  row can.

## Building it

```sh
./verify.sh              # fmt, clippy, tests, dependencies, wasm32, features
./verify.sh --quick      # everything except the WebAssembly build
./verify.sh test         # one stage
```

The WebAssembly build is the one a Cloudflare Worker runs:

```sh
cargo check --target wasm32-unknown-unknown --no-default-features
```

With the standard library switched off the crate is `#![no_std]` + `alloc`,
which is what keeps it free of a TLS stack, a thread, a clock and a
filesystem.
