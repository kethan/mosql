//! `createSchemalessClient(type, cfg)` — one factory over every backend.
//!
//! The JavaScript factory dispatches on a string and lazily imports a driver.
//! In Rust the driver is a [`SqlExecutor`](crate::executor::SqlExecutor) the
//! caller supplies, so the factory takes one directly and there is nothing to
//! import. [`Client::memory`] is the in-memory backend, which needs no driver
//! at all.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mosql::{Db, Value};
use uflite::db::{Collection, FindOptions, MemoryCollection};

use crate::adapter::SqlAdapter;
use crate::collection::{CollectionOptions, SqlCollection};
use crate::executor::SqlExecutor;

/// Which backend a [`Client`] talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An isolated in-memory store, as `createSchemalessClient('memory')`.
    Memory,
    /// The SQLite dialect.
    Sqlite,
    /// The PostgreSQL dialect.
    Pg,
    /// The MySQL dialect.
    Mysql,
}

/// A collection from any backend, so callers can be written once.
pub enum AnyCollection {
    /// A SQL table.
    Sql(SqlCollection),
    /// An in-memory collection.
    Memory(Arc<MemoryCollection>),
}

impl std::fmt::Debug for AnyCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnyCollection::Sql(c) => f.debug_tuple("Sql").field(c).finish(),
            AnyCollection::Memory(c) => f.debug_tuple("Memory").field(c).finish(),
        }
    }
}

impl Collection for AnyCollection {
    async fn count_documents(&self, query: &Value) -> mosql::Result<i64> {
        match self {
            AnyCollection::Sql(c) => c.count_documents(query).await,
            AnyCollection::Memory(c) => c.count_documents(query).await,
        }
    }

    async fn find(
        &self,
        query: &Value,
        projection: Option<&Value>,
        options: FindOptions,
    ) -> mosql::Result<Vec<Value>> {
        match self {
            AnyCollection::Sql(c) => c.find(query, projection, options).await,
            AnyCollection::Memory(c) => c.find(query, projection, options).await,
        }
    }

    async fn find_one(
        &self,
        query: &Value,
        projection: Option<&Value>,
    ) -> mosql::Result<Option<Value>> {
        match self {
            AnyCollection::Sql(c) => c.find_one(query, projection).await,
            AnyCollection::Memory(c) => c.find_one(query, projection).await,
        }
    }

    async fn insert_one(&self, document: Value) -> mosql::Result<Value> {
        match self {
            AnyCollection::Sql(c) => c.insert_one(document).await,
            AnyCollection::Memory(c) => c.insert_one(document).await,
        }
    }

    async fn insert_many(&self, documents: Vec<Value>) -> mosql::Result<Vec<Value>> {
        match self {
            AnyCollection::Sql(c) => c.insert_many(documents).await,
            AnyCollection::Memory(c) => c.insert_many(documents).await,
        }
    }

    async fn update_one(&self, query: &Value, update: &Value) -> mosql::Result<i64> {
        match self {
            AnyCollection::Sql(c) => c.update_one(query, update).await,
            AnyCollection::Memory(c) => c.update_one(query, update).await,
        }
    }

    async fn update_many(&self, query: &Value, update: &Value) -> mosql::Result<i64> {
        match self {
            AnyCollection::Sql(c) => c.update_many(query, update).await,
            AnyCollection::Memory(c) => c.update_many(query, update).await,
        }
    }

    async fn delete_one(&self, query: &Value) -> mosql::Result<i64> {
        match self {
            AnyCollection::Sql(c) => c.delete_one(query).await,
            AnyCollection::Memory(c) => c.delete_one(query).await,
        }
    }

    async fn delete_many(&self, query: &Value) -> mosql::Result<i64> {
        match self {
            AnyCollection::Sql(c) => c.delete_many(query).await,
            AnyCollection::Memory(c) => c.delete_many(query).await,
        }
    }
}

enum Inner {
    Memory {
        stores: Mutex<HashMap<String, Arc<MemoryCollection>>>,
    },
    Sql {
        adapter: Arc<SqlAdapter>,
        kind: Kind,
    },
}

/// `createSchemalessClient(type, cfg)` — a handle that hands out databases.
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("kind", &self.kind())
            .finish()
    }
}

impl Client {
    /// `createSchemalessClient('memory')` — isolated stores per database name.
    pub fn memory() -> Self {
        Client {
            inner: Arc::new(Inner::Memory {
                stores: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Any SQL dialect, over an executor the caller supplies.
    ///
    /// This is `createSchemalessClient('sql', { executor })`, and it is also how
    /// a driver plugs in: `Client::with_executor(Db::Sqlite, MySqlite)` is what
    /// `Client::sqlite(path)` will be a thin wrapper around.
    pub fn with_executor(db: Db, executor: impl SqlExecutor + 'static) -> Self {
        let kind = match db {
            Db::Sqlite => Kind::Sqlite,
            Db::Pg => Kind::Pg,
            Db::Mysql => Kind::Mysql,
        };
        Client {
            inner: Arc::new(Inner::Sql {
                adapter: Arc::new(SqlAdapter::new(db, executor)),
                kind,
            }),
        }
    }

    /// The backend this client talks to.
    pub fn kind(&self) -> Kind {
        match &*self.inner {
            Inner::Memory { .. } => Kind::Memory,
            Inner::Sql { kind, .. } => *kind,
        }
    }

    /// `client.db(name)` — a database handle. In memory, one isolated store per
    /// name; in SQL, the name is a label and the tables live in one database.
    pub fn db(&self, name: impl Into<String>) -> Database {
        Database {
            name: name.into(),
            inner: Arc::clone(&self.inner),
        }
    }

    /// `close()` — drops every in-memory store. A SQL client owns nothing to
    /// close: the executor belongs to the caller.
    pub fn close(&self) {
        if let Inner::Memory { stores } = &*self.inner {
            stores
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
    }
}

/// `client.db(name)` — hands out collections.
pub struct Database {
    name: String,
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("name", &self.name)
            .finish()
    }
}

impl Database {
    /// The name this database was opened with.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `db(name).collection(col)` with default options.
    pub fn collection(&self, name: impl Into<String>) -> AnyCollection {
        self.collection_with(name, CollectionOptions::default())
    }

    /// `db(name).collection(col, { schema, idColumn, idStrategy, migrateOnUpdate })`.
    pub fn collection_with(
        &self,
        name: impl Into<String>,
        options: CollectionOptions,
    ) -> AnyCollection {
        let name = name.into();
        match &*self.inner {
            Inner::Sql { adapter, .. } => {
                AnyCollection::Sql(SqlCollection::new(name, Arc::clone(adapter), options))
            }
            Inner::Memory { stores } => {
                // One store per (database, collection), so two handles to
                // `db('a').collection('users')` see the same documents while
                // `db('b').collection('users')` stays separate.
                let key = format!("{}.{}", self.name, name);
                let mut stores = stores
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                AnyCollection::Memory(
                    stores
                        .entry(key)
                        .or_insert_with(|| {
                            Arc::new(MemoryCollection::new(
                                format!("{}_{}", self.name, name),
                                Vec::new(),
                            ))
                        })
                        .clone(),
                )
            }
        }
    }
}
