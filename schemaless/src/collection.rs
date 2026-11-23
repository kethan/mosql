//! `SQLCollection` — a Mongo-shaped collection over a SQL table that creates
//! and migrates its own schema as documents arrive.
//!
//! This is the port of `schemaless.js`. The behaviour that matters is the
//! on-write DDL: the first document creates the table, a document with a new
//! field adds the column, a wider value widens the column, and a statement that
//! fails because the table or a column does not exist yet is retried after the
//! DDL runs. Nothing is declared up front.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use mosql::{Builder, CrudOptions, Db, Object, Value};
use uflite::db::{Collection, FindOptions};

use crate::adapter::{quote_ident, SqlAdapter};
use crate::executor::{ExecResult, Missing, SqlError};
use crate::infer::{base_type, escape_value, infer_sql_type, widen_type};

/// How the id column is produced.
#[derive(Clone)]
pub enum IdStrategy {
    /// `SERIAL` / `AUTO_INCREMENT` / `INTEGER PRIMARY KEY AUTOINCREMENT`.
    Auto,
    /// A 24-character hex id, like MongoDB's ObjectId.
    Mongo,
    /// A caller-supplied generator.
    Custom(Arc<dyn Fn() -> Value + Send + Sync>),
}

impl std::fmt::Debug for IdStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IdStrategy::Auto => f.write_str("Auto"),
            IdStrategy::Mongo => f.write_str("Mongo"),
            IdStrategy::Custom(_) => f.write_str("Custom"),
        }
    }
}

/// `new SQLCollection(name, adapter, config)`.
#[derive(Clone, Debug)]
pub struct CollectionOptions {
    /// `{ field: "TYPE" }` or `{ field: { type, required, unique, default, hidden } }`.
    pub schema: Value,
    /// The id column. `_id`, like the JavaScript default.
    pub id_column: String,
    /// How ids are produced.
    pub id_strategy: IdStrategy,
    /// Whether an update may add the columns it writes. Default `true`.
    pub migrate_on_update: bool,
}

impl Default for CollectionOptions {
    fn default() -> Self {
        CollectionOptions {
            schema: Value::object(),
            id_column: "_id".to_string(),
            id_strategy: IdStrategy::Auto,
            migrate_on_update: true,
        }
    }
}

/// A schema-less collection over one SQL table.
pub struct SqlCollection {
    name: String,
    adapter: Arc<SqlAdapter>,
    options: CollectionOptions,
    /// The schema cache. Only read and written between `await` points, so a
    /// `std` mutex is safe here and the futures stay `Send`.
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    initialized: bool,
    table_schema: HashMap<String, String>,
}

impl std::fmt::Debug for SqlCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqlCollection")
            .field("name", &self.name)
            .field("db", &self.adapter.db())
            .field("id_column", &self.options.id_column)
            .finish()
    }
}

/// What [`SqlCollection::upsert_one`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Upserted {
    /// `true` when nothing matched and a document was inserted.
    pub upserted: bool,
    /// The new document's id, when one was inserted.
    pub inserted_id: Option<Value>,
}

impl SqlCollection {
    /// A collection on `adapter`.
    pub fn new(
        name: impl Into<String>,
        adapter: Arc<SqlAdapter>,
        options: CollectionOptions,
    ) -> Self {
        SqlCollection {
            name: name.into(),
            adapter,
            options,
            state: Mutex::new(State::default()),
        }
    }

    /// The table name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The dialect.
    pub fn db(&self) -> Db {
        self.adapter.db()
    }

