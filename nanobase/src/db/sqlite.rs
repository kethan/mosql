//! The local file: one SQLite table per collection.
//!
//! A collection is a table with two columns: `id`, the row's identifier, and
//! `data`, the row itself as JSON. The document stays a document — the
//! filter is `mosql`'s matcher over the rows the table holds — so a query
//! means the same thing here as in every other adapter, and a row can hold
//! a field no column was declared for.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mosql::Value;
use uflite::error::{Error, Result};

use crate::db::{apply_changes, sanitize, AppDb, Row};
use crate::sqlite::Connection;

/// Rows in a local SQLite file.
pub struct SqliteDb {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteDb {
    /// Open the file, creating it if it is not there.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| {
                    Error::internal(format!("cannot create {}: {e}", dir.display()))
                })?;
            }
        }
        let conn = Connection::open(&path)?;
        Ok(SqliteDb {
            path,
            conn: Mutex::new(conn),
        })
    }

    /// A database that lives only in memory.
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_memory()?;
        Ok(SqliteDb {
            path: PathBuf::from(":memory:"),
            conn: Mutex::new(conn),
        })
    }

    /// The file the rows are in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The table name for a collection. A collection name is a name an app
    /// chose, so it is quoted rather than trusted.
    fn table(collection: &str) -> String {
        format!("c_{}", sanitize(collection))
    }

    /// Make sure the table for a collection exists.
    fn ensure(conn: &Connection, collection: &str) -> Result<()> {
        let table = SqliteDb::table(collection);
        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS {table} (id TEXT PRIMARY KEY, data TEXT NOT NULL)"
        ))
    }

    /// Every row of a collection, as documents.
    fn all(conn: &Connection, collection: &str) -> Result<Vec<Row>> {
        let table = SqliteDb::table(collection);
        let rows = conn.query(&format!("SELECT data FROM {table} ORDER BY rowid"), &[])?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(text) = row.get("data").and_then(|v| v.as_str()) {
                match mosql::parse_json(text) {
                    Ok(value) => out.push(value),
                    Err(e) => {
                        return Err(Error::internal(format!(
                            "a row of {collection} is not readable: {e}"
                        )))
                    }
                }
            }
        }
        Ok(out)
    }

    /// The rows matching a filter, over a connection already held.
    fn find_in(
        conn: &Connection,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        SqliteDb::ensure(conn, collection)?;
        let all = SqliteDb::all(conn, collection)?;
        let mut found: Vec<Row> = Vec::with_capacity(all.len());
        for row in all {
            if mosql::memory::matches(query, &row)? {
                found.push(row);
            }
        }
        let offset = offset.unwrap_or(0) as usize;
        found = found.into_iter().skip(offset).collect();
        if let Some(limit) = limit {
            found.truncate(limit as usize);
        }
        Ok(found)
    }
}

impl AppDb for SqliteDb {
    fn kind(&self) -> &'static str {
        "sqlite"
    }

    fn collections(&self) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("not poisoned");
        let rows = conn.query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name LIKE 'c\\_%' ESCAPE '\\' ORDER BY name",
            &[],
        )?;
        Ok(rows
            .iter()
            .filter_map(|r| r.get("name").and_then(|v| v.as_str()))
            .map(|name| name.trim_start_matches("c_").to_string())
            .collect())
    }

    fn rows(&self, collection: &str) -> Result<Vec<Row>> {
        let conn = self.conn.lock().expect("not poisoned");
        SqliteDb::ensure(&conn, collection)?;
        SqliteDb::all(&conn, collection)
    }

    fn find(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        let conn = self.conn.lock().expect("not poisoned");
        SqliteDb::find_in(&conn, collection, query, limit, offset)
    }

    fn find_one(&self, collection: &str, query: &Value) -> Result<Option<Row>> {
        let conn = self.conn.lock().expect("not poisoned");
        Ok(SqliteDb::find_in(&conn, collection, query, Some(1), None)?
            .into_iter()
            .next())
    }

    fn count(&self, collection: &str, query: &Value) -> Result<u64> {
        let conn = self.conn.lock().expect("not poisoned");
        Ok(SqliteDb::find_in(&conn, collection, query, None, None)?.len() as u64)
    }

    fn insert(&self, collection: &str, row: Row) -> Result<Row> {
        let conn = self.conn.lock().expect("not poisoned");
        SqliteDb::ensure(&conn, collection)?;
        let mut row = row;
        if row.get("id").is_none_or(|v| v.is_null()) {
            let id = crate::crypto::random_id();
            mosql::memory::set_path(&mut row, "id", Value::str(id));
        }
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("a row needs an id"))?
            .to_string();
        let table = SqliteDb::table(collection);
        conn.execute(
            &format!("INSERT INTO {table} (id, data) VALUES (?, ?)"),
            &[Value::str(id), Value::str(mosql::to_json(&row))],
        )?;
        Ok(row)
    }

    fn update(&self, collection: &str, query: &Value, changes: &Value) -> Result<Option<Row>> {
        let conn = self.conn.lock().expect("not poisoned");
        SqliteDb::ensure(&conn, collection)?;
        let mut current = SqliteDb::find_in(&conn, collection, query, Some(1), None)?
            .into_iter()
            .next();
        let Some(mut row) = current.take() else {
            return Ok(None);
        };
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("a row needs an id"))?
            .to_string();
        apply_changes(&mut row, changes)?;
        let table = SqliteDb::table(collection);
        conn.execute(
            &format!("UPDATE {table} SET data = ? WHERE id = ?"),
            &[Value::str(mosql::to_json(&row)), Value::str(id)],
        )?;
        Ok(Some(row))
    }

    fn remove(&self, collection: &str, query: &Value) -> Result<u64> {
        let conn = self.conn.lock().expect("not poisoned");
        SqliteDb::ensure(&conn, collection)?;
        let matching = SqliteDb::find_in(&conn, collection, query, None, None)?;
        let table = SqliteDb::table(collection);
        for row in &matching {
            if let Some(id) = row.get("id").and_then(|v| v.as_str()) {
                conn.execute(
                    &format!("DELETE FROM {table} WHERE id = ?"),
                    &[Value::str(id)],
                )?;
            }
        }
        Ok(matching.len() as u64)
    }
}
