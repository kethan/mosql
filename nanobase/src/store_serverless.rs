//! The platform's own data, in a serverless database.
//!
//! The local build keeps the platform in a SQLite file. This is the same
//! platform in a database that is reached over the network: Turso, libSQL,
//! Neon, or anything else that answers the app-database protocol over
//! HTTP.
//!
//! The platform's state is one document — the signing key, the accounts
//! and the apps — and it is stored as one row. A serverless database
//! cannot hold a transaction open while a password is hashed, and the
//! platform's state is small enough that one row is the right shape for
//! it: the whole catalog is read, changed and written back.
//!
//! The collection and the row's key are fixed, so two processes writing
//! the same platform are still writing the same row.

use std::sync::Arc;

use mosql::{Object, Value};
use uflite::error::{Error, Result};

use crate::db::{AppDb, Row};
use crate::platform::{BoxFuture, Platform, PlatformStore};

/// The collection the platform's state lives in.
pub const COLLECTION: &str = "platform";

/// The key of the one row that holds it.
const KEY: &str = "platform";

/// A platform catalog in a serverless database.
#[derive(Clone)]
pub struct ServerlessStore {
    db: Arc<dyn AppDb>,
    url: String,
}

impl ServerlessStore {
    /// Open a platform in the database at `url`.
    pub fn new(db: Arc<dyn AppDb>, url: impl Into<String>) -> Self {
        ServerlessStore {
            db,
            url: url.into(),
        }
    }

    /// The address of the database, for the command line to print.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The database the state is kept in.
    ///
    /// The platform's state is one row in it, and the app databases are
    /// separate: a caller that wants to look at the row — a test, or an
    /// operator's script — needs a way in.
    pub fn db(&self) -> Arc<dyn AppDb> {
        Arc::clone(&self.db)
    }

    /// The row that holds the platform.
    fn row(platform: &Platform) -> Row {
        let mut row = Object::new();
        row.set("id", Value::str(KEY));
        row.set("document", Value::str(platform.to_json()));
        Value::Object(row)
    }
}

impl PlatformStore for ServerlessStore {
    fn load(&self) -> BoxFuture<'_, Result<Platform>> {
        Box::pin(async move {
            let query = {
                let mut q = Object::new();
                q.set("id", Value::str(KEY));
                Value::Object(q)
            };
            let rows = self.db.find(COLLECTION, &query, None, None)?;
            let Some(row) = rows.first() else {
                // No row yet: a platform that has never been set up. The
                // caller makes the owner and writes it back.
                return Ok(Platform::default());
            };
            let text = row
                .get("document")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::internal("the platform row has no document"))?;
            let value = mosql::parse_json(text)
                .map_err(|e| Error::internal(format!("the platform document is not JSON: {e}")))?;
            Platform::from_value(&value)
        })
    }

    fn save(&self, platform: &Platform) -> BoxFuture<'_, Result<()>> {
        // The future owns a copy of the catalog, so it borrows only
        // `self`, which is what `'_` in the signature promises.
        let platform = platform.clone();
        Box::pin(async move {
            let row = Self::row(&platform);
            let query = {
                let mut q = Object::new();
                q.set("id", Value::str(KEY));
                Value::Object(q)
            };
            let existing = self.db.find_one(COLLECTION, &query)?;
            match existing {
                Some(_) => {
                    let mut changes = Object::new();
                    changes.set("document", Value::str(platform.to_json()));
                    self.db
                        .update(COLLECTION, &query, &Value::Object(changes))?;
                }
                None => {
                    self.db.insert(COLLECTION, row)?;
                }
            }
            Ok(())
        })
    }
}