    /// The options this collection was built with.
    pub fn options(&self) -> &CollectionOptions {
        &self.options
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic in one request must not poison the collection for the rest.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `initialize()` — create the table when a schema was declared, then
    /// cache the columns the database actually has.
    pub async fn initialize(&self) -> Result<(), SqlError> {
        if self.lock().initialized {
            return Ok(());
        }
        let has_schema = self
            .options
            .schema
            .as_object()
            .is_some_and(|schema| !schema.is_empty());
        if !self.adapter.table_exists(&self.name).await && has_schema {
            self.create_table(&Value::object()).await?;
        }
        let schema = self.adapter.table_schema(&self.name).await?;
        let mut state = self.lock();
        state.table_schema = schema;
        state.initialized = true;
        Ok(())
    }

    /// `createTable(document)` — the id column, the timestamps, and one
    /// `ALTER TABLE` per column (the table is created with only the id and the
    /// timestamps, exactly as the JavaScript does).
    pub async fn create_table(&self, document: &Value) -> Result<(), SqlError> {
        let db = self.adapter.db();

        let mut keys: Vec<String> = Vec::new();
        for source in [&self.options.schema, document] {
            if let Some(obj) = source.as_object() {
                for (key, _) in obj.iter() {
                    if key != "_id" && !keys.contains(key) {
                        keys.push(key.clone());
                    }
                }
            }
        }

        let mut columns = Vec::with_capacity(keys.len());
        for key in &keys {
            let def = self.options.schema.get(key);
            let kind = self.schema_type(def).unwrap_or_else(|| {
                infer_sql_type(document.get(key).unwrap_or(&Value::Null), db).to_string()
            });
            let nullable = if is_flag(def, "required") {
                " NOT NULL"
            } else {
                ""
            };
            let unique = if is_flag(def, "unique") {
                " UNIQUE"
            } else {
                ""
            };
            let default = match def.and_then(|d| d.get("default")) {
                Some(v) => format!(" DEFAULT {}", escape_value(v, db)),
                None => String::new(),
            };
            columns.push(format!(
                "{} {kind}{nullable}{unique}{default}",
                quote_ident(key, db)
            ));
        }

        let id = &self.options.id_column;
        let id_column = if matches!(self.options.id_strategy, IdStrategy::Auto) {
            match db {
                Db::Pg => format!("{id} SERIAL PRIMARY KEY"),
                Db::Mysql => format!("{} INT AUTO_INCREMENT PRIMARY KEY", quote_ident(id, db)),
                Db::Sqlite => format!("{id} INTEGER PRIMARY KEY AUTOINCREMENT"),
            }
        } else {
            let kind = match db {
                Db::Pg => "TEXT",
                Db::Mysql => "VARCHAR(24)",
                Db::Sqlite => "TEXT",
            };
            format!("{} {kind} PRIMARY KEY", quote_ident(id, db))
        };
        let timestamps = match db {
            Db::Pg => "created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP",
            Db::Mysql => "created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP",
            Db::Sqlite => "created_at TEXT DEFAULT (datetime('now')), updated_at TEXT DEFAULT (datetime('now'))",
        };

        let table = quote_ident(&self.name, db);
        self.adapter
            .execute(&format!(
                "CREATE TABLE IF NOT EXISTS {table} ({id_column}, {timestamps})"
            ))
            .await?;
        for column in columns {
            // A column that already exists is not an error: the table may have
            // been created by another request between the probe and the DDL.
            let _ = self
                .adapter
                .execute(&format!("ALTER TABLE {table} ADD COLUMN {column}"))
                .await;
        }
        Ok(())
    }

    /// `migrate(document)` — add the columns this document has and the table
    /// does not, widening types where the dialect allows it.
    pub async fn migrate(&self, document: &Value) -> Result<(), SqlError> {
        let db = self.adapter.db();
        let current = self.adapter.table_schema(&self.name).await?;
        let mut migrations: Vec<String> = Vec::new();

        if let Some(obj) = document.as_object() {
            for (key, value) in obj.iter() {
                if key == "_id" {
                    continue;
                }
                let def = self.options.schema.get(key);
                let kind = self
                    .schema_type(def)
                    .unwrap_or_else(|| infer_sql_type(value, db).to_string());
                match current.get(key.as_str()) {
                    None => {
                        let nullable = if is_flag(def, "required") {
                            " NOT NULL"
                        } else {
                            ""
                        };
                        let unique = if is_flag(def, "unique") {
                            " UNIQUE"
                        } else {
                            ""
                        };
                        let default = match def.and_then(|d| d.get("default")) {
                            Some(v) => format!(" DEFAULT {}", escape_value(v, db)),
                            None => String::new(),
                        };
                        migrations.push(format!(
                            "ALTER TABLE {} ADD COLUMN {} {kind}{nullable}{unique}{default}",
                            quote_ident(&self.name, db),
                            quote_ident(key, db)
                        ));
                        self.lock().table_schema.insert(key.clone(), kind);
                    }
                    Some(current_type) => {
                        let widened = widen_type(current_type, &kind);
                        if base_type(&widened) != base_type(current_type) && db != Db::Sqlite {
                            migrations.push(match db {
                                Db::Pg => format!(
                                    "ALTER TABLE {} ALTER COLUMN {} TYPE {widened}",
                                    self.name, key
                                ),
                                Db::Mysql => format!(
                                    "ALTER TABLE {} MODIFY COLUMN {} {widened}",
                                    quote_ident(&self.name, db),
                                    quote_ident(key, db)
                                ),
                                Db::Sqlite => unreachable!("SQLite is excluded above"),
                            });
                            self.lock().table_schema.insert(key.clone(), widened);
                        }
                    }
                }
            }
        }

        for sql in &migrations {
            if let Err(e) = self.adapter.execute(sql).await {
                let msg = e.message.to_lowercase();
                let ignorable = msg.contains("already exists")
                    || msg.contains("duplicate")
                    || msg.contains("exists");
                if !ignorable {
                    return Err(e);
                }
            }
        }
        // The JavaScript adapter replaces its cache with the snapshot it read
        // before migrating, so the additions above are dropped and the next
        // statement may repeat an `ALTER` (which fails and is ignored). Kept,
        // because it is observable in the SQL log.
        self.lock().table_schema = current;
        Ok(())
    }

    /// The columns the database currently has for this table.
    pub async fn table_schema(&self) -> Result<HashMap<String, String>, SqlError> {
        self.adapter.table_schema(&self.name).await
    }

    /// `classifyError` + retry: create the table or add the column, then run
    /// the statement again.
    async fn exec_with_ddl_retry<'a>(
        &self,
        sql: &str,
        ddl: Ddl<'a>,
    ) -> Result<ExecResult, SqlError> {
        match self.adapter.execute(sql).await {
            Ok(result) => Ok(result),
            Err(e) => match e.classify() {
                Missing::Table => {
                    self.create_table(ddl.doc_for_create.unwrap_or(&Value::Null))
                        .await?;
                    self.adapter.execute(sql).await
                }
                Missing::Column if ddl.allow_column_migrate => {
                    self.migrate(ddl.doc_for_migrate.unwrap_or(&Value::Null))
                        .await?;
                    self.adapter.execute(sql).await
                }
                _ => Err(e),
            },
        }
    }

