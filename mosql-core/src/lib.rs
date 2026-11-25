//! `mosql-core` — serverless databases over HTTP, from one connection URL.
//!
//! A serverless database is not a socket. It is an HTTPS endpoint that
//! takes a JSON request and answers with JSON, and the five that matter
//! most speak five different dialects of that:
//!
//! | provider | dialect | endpoint |
//! | --- | --- | --- |
//! | Turso / libSQL | SQLite | `https://{host}/v2/pipeline` |
//! | Cloudflare D1 | SQLite | `https://api.cloudflare.com/client/v4/accounts/{acct}/d1/database/{db}/query` |
//! | Neon / Vercel Postgres | PostgreSQL | `https://{host}/sql` |
//! | TiDB Cloud Serverless | MySQL | `https://http-{host}/v1beta/sql` |
//! | PlanetScale | MySQL (Vitess) | `https://{host}/psdb.v1alpha1.Database/Execute` |
//!
//! The point of this crate is that a caller does not have to know which
//! one it is talking to. It hands over a URL:
//!
//! ```
//! use mosql_core::driver::{AnyServerlessDriver, DriverConfig, Provider};
//!
//! let config = DriverConfig::from_url("libsql://db-name.turso.io?authToken=secret").unwrap();
//! assert_eq!(config.provider, Provider::Turso);
//! assert_eq!(config.dialect, mosql_core::driver::Dialect::Sqlite);
//! ```
//!
//! and gets back a driver that takes SQL and parameters:
//!
//! ```no_run
//! # use mosql_core::driver::{AnyServerlessDriver, SqlValue};
//! # async fn run<T: mosql_core::driver::HttpTransport>(transport: T) {
//! let driver = AnyServerlessDriver::from_url(
//!     "postgres://user:pass@ep-xxx.us-east-2.aws.neon.tech/db",
//!     transport,
//! )
//! .unwrap();
//! let rows = driver
//!     .execute("SELECT id FROM users WHERE age > $1", &[SqlValue::Int(18)])
//!     .await
//!     .unwrap();
//! # }
//! ```
//!
//! ## What is in here
//!
//! * [`driver::url`] — the connection string parser and the provider
//!   detector.
//! * [`driver::sql`] — a quote-aware scanner, so a `?` inside a string
//!   is not a parameter, and a parameter is never interpolated by
//!   accident.
//! * [`driver::encoding`] — base64, hex and percent-decoding, written
//!   here because a URL has all three in it.
//! * One module per provider.
//!
//! ## What is not in here
//!
//! There is no global state: no `static`, no `OnceLock`, no environment
//! variable read inside the driver. A driver owns its configuration and
//! its transport, and a caller owns the driver. That is what makes it
//! possible to test a wire protocol with a mock transport and nothing
//! else, and it is what makes it safe to run two drivers for two
//! databases in one process.
//!
//! The transport is a trait, so the crate does not care how the bytes
//! travel. A native build can post through the system `curl`; a Worker
//! can post through the host's `fetch`; a test can answer from a string.

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(missing_docs)]

#[macro_use]
extern crate alloc;

pub mod driver;
