//! `services/table.js` — DDL requests, validated, then delegated to a database.
//!
//! The service owns no DDL of its own. It checks every identifier and column
//! type *before* anything reaches the adapter (umosql's identifier validator
//! accepts word characters only; column types may not carry SQL fragments),
//! then calls the adapter method for the operation. An adapter without a
//! capability fails with `The database adapter does not support <method>`.
//!
//! Implement [`TableDb`] over your driver; the default methods return that
//! unsupported error, so you only override what your database can do.

use core::future::Future;

use mosql::Value;

use crate::error::{Error, Result};

/// The adapter contract from table.js. Every method corresponds to a umosql
/// driver method and returns whatever the driver returns, as JSON.
pub trait TableDb: Send + Sync {
    fn list_collections<'a>(&'a self) -> impl Future<Output = Result<Value>> + Send + 'a {
        std::future::ready(Err(Error::unsupported("listCollections")))
    }
    fn get_table_schema<'a>(
        &'a self,
        table: &'a str,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = table;
        std::future::ready(Err(Error::unsupported("getTableSchema")))
    }
    fn create_table_with_schema<'a>(
        &'a self,
        table: &'a str,
        schema: &'a Value,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = (table, schema);
        std::future::ready(Err(Error::unsupported("createTableWithSchema")))
    }
    fn add_column<'a>(
        &'a self,
        table: &'a str,
        column: &'a str,
        column_type: &'a str,
        options: Option<&'a Value>,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = (table, column, column_type, options);
        std::future::ready(Err(Error::unsupported("addColumn")))
    }
    fn rename_column<'a>(
        &'a self,
        table: &'a str,
        old: &'a str,
        new: &'a str,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = (table, old, new);
        std::future::ready(Err(Error::unsupported("renameColumn")))
    }
    fn modify_column<'a>(
        &'a self,
        table: &'a str,
        column: &'a str,
        column_type: &'a str,
        options: Option<&'a Value>,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = (table, column, column_type, options);
        std::future::ready(Err(Error::unsupported("modifyColumn")))
    }
    fn drop_column<'a>(
        &'a self,
        table: &'a str,
        column: &'a str,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = (table, column);
        std::future::ready(Err(Error::unsupported("dropColumn")))
    }
    fn drop_collection<'a>(
        &'a self,
        table: &'a str,
    ) -> impl Future<Output = Result<Value>> + Send + 'a {
        let _ = table;
        std::future::ready(Err(Error::unsupported("dropCollection")))
    }
}

/// A shared adapter is still an adapter: `TableService::new(&db)` and
/// `TableService::new(Arc::new(db))` both work.
impl<T: TableDb + ?Sized> TableDb for &T {
    async fn list_collections(&self) -> Result<Value> {
        (**self).list_collections().await
    }
    async fn get_table_schema(&self, table: &str) -> Result<Value> {
        (**self).get_table_schema(table).await
    }
    async fn create_table_with_schema(&self, table: &str, schema: &Value) -> Result<Value> {
        (**self).create_table_with_schema(table, schema).await
    }
    async fn add_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        (**self)
            .add_column(table, column, column_type, options)
            .await
    }
    async fn rename_column(&self, table: &str, old: &str, new: &str) -> Result<Value> {
        (**self).rename_column(table, old, new).await
    }
    async fn modify_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        (**self)
            .modify_column(table, column, column_type, options)
            .await
    }
    async fn drop_column(&self, table: &str, column: &str) -> Result<Value> {
        (**self).drop_column(table, column).await
    }
    async fn drop_collection(&self, table: &str) -> Result<Value> {
        (**self).drop_collection(table).await
    }
}