    /// `addMissingRenameColumns(update)` — `$rename: { old: new }` needs the
    /// new column to exist before the update can set it.
    async fn add_missing_rename_columns(&self, update: &Value) -> Result<(), SqlError> {
        if !self.options.migrate_on_update {
            return Ok(());
        }
        let Some(renames) = update.get("$rename").and_then(Value::as_object) else {
            return Ok(());
        };
        let db = self.adapter.db();
        let fallback = match db {
            Db::Pg => "TEXT",
            Db::Mysql => "VARCHAR(255)",
            Db::Sqlite => "TEXT",
        };
        for (old, new) in renames.iter() {
            let Some(new) = new.as_str() else { continue };
            if old.contains('.') || new.contains('.') || old == new {
                continue;
            }
            if self.lock().table_schema.contains_key(new) {
                continue;
            }
            let mut kind = self
                .lock()
                .table_schema
                .get(old.as_str())
                .cloned()
                .unwrap_or_else(|| fallback.to_string());
            if db == Db::Mysql && is_bare_char_type(&kind) {
                kind = fallback.to_string();
            }
            let sql = format!(
                "ALTER TABLE {} ADD COLUMN {} {kind}",
                quote_ident(&self.name, db),
                quote_ident(new, db)
            );
            // The JavaScript only warns here; a failure is not fatal.
            if self.adapter.execute(&sql).await.is_ok() {
                self.lock().table_schema.insert(new.to_string(), kind);
            }
        }
        Ok(())
    }

