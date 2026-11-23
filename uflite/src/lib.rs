//! # uflite services, in Rust
//!
//! A port of [uflite](https://github.com/kethan/uflite)'s `src/services`: the
//! Feathers-style service layer that sits between a router and a database.
//! Everything here is `async fn` in traits with `Send` futures, so it drops into
//! tokio or any other multi-threaded runtime without boxing, and the only
//! dependency is the `mosql` crate for documents, queries and the memory engine.
//!
//! | JavaScript | Rust | What it does |
//! | --- | --- | --- |
//! | `services/db.js` | [`db::DbService`] | CRUD + pagination over any [`db::Collection`] |
//! | `services/table.js` | [`table::TableService`] | validates DDL requests, delegates to a [`table::TableDb`] |
//! | `services/acl.js` | [`acl::Acl`] | policy validation and `can(context, permission)` |
//! | `services/local-storage.js` | [`storage::LocalStorage`] | file storage with key safety, limits and cursor paging |
//! | `services/storage.js` | [`storage::S3Storage`] | S3 validation/dispatch over an [`storage::S3Client`] you implement |
//! | `services/auth.js`, `sql-adapter-factory.js` | — | better-auth adapters; no Rust counterpart, see the README |
//!
//! Documents are [`mosql::Value`] throughout, so a service call is a JSON value
//! in and a JSON value out, exactly like the JavaScript API.
//!
//! ```
//! use uflite::{DbOptions, DbService, Params};
//! use mosql::{parse_json, Value};
//!
//! # async fn demo() -> Result<(), uflite::Error> {
//! let docs = vec![
//!     parse_json(r#"{"_id": 1, "name": "Ada", "age": 36}"#).unwrap(),
//!     parse_json(r#"{"_id": 2, "name": "Bob", "age": 17}"#).unwrap(),
//! ];
//! let users = DbService::memory(docs, DbOptions::default());
//!
//! // Feathers pagination: { total, limit, skip, data }
//! let adults = users
//!     .find(&Params::new().query_with(parse_json(r#"{"age": {"$gte": 18}, "$sort": {"age": -1}}"#)?)?)
//!     .await?;
//! let uflite::db::FindResult::Paginated(page) = &adults else { unreachable!() };
//! assert_eq!(page.total, 1);
//! assert_eq!(page.data[0].get("name").unwrap().as_str(), Some("Ada"));
//!
//! // create returns the stored document, with its generated _id
//! let created = users
//!     .create(parse_json(r#"{"name": "Cyd", "age": 44}"#)?, &Params::new())
//!     .await?;
//! assert!(created.get("_id").is_some());
//! # Ok(())
//! # }
//! ```
//!
//! Errors are one enum with an HTTP status ([`Error::status`]) and the exact
//! JavaScript message, so `error.status` ports unchanged.

#![forbid(unsafe_code)]

pub mod acl;
pub mod db;
pub mod error;
pub mod matcher;
pub mod params;
pub mod storage;
pub mod table;

pub use db::{DbOptions, DbService, FindOptions, FindResult, Multi, Page, Paginate};
pub use error::{Error, Result};
pub use params::{to_projection, Params};

/// uflite version this port tracks.
pub const UFLITE_VERSION: &str = "0.3.0";