impl<T: TableDb + ?Sized> TableDb for std::sync::Arc<T> {
    async fn list_collections(&self) -> Result<Value> {
        (**self).list_collections().await
    }
    async fn get_table_schema(&self, table: &str) -> Result<Value> {
        (**self).get_table_schema(table).await
    }
    async fn create_table_with_schema(&self, table: &str, schema: &Value) -> Result<Value> {
        (**self).create_table_with_schema(table, schema).await
    }
    async fn add_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        (**self)
            .add_column(table, column, column_type, options)
            .await
    }
    async fn rename_column(&self, table: &str, old: &str, new: &str) -> Result<Value> {
        (**self).rename_column(table, old, new).await
    }
    async fn modify_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        (**self)
            .modify_column(table, column, column_type, options)
            .await
    }
    async fn drop_column(&self, table: &str, column: &str) -> Result<Value> {
        (**self).drop_column(table, column).await
    }
    async fn drop_collection(&self, table: &str) -> Result<Value> {
        (**self).drop_collection(table).await
    }
}

/// `createTableService(db)`.
pub struct TableService<D: TableDb> {
    db: D,
}

impl<D: TableDb> TableService<D> {
    pub fn new(db: D) -> Self {
        TableService { db }
    }

    pub fn db(&self) -> &D {
        &self.db
    }

    /// List tables.
    pub async fn find(&self) -> Result<Value> {
        self.db.list_collections().await
    }

    /// Schema of one table.
    pub async fn get(&self, table: &str) -> Result<Value> {
        require_identifier(&Value::str(table), "table name")?;
        self.db.get_table_schema(table).await
    }

    /// `create({ name, schema })` — JSON Schema, or an already-mapped SQL
    /// schema `{ properties: { col: "TYPE" } }`.
    pub async fn create(&self, data: &Value) -> Result<Value> {
        if !data.is_object() {
            return Err(Error::bad_request("table data must be an object"));
        }
        let name = data.get("name").unwrap_or(&Value::Null);
        require_identifier(name, "table name")?;
        let schema = data.get("schema").unwrap_or(&Value::Null);
        if !schema.is_object() {
            return Err(Error::bad_request("schema must be an object"));
        }
        validate_schema(schema)?;
        let Value::Str(name) = name else {
            unreachable!()
        };
        self.db.create_table_with_schema(name, schema).await
    }

    /// `patch(table, { operation, ... })` with `operation` one of
    /// `addColumn`, `renameColumn`, `modifyColumn`, `dropColumn`.
    pub async fn patch(&self, table: &str, data: &Value) -> Result<Value> {
        require_identifier(&Value::str(table), "table name")?;
        if !data.is_object() {
            return Err(Error::bad_request("patch data must include an operation"));
        }
        let op = match data.get("operation") {
            Some(Value::Str(s)) => s.as_str(),
            _ => return Err(Error::bad_request("patch data must include an operation")),
        };
        match op {
            "addColumn" => {
                let column = data.get("columnName").unwrap_or(&Value::Null);
                let ty = data.get("type").unwrap_or(&Value::Null);
                require_identifier(column, "column name")?;
                require_column_type(ty)?;
                let (Value::Str(column), Value::Str(ty)) = (column, ty) else {
                    unreachable!()
                };
                self.db
                    .add_column(
                        table,
                        column,
                        ty,
                        data.get("options").filter(|o| !o.is_null()),
                    )
                    .await
            }
            "renameColumn" => {
                let old = data.get("oldName").unwrap_or(&Value::Null);
                let new = data.get("newName").unwrap_or(&Value::Null);
                require_identifier(old, "old column name")?;
                require_identifier(new, "new column name")?;
                let (Value::Str(old), Value::Str(new)) = (old, new) else {
                    unreachable!()
                };
                self.db.rename_column(table, old, new).await
            }
            "modifyColumn" => {
                let column = data.get("columnName").unwrap_or(&Value::Null);
                let ty = data.get("type").unwrap_or(&Value::Null);
                require_identifier(column, "column name")?;
                require_column_type(ty)?;
                let (Value::Str(column), Value::Str(ty)) = (column, ty) else {
                    unreachable!()
                };
                self.db
                    .modify_column(
                        table,
                        column,
                        ty,
                        data.get("options").filter(|o| !o.is_null()),
                    )
                    .await
            }
            "dropColumn" => {
                let column = data.get("columnName").unwrap_or(&Value::Null);
                require_identifier(column, "column name")?;
                let Value::Str(column) = column else {
                    unreachable!()
                };
                self.db.drop_column(table, column).await
            }
            other => Err(Error::bad_request(format!(
                "Unsupported table patch operation '{other}'"
            ))),
        }
    }