    /// `isAutoIdColumn` — a declared `SERIAL`/`AUTO_INCREMENT` column cannot
    /// take a null, or the sequence default never applies.
    fn is_auto_id_column(&self, column: &str) -> bool {
        let kind = self
            .schema_type(self.options.schema.get(column))
            .unwrap_or_default()
            .to_uppercase();
        match self.adapter.db() {
            Db::Pg => kind.contains("SERIAL"),
            Db::Mysql => kind.contains("AUTO_INCREMENT"),
            Db::Sqlite => false,
        }
    }

    /// `stripAutoIncrements(doc)`.
    fn strip_auto_increments(&self, doc: &Value) -> Value {
        let mut out = doc.clone();
        let Value::Object(obj) = &mut out else {
            return out;
        };
        let keys: Vec<String> = obj.keys().map(str::to_string).collect();
        for key in keys {
            let is_auto_id = key == self.options.id_column
                && matches!(self.options.id_strategy, IdStrategy::Auto);
            if !is_auto_id && !self.is_auto_id_column(&key) {
                continue;
            }
            if obj.get(&key).is_none_or(Value::is_null) {
                obj.remove(&key);
            }
        }
        out
    }

    /// `applyDefaults(document)`.
    fn apply_defaults(&self, document: &Value) -> Value {
        let mut out = document.clone();
        let Some(schema) = self.options.schema.as_object() else {
            return out;
        };
        let Value::Object(obj) = &mut out else {
            return out;
        };
        for (key, def) in schema.iter() {
            if obj.get(key).is_some() {
                continue;
            }
            if let Some(default) = def.get("default") {
                obj.set(key.clone(), default.clone());
            }
        }
        out
    }

    /// `filterHidden(document)` — drop fields marked `hidden: true`.
    fn filter_hidden(&self, document: Value) -> Value {
        let mut out = document;
        let Some(schema) = self.options.schema.as_object() else {
            return out;
        };
        let Value::Object(obj) = &mut out else {
            return out;
        };
        for (key, def) in schema.iter() {
            if is_flag(Some(def), "hidden") {
                obj.remove(key);
            }
        }
        out
    }

    /// `generateId()`.
    fn generate_id(&self) -> Value {
        match &self.options.id_strategy {
            IdStrategy::Custom(f) => f(),
            IdStrategy::Mongo => Value::str(mongo_id()),
            IdStrategy::Auto => Value::Null,
        }
    }

    /// The declared type of a field, if the schema gives one.
    fn schema_type(&self, def: Option<&Value>) -> Option<String> {
        let def = def?;
        match def {
            Value::Str(s) => Some(s.clone()),
            Value::Object(_) => def.get("type").and_then(Value::as_str).map(str::to_string),
            _ => None,
        }
    }

    /// The document an update would write, used to migrate new columns.
    fn update_document(&self, update: &Value) -> Value {
        let mut out = Value::object();
        let Value::Object(dst) = &mut out else {
            return out;
        };
        if let Some(set) = update.get("$set").and_then(Value::as_object) {
            for (key, value) in set.iter() {
                dst.set(key.clone(), value.clone());
            }
        }
        if let Some(dates) = update.get("$currentDate").and_then(Value::as_object) {
            for (key, _) in dates.iter() {
                dst.set(key.clone(), Value::Date(now_iso()));
            }
        }
        if let Some(obj) = update.as_object() {
            for (key, value) in obj.iter() {
                if !key.starts_with('$') {
                    dst.set(key.clone(), value.clone());
                }
            }
        }
        out
    }

