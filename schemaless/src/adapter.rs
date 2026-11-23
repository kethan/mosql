//! `createSQLAdapter` — the statements a schema-less collection needs beyond
//! the query builder: introspection, DDL, and identifier quoting.

use std::collections::HashMap;
use std::sync::Arc;

use mosql::{Builder, Db, Value};

use crate::collection::{CollectionOptions, SqlCollection};
use crate::executor::{ExecResult, SqlError, SqlExecutor};
use crate::infer::escape_value;

/// One database, reached through one [`SqlExecutor`].
pub struct SqlAdapter {
    db: Db,
    executor: Arc<dyn SqlExecutor>,
}

/// A fresh query builder.
///
/// [`Builder`] holds its operator tables in `Rc`s, so it is neither `Send` nor
/// `Sync` and cannot live in an adapter that is shared across tasks. Building
/// one per statement costs a few allocations, which is noise next to the round
/// trip it produces.
pub fn builder() -> Builder {
    Builder::full()
}

impl SqlAdapter {
    /// A adapter over `executor` speaking the `db` dialect.
    pub fn new(db: Db, executor: impl SqlExecutor + 'static) -> Self {
        SqlAdapter::with_shared(db, Arc::new(executor))
    }

    /// [`SqlAdapter::new`] for an executor shared with other adapters.
    pub fn with_shared(db: Db, executor: Arc<dyn SqlExecutor>) -> Self {
        SqlAdapter { db, executor }
    }

    /// The dialect this adapter speaks.
    pub fn db(&self) -> Db {
        self.db
    }

    /// Run one statement.
    pub async fn execute(&self, sql: &str) -> Result<ExecResult, SqlError> {
        self.executor.execute(sql, &[]).await
    }

    /// `tableExists(tableName)`.
    pub async fn table_exists(&self, table: &str) -> bool {
        let sql = match self.db {
            Db::Pg => format!(
                "SELECT EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'public' AND table_name = '{table}')"
            ),
            Db::Mysql => format!(
                "SELECT COUNT(*) as count FROM information_schema.tables WHERE table_name = '{table}' AND table_schema = DATABASE()"
            ),
            Db::Sqlite => format!(
                "SELECT name FROM sqlite_master WHERE type='table' AND name='{table}'"
            ),
        };
        // The adapter reports a failed probe as "does not exist", exactly like
        // the JavaScript `try { ... } catch { return false }`.
        let Ok(result) = self.execute(&sql).await else {
            return false;
        };
        result
            .rows
            .first()
            .and_then(|row| row_get(row, &["exists", "count", "name"]))
            .is_some_and(truthy)
    }

    /// `getTableSchema(tableName)` — user columns and their declared types.
    ///
    /// `_id` and the two timestamp columns are dropped: the collection adds
    /// them itself and must not treat them as fields to migrate.
    pub async fn table_schema(&self, table: &str) -> Result<HashMap<String, String>, SqlError> {
        let sql = match self.db {
            Db::Pg => format!(
                "SELECT column_name, data_type FROM information_schema.columns WHERE table_name = '{table}'"
            ),
            Db::Mysql => format!(
                "SELECT COLUMN_NAME, DATA_TYPE FROM information_schema.columns WHERE TABLE_NAME = '{table}' AND TABLE_SCHEMA = DATABASE()"
            ),
            Db::Sqlite => format!("PRAGMA table_info({table})"),
        };
        let result = self.execute(&sql).await?;
        let mut columns = HashMap::new();
        for row in &result.rows {
            let (Some(name), Some(kind)) = (
                row_get(row, &["column_name", "COLUMN_NAME", "name"]).and_then(Value::as_str),
                row_get(row, &["data_type", "DATA_TYPE", "type"]).and_then(Value::as_str),
            ) else {
                continue;
            };
            if name == "_id" || name.starts_with("created_") || name.starts_with("updated_") {
                continue;
            }
            columns.insert(name.to_string(), kind.to_uppercase());
        }
        Ok(columns)
    }

