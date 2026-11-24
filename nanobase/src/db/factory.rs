//! Opening the database an app's document asks for.
//!
//! The document says which engine the rows are in. This reads that and
//! opens it: memory, a local SQLite file, a serverless database behind an
//! HTTP API, or a database service on a socket.
//!
//! The SQL engines — Postgres, MySQL — are not here yet. They want a
//! driver, and a driver is a dependency this crate does not have. What is
//! here is the seam they will slot into: [`AppDb`] is what the runtime
//! sees, so a new engine is a new adapter and nothing else changes.

use std::path::Path;
use std::sync::Arc;

use uflite::error::{Error, Result};

use crate::config::{DatabaseConfig, DbKind};
use crate::db::http::HttpDb;
use crate::db::memory::MemoryDb;
use crate::db::sqlite::SqliteDb;
use crate::db::tcp::TcpDb;
use crate::db::AppDb;

/// The prefix that says a database is on a socket rather than on the web.
const TCP_SCHEME: &str = "tcp://";

/// Open the database an app's configuration asks for.
///
/// `path` is where a local file goes: the app's own data directory. A
/// remote engine uses the URL in the configuration instead.
pub fn open(config: &DatabaseConfig, path: &Path) -> Result<Arc<dyn AppDb>> {
    // A socket is a socket whatever the engine says: the protocol is the
    // app-database one, and it does not care which engine is behind it.
    // The scheme in the URL is what picks the transport.
    if let Some(address) = config
        .url
        .as_deref()
        .and_then(|url| url.strip_prefix(TCP_SCHEME))
    {
        return open_tcp_at(config, address);
    }
    match config.kind {
        DbKind::Memory => Ok(Arc::new(MemoryDb::new())),
        DbKind::Sqlite => Ok(Arc::new(SqliteDb::open(path.join("app.db"))?)),
        DbKind::Libsql | DbKind::D1 => {
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| Error::internal("this database needs a url"))?;
            let mut db = HttpDb::new(url, Arc::new(crate::httpclient::CurlHttpClient));
            if let Some(token) = &config.token {
                db = db.with_token(token.clone());
            }
            Ok(Arc::new(db))
        }
        DbKind::Postgres | DbKind::Mysql => Err(Error::internal(format!(
            "the {} engine is not built into this binary yet",
            config.kind.as_str()
        ))),
    }
}

/// The database an app's configuration asks for, over a socket.
///
/// The URL is a `host:port`, so a database service on the same network
/// answers the same protocol the HTTP service does.
pub fn open_tcp(config: &DatabaseConfig) -> Result<Arc<dyn AppDb>> {
    match config.kind {
        DbKind::Postgres | DbKind::Mysql | DbKind::Libsql | DbKind::D1 => {
            let address = config
                .url
                .as_deref()
                .ok_or_else(|| Error::internal("this database needs a url"))?;
            open_tcp_at(config, address)
        }
        other => Err(Error::internal(format!(
            "the {} engine is not reached over a socket",
            other.as_str()
        ))),
    }
}

/// The same, at an address the caller already knows.
fn open_tcp_at(config: &DatabaseConfig, address: &str) -> Result<Arc<dyn AppDb>> {
    let mut db = TcpDb::new(address);
    if let Some(token) = &config.token {
        db = db.with_token(token.clone());
    }
    Ok(Arc::new(db))
}