    /// One update, returning `(matched, modified)`.
    async fn update(
        &self,
        query: &Value,
        update: &Value,
        one: bool,
    ) -> Result<(i64, i64), SqlError> {
        self.initialize().await?;
        self.add_missing_rename_columns(update).await?;
        // Both statements are built before the first await: the builder holds
        // its operator tables in `Rc`s, so it must not be alive across one or
        // the future stops being `Send`.
        let (count_sql, sql) = {
            let collection = Builder::full().collection(&self.name, self.adapter.db())?;
            let options = CrudOptions {
                returning: None,
                allow_delete_all: false,
            };
            let write = if one {
                collection.update_one(query, update, &options)?
            } else {
                collection.update_many(query, update, &options)?
            };
            (collection.count_documents(query)?, write)
        };
        let pre = self.exec_with_ddl_retry(&count_sql, Ddl::default()).await?;
        let matched = count_from(&pre.rows);
        let matched = if one { matched.min(1) } else { matched };
        let document = self.update_document(update);
        let result = self
            .exec_with_ddl_retry(
                &sql,
                Ddl {
                    doc_for_migrate: Some(&document),
                    allow_column_migrate: self.options.migrate_on_update,
                    ..Ddl::default()
                },
            )
            .await?;
        Ok((matched, result.affected))
    }

    /// `upsertOne(query, update, insertDoc)` — insert when nothing matched.
    pub async fn upsert_one(
        &self,
        query: &Value,
        update: &Value,
        insert_doc: Value,
    ) -> Result<Upserted, SqlError> {
        let (_, modified) = self.update(query, update, true).await?;
        if modified != 0 {
            return Ok(Upserted {
                upserted: false,
                inserted_id: None,
            });
        }
        let mut base = Value::object();
        if let (Some(obj), Value::Object(dst)) = (query.as_object(), &mut base) {
            for (key, value) in obj.iter() {
                let eq = match value {
                    Value::Object(_) => value.get("$eq").cloned(),
                    Value::Null => Some(Value::Null),
                    Value::Array(_) => None,
                    other => Some(other.clone()),
                };
                if let Some(eq) = eq {
                    dst.set(key.clone(), eq);
                }
            }
        }
        merge(&mut base, &insert_doc);
        if let Some(set) = update.get("$set") {
            merge(&mut base, set);
        }
        let inserted_id = self.insert_one(base).await?;
        Ok(Upserted {
            upserted: true,
            inserted_id: Some(inserted_id),
        })
    }

    /// `aggregate(pipeline)`.
    pub async fn aggregate(&self, pipeline: &Value) -> Result<Vec<Value>, SqlError> {
        self.initialize().await?;
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .aggregate(pipeline)?;
        // (built above, before the await)
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result
            .rows
            .into_iter()
            .map(|row| self.filter_hidden(row))
            .collect())
    }

    /// `distinct(field, query)`.
    pub async fn distinct(&self, field: &str, query: &Value) -> Result<Vec<Value>, SqlError> {
        self.initialize().await?;
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .distinct(field, query)?;
        // (built above, before the await)
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result
            .rows
            .iter()
            .filter_map(|row| {
                row.as_object()
                    .and_then(Object::first)
                    .map(|(_, v)| v.clone())
            })
            .collect())
    }

    /// `createIndex(field, options)` — a plain index per dialect. `index_type`
    /// is Postgres' `USING` clause (`GIN`, `GIST`, …).
    pub async fn create_index(
        &self,
        field: &str,
        unique: bool,
        index_type: Option<&str>,
    ) -> Result<(), SqlError> {
        let db = self.adapter.db();
        let name = format!("idx_{}_{}", self.name, field.replace('.', "_"));
        let unique = if unique { "UNIQUE" } else { "" };
        if db == Db::Mysql {
            let existing = format!(
                "SHOW INDEX FROM {} WHERE Key_name = '{name}'",
                quote_ident(&self.name, db)
            );
            if self.adapter.execute(&existing).await?.rows.is_empty() {
                self.adapter
                    .execute(&format!(
                        "CREATE {unique} INDEX {} ON {} ({})",
                        quote_ident(&name, db),
                        quote_ident(&self.name, db),
                        quote_ident(field, db)
                    ))
                    .await?;
            }
        } else {
            let using = match (db, index_type) {
                (Db::Pg, Some(kind)) => format!("USING {}", kind.to_uppercase()),
                _ => String::new(),
            };
            self.adapter
                .execute(
                    format!(
                        "CREATE {unique} INDEX IF NOT EXISTS {name} ON {} {using} ({field})",
                        self.name
                    )
                    .trim(),
                )
                .await?;
        }
        Ok(())
    }

    /// `dropColumn(column)` — SQLite cannot drop a column.
    pub async fn drop_column(&self, column: &str) -> Result<(), SqlError> {
        if self.adapter.db() == Db::Sqlite {
            return Err(SqlError::new("SQLite does not support DROP COLUMN"));
        }
        let db = self.adapter.db();
        self.adapter
            .execute(&format!(
                "ALTER TABLE {} DROP COLUMN {}",
                quote_ident(&self.name, db),
                quote_ident(column, db)
            ))
            .await
            .map(|_| ())
    }
}