    /// Drop a table.
    pub async fn remove(&self, table: &str) -> Result<Value> {
        require_identifier(&Value::str(table), "table name")?;
        self.db.drop_collection(table).await
    }
}

// ---------------------------------------------------------------------------
// Validation — the same boundaries as table.js, hand-parsed (no regex crate)
// ---------------------------------------------------------------------------

/// `/^\w+$/`: one or more of `[A-Za-z0-9_]` and nothing else.
pub fn is_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// `^[A-Za-z][A-Za-z0-9_]*(\s+[A-Za-z][A-Za-z0-9_]*)*(\s*\(\s*\d+(\s*,\s*\d+)?\s*\))?$`
/// — a SQL type name such as `VARCHAR(255)`, `DOUBLE PRECISION`, `DECIMAL(10, 2)`.
pub fn is_column_type(value: &str) -> bool {
    let b = value.as_bytes();
    let mut i = 0;

    fn ident(b: &[u8], i: &mut usize) -> bool {
        let start = *i;
        if *i >= b.len() || !b[*i].is_ascii_alphabetic() {
            return false;
        }
        *i += 1;
        while *i < b.len() && (b[*i].is_ascii_alphanumeric() || b[*i] == b'_') {
            *i += 1;
        }
        *i > start
    }
    fn ws(b: &[u8], i: &mut usize) -> bool {
        let start = *i;
        while *i < b.len() && (b[*i] as char).is_whitespace() {
            *i += 1;
        }
        *i > start
    }
    fn digits(b: &[u8], i: &mut usize) -> bool {
        let start = *i;
        while *i < b.len() && b[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    }
    fn opt_ws(b: &[u8], i: &mut usize) {
        while *i < b.len() && (b[*i] as char).is_whitespace() {
            *i += 1;
        }
    }

    // First identifier.
    if !ident(b, &mut i) {
        return false;
    }
    // Further words, each preceded by whitespace.
    loop {
        let save = i;
        if ws(b, &mut i) && ident(b, &mut i) {
            continue;
        }
        i = save;
        break;
    }
    // Optional `\s*(n)` or `\s*(n, m)`: whitespace is only allowed when it
    // precedes a `(` — `\s*\(` — so a trailing space is not a valid type.
    let before_ws = i;
    opt_ws(b, &mut i);
    if i < b.len() && b[i] == b'(' {
        i += 1;
        opt_ws(b, &mut i);
        if !digits(b, &mut i) {
            return false;
        }
        opt_ws(b, &mut i);
        if i < b.len() && b[i] == b',' {
            i += 1;
            opt_ws(b, &mut i);
            if !digits(b, &mut i) {
                return false;
            }
            opt_ws(b, &mut i);
        }
        if i >= b.len() || b[i] != b')' {
            return false;
        }
        i += 1;
    } else {
        i = before_ws;
    }
    i == b.len()
}

fn require_identifier(value: &Value, label: &str) -> Result<()> {
    match value.as_str() {
        Some(s) if is_identifier(s) => Ok(()),
        _ => Err(Error::bad_request(format!(
            "{label} must be a non-empty SQL identifier"
        ))),
    }
}

fn require_column_type(value: &Value) -> Result<()> {
    match value.as_str() {
        Some(s) if is_column_type(s) => Ok(()),
        _ => Err(Error::bad_request("type must be a valid SQL column type")),
    }
}

/// `schema.properties ?? schema` must be an object; each column name is an
/// identifier and each present `type` a valid column type.
pub fn validate_schema(schema: &Value) -> Result<()> {
    let definitions = match schema.get("properties") {
        Some(p) => p,
        None => schema,
    };
    let Some(defs) = definitions.as_object() else {
        return Err(Error::bad_request("schema properties must be an object"));
    };
    for (name, definition) in defs.iter() {
        require_identifier(&Value::str(name.as_str()), "column name")?;
        let ty = match definition {
            Value::Str(_) => Some(definition),
            Value::Object(o) => o.get("type"),
            _ => None,
        };
        if let Some(ty) = ty {
            require_column_type(ty)?;
        }
    }
    Ok(())
}
