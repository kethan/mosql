//! # mosql (Rust)
//!
//! A zero-dependency Rust port of [`umosql`](https://github.com/kethan/mosql):
//! build PostgreSQL / MySQL / SQLite SQL from MongoDB-style queries.
//!
//! ```no_run
//! use mosql::{parse_json, Builder, Db, Value};
//!
//! let query = parse_json(r#"{"age": {"$gte": 18}}"#)?;
//! let sql = Builder::full()
//!     .collection("users", Db::Pg)?
//!     .find(&query, None)?
//!     .sort(&parse_json(r#"{"age": -1}"#)?)?
//!     .limit(&Value::Int(10))?
//!     .to_sql()?;
//! # Ok::<(), mosql::Error>(())
//! ```
//!
//! Every generated fragment is byte-identical to the JavaScript implementation —
//! see `tests/parity.rs`, which replays fixtures produced by the JS builder.

#![no_std]
#![allow(missing_docs)]

#[macro_use]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

#[cfg(all(target_arch = "wasm32", not(feature = "std")))]
mod wasm_alloc;

mod builder;
pub mod dispatch;
mod error;
mod escape;
pub mod float;
mod json_path;
#[cfg(feature = "memory")]
pub mod memory;
mod ops;
mod stages;
mod validate;
mod value;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

// The raw Node-API declarations exist only for the bridge below. They must be
// gated exactly like it: on Windows a DLL has to resolve every import at link
// time, so compiling them without the feature (and without Node's symbols)
// fails to link. Linux and macOS tolerate undefined symbols in a shared
// object, which is how this went unnoticed until the Windows CI job.
#[cfg(all(not(target_arch = "wasm32"), feature = "napi"))]
pub mod node_api;

#[cfg(all(not(target_arch = "wasm32"), feature = "napi"))]
pub mod napi;

pub use builder::{Builder, Collection, CrudOptions, FindQuery};
pub use error::{Error, Result};
pub use json_path::{batch_json_update, json_path, json_update, Cast, JsonOp};
#[cfg(feature = "memory")]
pub use memory::{Collection as MemoryCollection, Database as MemoryDatabase};
pub use value::{parse_json, to_json, Object, Value};

/// Target SQL dialect.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Db {
    /// SQLite
    Sqlite,
    /// PostgreSQL
    Pg,
    /// MySQL
    Mysql,
}

impl Db {
    /// Parse a dialect name. Unknown names fall back to SQLite, exactly like the
    /// JS implementation (which compares strings and treats everything else as
    /// the default branch).
    pub fn parse(s: &str) -> Db {
        match s {
            "pg" => Db::Pg,
            "mysql" => Db::Mysql,
            _ => Db::Sqlite,
        }
    }

    /// The name used in the JS API.
    pub fn as_str(&self) -> &'static str {
        match self {
            Db::Sqlite => "sqlite",
            Db::Pg => "pg",
            Db::Mysql => "mysql",
        }
    }
}

impl core::str::FromStr for Db {
    type Err = Error;

    fn from_str(s: &str) -> Result<Db> {
        Ok(Db::parse(s))
    }
}
