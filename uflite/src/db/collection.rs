//! The storage contract behind [`DbService`](super::DbService).
//!
//! db.js in uflite is written against the subset of the Mongo driver API that
//! umosql collections implement: `countDocuments`, `find(...).toArray()`,
//! `findOne`, `insertOne/Many`, `updateOne/Many`, `deleteOne/Many`. That is
//! exactly this trait. The memory engine implements it in
//! [`MemoryCollection`](super::MemoryCollection); a SQL adapter implements it
//! over rusqlite, sqlx or tokio-postgres.
//!
//! Methods are declared with explicit `impl Future + Send` so the futures are
//! usable from `tokio::spawn`; implementations may use plain `async fn`.

use core::future::Future;

use mosql::{Result as QueryResult, Value};

/// `find(query, projection, options)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FindOptions {
    /// Records to skip first. `0` skips nothing.
    pub skip: i64,
    /// Maximum records to return. `None` returns everything; `Some(0)` returns
    /// nothing, like `slice(0, 0)`.
    pub limit: Option<i64>,
    /// Mongo-style sort document, e.g. `{ "age": -1 }`.
    pub sort: Option<Value>,
}

/// A document collection. Every method takes `&self`: adapters are shared
/// across tasks and own whatever synchronisation they need.
pub trait Collection: Send + Sync {
    /// `countDocuments(query)` — how many documents match.
    fn count_documents<'a>(
        &'a self,
        query: &'a Value,
    ) -> impl Future<Output = QueryResult<i64>> + Send + 'a;

    /// `find(query, projection, options).toArray()`.
    fn find<'a>(
        &'a self,
        query: &'a Value,
        projection: Option<&'a Value>,
        options: FindOptions,
    ) -> impl Future<Output = QueryResult<Vec<Value>>> + Send + 'a;

    /// `findOne(query, projection)`.
    fn find_one<'a>(
        &'a self,
        query: &'a Value,
        projection: Option<&'a Value>,
    ) -> impl Future<Output = QueryResult<Option<Value>>> + Send + 'a;

    /// `insertOne(doc)` — returns the inserted id.
    fn insert_one<'a>(&'a self, doc: Value)
        -> impl Future<Output = QueryResult<Value>> + Send + 'a;

    /// `insertMany(docs)` — returns the inserted ids in order.
    fn insert_many<'a>(
        &'a self,
        docs: Vec<Value>,
    ) -> impl Future<Output = QueryResult<Vec<Value>>> + Send + 'a;

    /// `updateOne(filter, update)` — returns the number of matched documents.
    fn update_one<'a>(
        &'a self,
        filter: &'a Value,
        update: &'a Value,
    ) -> impl Future<Output = QueryResult<i64>> + Send + 'a;

    /// `updateMany(filter, update)` — returns the number of matched documents.
    fn update_many<'a>(
        &'a self,
        filter: &'a Value,
        update: &'a Value,
    ) -> impl Future<Output = QueryResult<i64>> + Send + 'a;

    /// `deleteOne(filter)` — returns the number of deleted documents.
    fn delete_one<'a>(
        &'a self,
        filter: &'a Value,
    ) -> impl Future<Output = QueryResult<i64>> + Send + 'a;

    /// `deleteMany(filter)` — returns the number of deleted documents.
    fn delete_many<'a>(
        &'a self,
        filter: &'a Value,
    ) -> impl Future<Output = QueryResult<i64>> + Send + 'a;
}