    /// `listCollections()`.
    pub async fn list_collections(&self) -> Result<Vec<String>, SqlError> {
        let sql = match self.db {
            Db::Pg => {
                "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'"
            }
            Db::Mysql => {
                "SELECT TABLE_NAME FROM information_schema.tables WHERE TABLE_SCHEMA = DATABASE()"
            }
            Db::Sqlite => "SELECT name FROM sqlite_master WHERE type='table'",
        };
        let result = self.execute(sql).await?;
        Ok(result
            .rows
            .iter()
            .filter_map(|row| {
                row_get(row, &["table_name", "TABLE_NAME", "name"]).and_then(Value::as_str)
            })
            .map(str::to_string)
            .collect())
    }

    /// `dropCollection(name)`.
    pub async fn drop_collection(&self, table: &str) -> Result<(), SqlError> {
        let sql = match self.db {
            Db::Pg => format!("DROP TABLE IF EXISTS {table} CASCADE"),
            Db::Mysql => format!("DROP TABLE IF EXISTS {}", quote_ident(table, self.db)),
            Db::Sqlite => format!("DROP TABLE IF EXISTS {table}"),
        };
        self.execute(&sql).await.map(|_| ())
    }

    /// `createTableWithSchema(tableName, schema)` — either a JSON Schema
    /// (`{ properties, required }`) or a plain `{ column: type }` map.
    pub async fn create_table_with_schema(
        &self,
        table: &str,
        schema: &Value,
    ) -> Result<(), SqlError> {
        let mapped = map_json_schema(schema, self.db);
        let collection = SqlCollection::new(
            table,
            Arc::new(SqlAdapter {
                db: self.db,
                executor: Arc::clone(&self.executor),
            }),
            CollectionOptions {
                schema: mapped,
                ..CollectionOptions::default()
            },
        );
        collection.create_table(&Value::object()).await
    }

    /// `addColumn(tableName, columnName, type, options)`.
    pub async fn add_column(
        &self,
        table: &str,
        column: &str,
        kind: &str,
        options: ColumnOptions,
    ) -> Result<(), SqlError> {
        let sql = format!(
            "ALTER TABLE {} ADD COLUMN {} {}{}{}{}",
            quote_ident(table, self.db),
            quote_ident(column, self.db),
            kind,
            options.nullable(),
            options.unique(),
            options.default(self.db)
        );
        self.execute(&sql).await.map(|_| ())
    }

    /// `renameColumn(tableName, oldName, newName)`.
    pub async fn rename_column(&self, table: &str, old: &str, new: &str) -> Result<(), SqlError> {
        let sql = format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            quote_ident(table, self.db),
            quote_ident(old, self.db),
            quote_ident(new, self.db)
        );
        self.execute(&sql).await.map(|_| ())
    }

    /// `modifyColumn(tableName, columnName, newType, options)` — SQLite cannot.
    pub async fn modify_column(
        &self,
        table: &str,
        column: &str,
        new_type: &str,
        options: ColumnOptions,
    ) -> Result<(), SqlError> {
        if self.db == Db::Sqlite {
            return Err(SqlError::new("SQLite does not support MODIFY COLUMN"));
        }
        let sql = match self.db {
            Db::Pg => format!(
                "ALTER TABLE {table} ALTER COLUMN {column} TYPE {new_type}{}",
                options.default(self.db)
            ),
            Db::Mysql => format!(
                "ALTER TABLE {} MODIFY COLUMN {} {}{}{}{}",
                quote_ident(table, self.db),
                quote_ident(column, self.db),
                new_type,
                options.nullable(),
                options.unique(),
                options.default(self.db)
            ),
            Db::Sqlite => unreachable!("handled above"),
        };
        self.execute(&sql).await.map(|_| ())
    }

    /// `dropIndex(tableName, indexName)`.
    pub async fn drop_index(&self, table: &str, index: &str) -> Result<(), SqlError> {
        let sql = match self.db {
            Db::Pg => format!("DROP INDEX IF EXISTS {index}"),
            Db::Mysql => format!(
                "DROP INDEX {} ON {}",
                quote_ident(index, self.db),
                quote_ident(table, self.db)
            ),
            Db::Sqlite => format!("DROP INDEX IF EXISTS {index}"),
        };
        self.execute(&sql).await.map(|_| ())
    }

    /// `collection(name, config)` — a schema-less collection on this adapter.
    pub fn collection(&self, name: &str, options: CollectionOptions) -> SqlCollection {
        SqlCollection::new(name, self.clone_for(name), options)
    }

    /// A cheap clone: the executor is shared, the builder is not.
    fn clone_for(&self, _name: &str) -> Arc<SqlAdapter> {
        Arc::new(SqlAdapter {
            db: self.db,
            executor: Arc::clone(&self.executor),
        })
    }
}

