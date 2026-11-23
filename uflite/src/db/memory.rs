//! [`Collection`] over the `mosql` in-memory engine.
//!
//! The engine is synchronous and takes `&mut self` for writes, so the adapter
//! owns it behind a `Mutex`. No lock is ever held across an `.await` — the
//! futures here never actually yield — so they are `Send` and cheap.

use std::sync::{Mutex, MutexGuard, PoisonError};

use mosql::memory::Collection as Engine;
use mosql::Value;

use super::collection::{Collection, FindOptions};
use mosql::Result as QueryResult;

/// A `mosql::memory::Collection` exposed as an async [`Collection`].
pub struct MemoryCollection {
    name: String,
    engine: Mutex<Engine>,
}

impl core::fmt::Debug for MemoryCollection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryCollection")
            .field("name", &self.name)
            .field("len", &self.len())
            .finish()
    }
}

impl MemoryCollection {
    /// A named collection seeded with `docs`. Documents without an id get one
    /// from the engine on insert, exactly like umosql/memory.
    pub fn new(name: impl Into<String>, docs: Vec<Value>) -> Self {
        let name = name.into();
        let engine = Engine::new(&name, docs, Default::default());
        MemoryCollection {
            name,
            engine: Mutex::new(engine),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of documents right now.
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// A snapshot of every document.
    pub fn snapshot(&self) -> Vec<Value> {
        self.lock().get_all()
    }

    /// Replace the engine's contents. Useful between tests.
    pub fn replace(&self, docs: Vec<Value>) {
        let mut g = self.lock();
        g.clear();
        for d in docs {
            let _ = g.insert_one(d);
        }
    }

    fn lock(&self) -> MutexGuard<'_, Engine> {
        // A panic while holding the lock leaves the engine in an unknown
        // state only if it panicked mid-write; the memory engine does not
        // panic on user input, so recovering the guard is the better trade
        // than poisoning every later call.
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Collection for MemoryCollection {
    async fn count_documents(&self, query: &Value) -> QueryResult<i64> {
        self.lock().count_documents(query)
    }

    async fn find(
        &self,
        query: &Value,
        projection: Option<&Value>,
        options: FindOptions,
    ) -> QueryResult<Vec<Value>> {
        let g = self.lock();
        // Same order as the JS cursor: filter → sort → skip → limit → project.
        let mut q = g.find(query);
        if let Some(p) = projection {
            q = q.select(p);
        }
        if let Some(s) = &options.sort {
            q = q.sort(s);
        }
        q = q.skip(options.skip);
        if let Some(l) = options.limit {
            q = q.limit(l);
        }
        q.to_vec()
    }

    async fn find_one(
        &self,
        query: &Value,
        projection: Option<&Value>,
    ) -> QueryResult<Option<Value>> {
        self.lock().find_one(query, projection)
    }

    async fn insert_one(&self, doc: Value) -> QueryResult<Value> {
        self.lock().insert_one(doc)
    }

    async fn insert_many(&self, docs: Vec<Value>) -> QueryResult<Vec<Value>> {
        self.lock().insert_many(&docs)
    }

    async fn update_one(&self, filter: &Value, update: &Value) -> QueryResult<i64> {
        Ok(self.lock().update_one(filter, update, false)?.matched_count)
    }

    async fn update_many(&self, filter: &Value, update: &Value) -> QueryResult<i64> {
        Ok(self.lock().update_many(filter, update)?.matched_count)
    }

    async fn delete_one(&self, filter: &Value) -> QueryResult<i64> {
        Ok(self.lock().delete_one(filter)?.deleted_count)
    }

    async fn delete_many(&self, filter: &Value) -> QueryResult<i64> {
        Ok(self.lock().delete_many(filter)?.deleted_count)
    }
}
