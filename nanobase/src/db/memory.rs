//! The in-process database: rows in memory, gone when the process stops.
//!
//! It is the engine the JS side calls "memory", and it is the one the
//! tests use. The matcher and the updater are `mosql`'s, so a filter means
//! the same thing here as in every other adapter.

use std::sync::Mutex;

use mosql::memory::{CollectionOptions, Database};
use mosql::Value;
use uflite::error::Result;

use crate::db::{AppDb, Row};

/// Rows in memory.
pub struct MemoryDb {
    db: Mutex<Database>,
}

impl Default for MemoryDb {
    fn default() -> Self {
        MemoryDb {
            db: Mutex::new(Database::new("app")),
        }
    }
}

impl MemoryDb {
    pub fn new() -> Self {
        Self::default()
    }

    /// The collection, with the platform's id convention: an `id` field,
    /// filled in when a row arrives without one.
    fn collection<'a>(db: &'a mut Database, name: &str) -> &'a mut mosql::memory::Collection {
        db.collection(
            name,
            None,
            CollectionOptions {
                id_column: Some("id".to_string()),
                id_strategy: Some("mongo".to_string()),
            },
        )
    }
}

impl AppDb for MemoryDb {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn collections(&self) -> Result<Vec<String>> {
        let db = self.db.lock().expect("not poisoned");
        Ok(db.list_collections())
    }

    fn rows(&self, collection: &str) -> Result<Vec<Row>> {
        let mut db = self.db.lock().expect("not poisoned");
        Ok(MemoryDb::collection(&mut db, collection).get_all())
    }

    fn find(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        let mut db = self.db.lock().expect("not poisoned");
        let mut found = MemoryDb::collection(&mut db, collection)
            .find(query)
            .skip(offset.unwrap_or(0) as i64)
            .to_vec()?;
        if let Some(limit) = limit {
            found.truncate(limit as usize);
        }
        Ok(found)
    }

    fn find_one(&self, collection: &str, query: &Value) -> Result<Option<Row>> {
        let mut db = self.db.lock().expect("not poisoned");
        Ok(MemoryDb::collection(&mut db, collection).find_one(query, None)?)
    }

    fn count(&self, collection: &str, query: &Value) -> Result<u64> {
        let mut db = self.db.lock().expect("not poisoned");
        Ok(MemoryDb::collection(&mut db, collection).count_documents(query)? as u64)
    }

    fn insert(&self, collection: &str, row: Row) -> Result<Row> {
        let mut db = self.db.lock().expect("not poisoned");
        let mut row = row;
        if row.get("id").is_none_or(|v| v.is_null()) {
            let id = crate::crypto::random_id();
            mosql::memory::set_path(&mut row, "id", Value::str(id));
        }
        let id = row.get("id").cloned().unwrap_or(Value::Null);
        MemoryDb::collection(&mut db, collection).insert_one(row)?;
        // The collection gives the id back; find the row to return it whole.
        let mut filter = mosql::Object::new();
        filter.set("id", id);
        Ok(MemoryDb::collection(&mut db, collection)
            .find_one(&Value::Object(filter), None)?
            .unwrap_or(Value::Null))
    }

    fn update(&self, collection: &str, query: &Value, changes: &Value) -> Result<Option<Row>> {
        let mut db = self.db.lock().expect("not poisoned");
        let current = MemoryDb::collection(&mut db, collection).find_one(query, None)?;
        let Some(current) = current else {
            return Ok(None);
        };
        let id = current.get("id").cloned().unwrap_or(Value::Null);
        let result = MemoryDb::collection(&mut db, collection).update_one(query, changes, false)?;
        if result.matched_count == 0 {
            return Ok(None);
        }
        let mut filter = mosql::Object::new();
        filter.set("id", id);
        Ok(MemoryDb::collection(&mut db, collection).find_one(&Value::Object(filter), None)?)
    }

    fn remove(&self, collection: &str, query: &Value) -> Result<u64> {
        let mut db = self.db.lock().expect("not poisoned");
        let result = MemoryDb::collection(&mut db, collection).delete_many(query)?;
        Ok(result.deleted_count as u64)
    }
}