/// `NOT NULL`, `UNIQUE` and `DEFAULT` fragments for one column.
#[derive(Debug, Clone, Default)]
pub struct ColumnOptions {
    pub required: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnOptions {
    fn nullable(&self) -> &'static str {
        if self.required {
            " NOT NULL"
        } else {
            ""
        }
    }

    fn unique(&self) -> &'static str {
        if self.unique {
            " UNIQUE"
        } else {
            ""
        }
    }

    fn default(&self, db: Db) -> String {
        match &self.default {
            Some(v) => format!(" DEFAULT {}", escape_value(v, db)),
            None => String::new(),
        }
    }
}

/// `qi(name, database)` — quote MySQL identifiers that collide with reserved
/// words (`longText` lexes as LONGTEXT), leaving every other dialect alone.
///
/// An invalid name is returned as-is so the database reports the error, which
/// is what the JavaScript adapter does.
pub fn quote_ident(name: &str, db: Db) -> String {
    if db != Db::Mysql {
        return name.to_string();
    }
    match builder().collection(name, db) {
        Ok(c) => c.name().to_string(),
        Err(_) => name.to_string(),
    }
}

/// `createTableWithSchema`'s JSON-Schema → column mapping.
fn map_json_schema(schema: &Value, db: Db) -> Value {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return schema.clone();
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let to_type = |p: &Value| -> String {
        match p.get("type").and_then(Value::as_str) {
            Some("string") => {
                if p.get("format").and_then(Value::as_str) == Some("date-time") {
                    return match db {
                        Db::Pg => "TIMESTAMP",
                        Db::Mysql => "DATETIME",
                        Db::Sqlite => "TEXT",
                    }
                    .to_string();
                }
                match (db, p.get("maxLength").and_then(Value::as_f64)) {
                    (Db::Pg, Some(n)) => format!("VARCHAR({})", n as i64),
                    (Db::Mysql, Some(n)) => format!("VARCHAR({})", n as i64),
                    (Db::Pg, None) => "TEXT".to_string(),
                    (Db::Mysql, None) => "VARCHAR(255)".to_string(),
                    (Db::Sqlite, _) => "TEXT".to_string(),
                }
            }
            Some("integer") => match db {
                Db::Mysql => "INT",
                _ => "INTEGER",
            }
            .to_string(),
            Some("number") => match db {
                Db::Sqlite => "REAL",
                _ => "DECIMAL(20,6)",
            }
            .to_string(),
            Some("boolean") => match db {
                Db::Pg => "BOOLEAN",
                Db::Mysql => "TINYINT(1)",
                Db::Sqlite => "INTEGER",
            }
            .to_string(),
            Some("array") | Some("object") => match db {
                Db::Pg => "JSONB",
                Db::Mysql => "JSON",
                Db::Sqlite => "TEXT",
            }
            .to_string(),
            _ => match db {
                Db::Pg => "TEXT",
                Db::Mysql => "VARCHAR(255)",
                Db::Sqlite => "TEXT",
            }
            .to_string(),
        }
    };

    let mut out = mosql::Object::new();
    for (key, p) in props.iter() {
        let mut def = mosql::Object::new();
        def.set("type", Value::str(to_type(p)));
        def.set("required", Value::Bool(required.contains(&key.as_str())));
        if let Some(d) = p.get("default") {
            def.set("default", d.clone());
        }
        out.set(key.clone(), Value::Object(def));
    }
    Value::Object(out)
}

/// The first of `keys` present in a row, so the three introspection shapes
/// (`column_name` / `COLUMN_NAME` / `name`) can be read with one call.
fn row_get<'a>(row: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| row.get(k))
}

/// `!!value` over a driver row: `0`, `false`, `null` and `""` are falsy.
fn truthy(v: &Value) -> bool {
    !matches!(v, Value::Null | Value::Bool(false))
        && !matches!(v, Value::Int(0))
        && !matches!(v, Value::Float(f) if *f == 0.0)
        && !matches!(v, Value::Str(s) if s.is_empty())
}