/// What a retried statement may do to recover.
#[derive(Default)]
struct Ddl<'a> {
    doc_for_create: Option<&'a Value>,
    doc_for_migrate: Option<&'a Value>,
    allow_column_migrate: bool,
}

impl Collection for SqlCollection {
    async fn count_documents(&self, query: &Value) -> mosql::Result<i64> {
        self.initialize().await?;
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .count_documents(query)?;
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(count_from(&result.rows))
    }

    async fn find(
        &self,
        query: &Value,
        projection: Option<&Value>,
        options: FindOptions,
    ) -> mosql::Result<Vec<Value>> {
        self.initialize().await?;
        let sql = {
            let collection = Builder::full().collection(&self.name, self.adapter.db())?;
            let mut find = collection.find(query, projection)?;
            if let Some(sort) = &options.sort {
                find = find.sort(sort)?;
            }
            if options.skip != 0 {
                find = find.skip(&Value::Int(options.skip))?;
            }
            if let Some(limit) = options.limit {
                find = find.limit(&Value::Int(limit))?;
            }
            find.to_sql()?
        };
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result
            .rows
            .into_iter()
            .map(|row| self.filter_hidden(row))
            .collect())
    }

    async fn find_one(
        &self,
        query: &Value,
        projection: Option<&Value>,
    ) -> mosql::Result<Option<Value>> {
        self.initialize().await?;
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .find_one(query, projection)?
            .to_sql()?;
        // (built above, before the await)
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result
            .rows
            .into_iter()
            .next()
            .map(|row| self.filter_hidden(row)))
    }

    async fn insert_one(&self, document: Value) -> mosql::Result<Value> {
        self.initialize().await?;
        let id_column = self.options.id_column.clone();
        let non_auto = !matches!(self.options.id_strategy, IdStrategy::Auto);
        let mut doc = self.apply_defaults(&document);
        if non_auto && doc.get(&id_column).is_none() {
            let id = self.generate_id();
            if let Value::Object(obj) = &mut doc {
                obj.set(id_column.clone(), id);
            }
        }
        if self.lock().table_schema.is_empty() {
            self.create_table(&doc).await?;
            let schema = self.adapter.table_schema(&self.name).await?;
            self.lock().table_schema = schema;
        }
        self.migrate(&doc).await?;

        let insert = self.strip_auto_increments(&doc);
        let options = CrudOptions {
            returning: if self.adapter.db() == Db::Pg {
                Some(vec![id_column.clone()])
            } else {
                None
            },
            allow_delete_all: false,
        };
        let sql = {
            let collection = Builder::full().collection(&self.name, self.adapter.db())?;
            collection.insert_one(&insert, &options)?
        };
        let result = self
            .exec_with_ddl_retry(
                &sql,
                Ddl {
                    doc_for_create: Some(&doc),
                    doc_for_migrate: Some(&doc),
                    allow_column_migrate: true,
                },
            )
            .await?;
        let inserted_id = if non_auto {
            insert.get(&id_column).cloned()
        } else {
            result.insert_id.clone().or_else(|| {
                result
                    .rows
                    .first()
                    .and_then(|row| row.get(&id_column))
                    .cloned()
            })
        };
        Ok(inserted_id.unwrap_or(Value::Null))
    }

    async fn insert_many(&self, documents: Vec<Value>) -> mosql::Result<Vec<Value>> {
        let mut ids = Vec::with_capacity(documents.len());
        for document in documents {
            ids.push(self.insert_one(document).await?);
        }
        Ok(ids)
    }

    async fn update_one(&self, query: &Value, update: &Value) -> mosql::Result<i64> {
        Ok(self.update(query, update, true).await?.0)
    }

    async fn update_many(&self, query: &Value, update: &Value) -> mosql::Result<i64> {
        Ok(self.update(query, update, false).await?.0)
    }

    async fn delete_one(&self, query: &Value) -> mosql::Result<i64> {
        self.initialize().await?;
        let options = CrudOptions {
            returning: None,
            allow_delete_all: false,
        };
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .delete_one(query, &options)?;
        // (built above, before the await)
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result.affected)
    }

    async fn delete_many(&self, query: &Value) -> mosql::Result<i64> {
        self.initialize().await?;
        let options = CrudOptions {
            returning: None,
            allow_delete_all: true,
        };
        let sql = Builder::full()
            .collection(&self.name, self.adapter.db())?
            .delete_many(query, &options)?;
        // (built above, before the await)
        let result = self.exec_with_ddl_retry(&sql, Ddl::default()).await?;
        Ok(result.affected)
    }
}

