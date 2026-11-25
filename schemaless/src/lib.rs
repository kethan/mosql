//! Schema-less SQL collections on top of the [`mosql`] query builder.
//!
//! This is the port of umosql's `schemaless.js` and `client.js`. A collection
//! has no declared schema: the first document creates the table, a document
//! with a new field adds the column, a wider value widens the column, and a
//! statement that fails because the table or a column is missing is retried
//! after the DDL runs.
//!
//! # The driver is one trait
//!
//! `createSQLAdapter({ database, execute })` takes a closure that runs one
//! statement. That closure is the whole driver contract, and it is why umosql
//! bundles no driver. [`SqlExecutor`] is the same seam in Rust: implement it
//! over rusqlite, tokio-postgres, mysql_async, a serverless HTTP driver, or a
//! test double, and every collection in this crate works.
//!
//! ```
//! use mosql::parse_json;
//! use mosql_schemaless::{Client, CollectionOptions, SqlExecutor};
//! use uflite::db::Collection;
//!
//! # async fn run(executor: impl SqlExecutor + 'static) -> Result<(), mosql::Error> {
//! let client = Client::with_executor(mosql::Db::Pg, executor);
//! let users = client.db("app").collection_with(
//!     "users",
//!     CollectionOptions {
//!         schema: parse_json(r#"{"age": "INTEGER"}"#)?,
//!         ..CollectionOptions::default()
//!     },
//! );
//!
//! // Creates the table, then the column `name` did not exist for.
//! let id = users.insert_one(parse_json(r#"{"name": "Ada", "age": 36}"#)?).await?;
//!
//! // Reads go through the same Mongo-shaped API the services expect.
//! let adult = users
//!     .find_one(&parse_json(r#"{"age": {"$gte": 18}}"#)?, None)
//!     .await?;
//! # let _ = (id, adult); Ok(()) }
//! ```
//!
//! The collection implements [`uflite::db::Collection`], so
//! [`uflite::DbService`](uflite::DbService) runs unchanged over SQL:
//!
//! ```
//! # use mosql_schemaless::Client;
//! # use uflite::{DbOptions, DbService};
//! # fn run(client: Client) {
//! let users = client.db("app").collection("users");
//! let service = DbService::new(users, DbOptions::default());
//! # let _ = service;
//! # }
//! ```
//!
//! # Not ported
//!
//! The MongoDB backend (a driver, and not a SQL dialect) and `loadDriver`
//! itself: Rust has no runtime `import()`, so a driver is a dependency chosen
//! in `Cargo.toml` rather than a package installed at call time. The "driver
//! not installed" message therefore has no equivalent to port.

pub mod adapter;
pub mod client;
pub mod collection;
pub mod executor;
pub mod infer;

pub use adapter::{quote_ident, ColumnOptions, SqlAdapter};
pub use client::{AnyCollection, Client, Database, Kind};
pub use collection::{CollectionOptions, IdStrategy, SqlCollection, Upserted};
pub use executor::{ExecResult, Missing, SqlError, SqlExecutor};
pub use infer::{base_type, escape_value, infer_sql_type, widen_type};