/// Copy every key of `source` over `target`.
fn merge(target: &mut Value, source: &Value) {
    let (Value::Object(dst), Some(src)) = (&mut *target, source.as_object()) else {
        return;
    };
    for (key, value) in src.iter() {
        dst.set(key.clone(), value.clone());
    }
}

/// `parseInt(row?.count || 0)` over a `COUNT(*)` row.
fn count_from(rows: &[Value]) -> i64 {
    rows.first()
        .and_then(|row| row.get("count"))
        .and_then(|value| match value {
            Value::Int(i) => Some(*i),
            Value::Float(f) => Some(*f as i64),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

/// `schemaDef?.required` / `.unique` / `.hidden`.
fn is_flag(def: Option<&Value>, flag: &str) -> bool {
    def.and_then(|d| d.get(flag)).and_then(Value::as_bool) == Some(true)
}

/// MySQL reports `VARCHAR` without a length for columns it cannot size; that
/// cannot be used in an `ALTER`, so the fallback type is used instead.
fn is_bare_char_type(kind: &str) -> bool {
    let lower = kind.trim().to_lowercase();
    let (name, has_length) = match lower.find('(') {
        Some(i) => (&lower[..i], true),
        None => (lower.as_str(), false),
    };
    matches!(name.trim(), "varchar" | "char" | "character") && !has_length
}

/// A MongoDB-style 24-character hex id: 8 of timestamp, 16 of randomness.
fn mongo_id() -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mixed = now.rotate_left(17)
        ^ SEQ.fetch_add(0x9E37_79B9_7F4A_7C15, AtomicOrdering::Relaxed)
        ^ ((std::process::id() as u64) << 32);
    format!("{:08x}{:016x}", (now / 1_000_000_000) as u32, mixed)
}

/// `new Date().toISOString()`.
fn now_iso() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = mosql::memory::civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        (rem % 3_600_000) / 60_000,
        (rem % 60_000) / 1_000,
        rem % 1_000
    )
}
