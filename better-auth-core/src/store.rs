//! The storage boundary.
//!
//! Every one of the 59 routes reaches data through [`AuthStore`] and nothing
//! else. The trait is the eight operations an adapter has to provide — the
//! same shape as Better Auth's custom adapter interface — so a store built on
//! tokio-postgres or mysql_async plugs in without a handler changing.
//!
//! Queries are described, not written: a list of [`WhereClause`]s joined by
//! AND and OR, with the same precedence SQL gives them. [`SqliteAuthStore`]
//! turns that description into parameterised SQL; [`MemoryStore`] evaluates
//! it with [`matches`], which any non-SQL store can reuse.
//!
//! Methods return boxed futures with every argument sharing the receiver's
//! lifetime, so an implementation can `async move` borrowed arguments
//! straight in. The SQLite and memory stores do their work eagerly and
//! return the result in a ready future — there is nothing to await on a
//! local file.

use std::future::Future;
#[cfg(feature = "native")]
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use mosql::Value;

#[cfg(feature = "native")]
pub mod http_store;
#[cfg(feature = "native")]
pub mod tcp_store;

#[cfg(feature = "native")]
use nanobase::sqlite::Connection;

use crate::error::{AuthError, AuthResult};
use crate::models::{self, Record, RecordMap};
use crate::sql::{check_field, check_model, check_update};
#[cfg(feature = "native")]
use crate::sql::{
    count_sql, delete_sql, insert_sql, row_to_record, select_one_sql, select_sql, update_sql,
};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a field is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Eq,
    Ne,
    Lt,
    Lte,
    Gt,
    Gte,
    In,
    Contains,
    StartsWith,
    EndsWith,
}

/// One comparison. `connector_or` says how this clause joins the one before
/// it: `false` is AND, `true` is OR. AND binds tighter than OR, as in SQL.
#[derive(Debug, Clone, PartialEq)]
pub struct WhereClause {
    pub field: String,
    pub op: FilterOp,
    pub value: Value,
    pub connector_or: bool,
}

impl WhereClause {
    pub fn new(field: &str, op: FilterOp, value: Value) -> Self {
        WhereClause {
            field: field.to_string(),
            op,
            value,
            connector_or: false,
        }
    }

    pub fn or(field: &str, op: FilterOp, value: Value) -> Self {
        WhereClause {
            connector_or: true,
            ..WhereClause::new(field, op, value)
        }
    }

    pub fn eq(field: &str, value: Value) -> Self {
        WhereClause::new(field, FilterOp::Eq, value)
    }

    /// `id = ?`, the most common lookup.
    pub fn id(id: &str) -> Self {
        WhereClause::eq("id", Value::str(id))
    }

    pub(crate) fn check(&self, model: &str) -> AuthResult<()> {
        check_field(model, &self.field)
    }
}

/// What [`AuthStore::find_many`] returns.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FindManyQuery {
    pub r#where: Vec<WhereClause>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    /// (field, ascending)
    pub sort_by: Option<(String, bool)>,
}

impl FindManyQuery {
    pub fn new(r#where: Vec<WhereClause>) -> Self {
        FindManyQuery {
            r#where,
            ..FindManyQuery::default()
        }
    }

    pub fn sorted(mut self, field: &str, ascending: bool) -> Self {
        self.sort_by = Some((field.to_string(), ascending));
        self
    }

    pub fn page(mut self, limit: u64, offset: u64) -> Self {
        self.limit = Some(limit);
        self.offset = Some(offset);
        self
    }
}

/// Eight operations, and a store is done.
pub trait AuthStore: Send + Sync + 'static {
    /// Insert a record and return it as stored, with `id`, `createdAt` and
    /// `updatedAt` filled in where absent.
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>>;
    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>>;
    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>>;
    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>>;
    /// Update every matching record; return the first one as it now stands.
    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>>;
    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>>;
    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>>;
    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>>;
}

/// The typed view: records come back as models.
pub trait AuthStoreExt: AuthStore {
    fn create_typed<'a, T: Record>(&'a self, record: &T) -> BoxFuture<'a, AuthResult<T>> {
        let data = record.to_record();
        Box::pin(async move { T::from_record(&self.create(T::MODEL, data).await?) })
    }

    fn find_one_typed<'a, T: Record>(
        &'a self,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<T>>> {
        Box::pin(async move {
            self.find_one(T::MODEL, r#where)
                .await?
                .as_ref()
                .map(T::from_record)
                .transpose()
        })
    }

    fn find_many_typed<'a, T: Record>(
        &'a self,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<T>>> {
        Box::pin(async move {
            self.find_many(T::MODEL, query)
                .await?
                .iter()
                .map(T::from_record)
                .collect()
        })
    }

    fn update_typed<'a, T: Record>(
        &'a self,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<T>>> {
        Box::pin(async move {
            self.update(T::MODEL, r#where, update)
                .await?
                .as_ref()
                .map(T::from_record)
                .transpose()
        })
    }
}

impl<S: AuthStore + ?Sized> AuthStoreExt for S {}

// ------------------------------------------------------------ description ---

/// Values compared the way SQL compares them: integers and floats by
/// number, 1/0 as booleans, everything else by equality.
pub fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Int(y)) | (Value::Int(y), Value::Bool(x)) => *x == (*y != 0),
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => (*x as f64) == *y,
        (Value::Float(x), Value::Float(y)) => x == y,
        _ => a == b,
    }
}

fn ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

fn clause_holds(record: &RecordMap, clause: &WhereClause) -> bool {
    let field = record.get(&clause.field).unwrap_or(&Value::Null);
    let value = &clause.value;
    match clause.op {
        FilterOp::Eq if value.is_null() => field.is_null(),
        FilterOp::Ne if value.is_null() => !field.is_null(),
        FilterOp::Eq => values_equal(field, value),
        FilterOp::Ne => !values_equal(field, value),
        FilterOp::Lt => ordering(field, value).is_some_and(|o| o == std::cmp::Ordering::Less),
        FilterOp::Lte => ordering(field, value).is_some_and(|o| o != std::cmp::Ordering::Greater),
        FilterOp::Gt => ordering(field, value).is_some_and(|o| o == std::cmp::Ordering::Greater),
        FilterOp::Gte => ordering(field, value).is_some_and(|o| o != std::cmp::Ordering::Less),
        FilterOp::In => value
            .as_array()
            .is_some_and(|items| items.iter().any(|item| values_equal(field, item))),
        FilterOp::Contains => string_pair(field, value)
            .is_some_and(|(a, b)| a.to_lowercase().contains(&b.to_lowercase())),
        FilterOp::StartsWith => string_pair(field, value)
            .is_some_and(|(a, b)| a.to_lowercase().starts_with(&b.to_lowercase())),
        FilterOp::EndsWith => string_pair(field, value)
            .is_some_and(|(a, b)| a.to_lowercase().ends_with(&b.to_lowercase())),
    }
}

fn string_pair(a: &Value, b: &Value) -> Option<(String, String)> {
    Some((a.as_str()?.to_string(), b.as_str()?.to_string()))
}

/// Does a record satisfy the clauses? AND binds tighter than OR, matching
/// how the SQL store groups them.
pub fn matches(record: &RecordMap, r#where: &[WhereClause]) -> bool {
    let mut group = Vec::new();
    for clause in r#where {
        if clause.connector_or && !group.is_empty() {
            if group.iter().all(|c: &WhereClause| clause_holds(record, c)) {
                return true;
            }
            group.clear();
        }
        group.push(clause.clone());
    }
    group.iter().all(|c| clause_holds(record, c))
}

/// Sort records by a field, the way `ORDER BY` would: numbers before
/// strings, nulls first.
pub fn sort_records(records: &mut [RecordMap], field: &str, ascending: bool) {
    records.sort_by(|a, b| {
        let key = |r: &RecordMap| r.get(field).unwrap_or(&Value::Null).clone();
        let (x, y) = (key(a), key(b));
        let order = match (&x, &y) {
            (Value::Null, Value::Null) => std::cmp::Ordering::Equal,
            (Value::Null, _) => std::cmp::Ordering::Less,
            (_, Value::Null) => std::cmp::Ordering::Greater,
            _ => ordering(&x, &y).unwrap_or(std::cmp::Ordering::Equal),
        };
        if ascending {
            order
        } else {
            order.reverse()
        }
    });
}

/// A record ready to be inserted: model and field names checked, `id`,
/// `createdAt` and `updatedAt` present.
pub fn prepare_create(model: &str, mut data: RecordMap) -> AuthResult<RecordMap> {
    check_model(model)?;
    let declared = models::columns(model).expect("checked");
    // Fields the table does not declare are allowed: they travel in the
    // extra column, and only declared names are ever turned into SQL.
    if data.get("id").and_then(Value::as_str).is_none() {
        data.set("id", Value::str(models::new_id()));
    }
    for (field, value) in models::defaults(model) {
        if data.get(field).is_none() {
            data.set(field, value.clone());
        }
    }
    let now = models::now_iso();
    if declared.contains(&"createdAt") && data.get("createdAt").and_then(Value::as_str).is_none() {
        data.set("createdAt", Value::str(now.clone()));
    }
    if declared.contains(&"updatedAt") && data.get("updatedAt").and_then(Value::as_str).is_none() {
        data.set("updatedAt", Value::str(now));
    }
    Ok(data)
}

fn poisoned<T>(_: PoisonError<T>) -> AuthError {
    AuthError::internal("a store lock was poisoned by an earlier panic")
}

// ------------------------------------------------------------ memory store ---

/// Keeps records in a vector and evaluates clauses in Rust. For tests and
/// for embedding where there is nothing to persist.
pub struct MemoryStore {
    records: Mutex<Vec<(String, RecordMap)>>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        MemoryStore {
            records: Mutex::new(Vec::new()),
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AuthStore for MemoryStore {
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>> {
        let result = (|| {
            let record = prepare_create(model, data)?;
            let mut all = self.records.lock().map_err(poisoned)?;
            all.push((model.to_string(), record.clone()));
            Ok(record)
        })();
        Box::pin(async move { result })
    }

    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = (|| {
            for clause in r#where {
                clause.check(model)?;
            }
            let all = self.records.lock().map_err(poisoned)?;
            Ok(all
                .iter()
                .filter(|(m, _)| m == model)
                .map(|(_, r)| r)
                .find(|r| matches(r, r#where))
                .cloned())
        })();
        Box::pin(async move { result })
    }

    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>> {
        let result = (|| {
            for clause in &query.r#where {
                clause.check(model)?;
            }
            if let Some((field, _)) = &query.sort_by {
                check_field(model, field)?;
            }
            let all = self.records.lock().map_err(poisoned)?;
            let mut found: Vec<RecordMap> = all
                .iter()
                .filter(|(m, _)| m == model)
                .map(|(_, r)| r)
                .filter(|r| matches(r, &query.r#where))
                .cloned()
                .collect();
            if let Some((field, ascending)) = &query.sort_by {
                sort_records(&mut found, field, *ascending);
            }
            let offset = query.offset.unwrap_or(0) as usize;
            found = found.into_iter().skip(offset).collect();
            if let Some(limit) = query.limit {
                found.truncate(limit as usize);
            }
            Ok(found)
        })();
        Box::pin(async move { result })
    }

    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            let all = self.records.lock().map_err(poisoned)?;
            Ok(all
                .iter()
                .filter(|(m, r)| m == model && matches(r, r#where))
                .count() as u64)
        })();
        Box::pin(async move { result })
    }

    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = (|| {
            check_update(model, &update)?;
            let mut all = self.records.lock().map_err(poisoned)?;
            let mut first: Option<RecordMap> = None;
            let mut touched = false;
            for (m, record) in all.iter_mut() {
                if m == model && matches(record, r#where) {
                    apply_update(model, record, &update);
                    touched = true;
                    if first.is_none() {
                        first = Some(record.clone());
                    }
                }
            }
            Ok(if touched { first } else { None })
        })();
        Box::pin(async move { result })
    }

    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            check_update(model, &update)?;
            let mut all = self.records.lock().map_err(poisoned)?;
            let mut count = 0;
            for (m, record) in all.iter_mut() {
                if m == model && matches(record, r#where) {
                    apply_update(model, record, &update);
                    count += 1;
                }
            }
            Ok(count)
        })();
        Box::pin(async move { result })
    }

    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>> {
        let result = (|| {
            let mut all = self.records.lock().map_err(poisoned)?;
            all.retain(|(m, r)| !(m == model && matches(r, r#where)));
            Ok(())
        })();
        Box::pin(async move { result })
    }

    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            let mut all = self.records.lock().map_err(poisoned)?;
            let before = all.len();
            all.retain(|(m, r)| !(m == model && matches(r, r#where)));
            Ok((before - all.len()) as u64)
        })();
        Box::pin(async move { result })
    }
}

fn apply_update(model: &str, record: &mut RecordMap, update: &RecordMap) {
    let declared = models::columns(model).expect("checked model");
    for (key, value) in update.iter() {
        if declared.contains(&key.as_str()) {
            record.set(key.clone(), value.clone());
        }
    }
    if declared.contains(&"updatedAt") {
        record.set("updatedAt", Value::str(models::now_iso()));
    }
}

// ------------------------------------------------------------- sqlite store ---

/// The reference store: one SQLite file, one connection, parameterised SQL.
///
/// Records live in the five declared tables. Fields a table does not declare
/// are kept in its `data` column as JSON and merged back on read, so the
/// eight-method contract holds for any record, not just the declared columns.
#[cfg(feature = "native")]
pub struct SqliteAuthStore {
    conn: Mutex<Connection>,
    path: PathBuf,
}

#[cfg(feature = "native")]
impl std::fmt::Debug for SqliteAuthStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteAuthStore")
            .field("path", &self.path)
            .finish()
    }
}

#[cfg(feature = "native")]
impl SqliteAuthStore {
    /// Open or create a file database and make sure the tables exist.
    pub fn open(path: impl AsRef<Path>) -> AuthResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let conn = Connection::open(&path)?;
        Self::init(conn, path)
    }

    /// A private in-memory database.
    pub fn open_memory() -> AuthResult<Self> {
        Self::init(Connection::open_memory()?, PathBuf::from(":memory:"))
    }

    fn init(conn: Connection, path: PathBuf) -> AuthResult<Self> {
        conn.execute_batch(&models::schema_sql())?;
        Ok(SqliteAuthStore {
            conn: Mutex::new(conn),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, uflite::Error>) -> AuthResult<T> {
        let conn = self.conn.lock().map_err(poisoned)?;
        f(&conn).map_err(AuthError::from)
    }

    fn create_now(&self, model: &str, data: RecordMap) -> AuthResult<RecordMap> {
        let record = prepare_create(model, data)?;
        let (sql, params) = insert_sql(model, &record)?;
        self.with(|conn| conn.execute(&sql, &params))?;
        Ok(record)
    }

    fn find_one_now(&self, model: &str, r#where: &[WhereClause]) -> AuthResult<Option<RecordMap>> {
        let (sql, params) = select_one_sql(model, r#where)?;
        let row = self.with(|conn| conn.query_one(&sql, &params))?;
        Ok(row.map(|row| row_to_record(model, &row.columns, &row.values)))
    }

    fn find_many_now(&self, model: &str, query: &FindManyQuery) -> AuthResult<Vec<RecordMap>> {
        let (sql, params) = select_sql(model, query)?;
        let rows = self.with(|conn| conn.query(&sql, &params))?;
        Ok(rows
            .iter()
            .map(|row| row_to_record(model, &row.columns, &row.values))
            .collect())
    }

    fn count_now(&self, model: &str, r#where: &[WhereClause]) -> AuthResult<u64> {
        let (sql, params) = count_sql(model, r#where)?;
        let row = self.with(|conn| conn.query_one(&sql, &params))?;
        Ok(row
            .and_then(|r| {
                r.get("n").and_then(|v| match v {
                    Value::Int(i) => Some(*i),
                    _ => None,
                })
            })
            .unwrap_or(0)
            .max(0) as u64)
    }

    fn update_now(
        &self,
        model: &str,
        r#where: &[WhereClause],
        update: &RecordMap,
    ) -> AuthResult<Option<RecordMap>> {
        let changed = self.update_many_now(model, r#where, update)?;
        if changed == 0 {
            return Ok(None);
        }
        self.find_one_now(model, r#where)
    }

    fn update_many_now(
        &self,
        model: &str,
        r#where: &[WhereClause],
        update: &RecordMap,
    ) -> AuthResult<u64> {
        let Some((sql, params)) = update_sql(model, r#where, update)? else {
            return Ok(0);
        };
        self.with(|conn| conn.execute(&sql, &params))
    }

    fn delete_now(&self, model: &str, r#where: &[WhereClause]) -> AuthResult<()> {
        self.delete_many_now(model, r#where).map(|_| ())
    }

    fn delete_many_now(&self, model: &str, r#where: &[WhereClause]) -> AuthResult<u64> {
        let (sql, params) = delete_sql(model, r#where)?;
        self.with(|conn| conn.execute(&sql, &params))
    }
}

#[cfg(feature = "native")]
impl AuthStore for SqliteAuthStore {
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>> {
        let result = self.create_now(model, data);
        Box::pin(async move { result })
    }

    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = self.find_one_now(model, r#where);
        Box::pin(async move { result })
    }

    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>> {
        let result = self.find_many_now(model, &query);
        Box::pin(async move { result })
    }

    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = self.count_now(model, r#where);
        Box::pin(async move { result })
    }

    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = self.update_now(model, r#where, &update);
        Box::pin(async move { result })
    }

    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = self.update_many_now(model, r#where, &update);
        Box::pin(async move { result })
    }

    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>> {
        let result = self.delete_now(model, r#where);
        Box::pin(async move { result })
    }

    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = self.delete_many_now(model, r#where);
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{b, s, Account, User};
    use crate::{array, object};

    fn user(email: &str, name: &str) -> RecordMap {
        object! {
            "name" => s(name),
            "email" => s(email),
            "emailVerified" => b(false),
            "isAnonymous" => b(false),
            "twoFactorEnabled" => b(false),
            "banned" => b(false),
            "createdAt" => s("2026-01-01T00:00:00Z"),
            "updatedAt" => s("2026-01-01T00:00:00Z"),
        }
    }

    fn store() -> SqliteAuthStore {
        SqliteAuthStore::open_memory().unwrap()
    }

    #[test]
    fn create_fills_id_and_reads_back() {
        let store = store();
        let created = block_on(store.create("user", user("ada@example.com", "Ada"))).unwrap();
        let id = created
            .get("id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        assert!(!id.is_empty());
        assert_eq!(created.get("role"), Some(&Value::str("user")));

        let found = block_on(store.find_one("user", &[WhereClause::id(&id)]))
            .unwrap()
            .unwrap();
        assert_eq!(found.get("email"), Some(&Value::str("ada@example.com")));
        assert_eq!(found.get("emailVerified"), Some(&Value::Bool(false)));
    }

    #[test]
    fn unique_email_is_a_conflict() {
        let store = store();
        block_on(store.create("user", user("ada@example.com", "Ada"))).unwrap();
        let err = block_on(store.create("user", user("ada@example.com", "Other"))).unwrap_err();
        assert_eq!(err.status, 409);
    }

    #[test]
    fn clauses_match_the_way_sql_does() {
        let store = store();
        for (email, name, banned) in [
            ("a@x.co", "Ada Lovelace", false),
            ("b@x.co", "Alan Turing", true),
            ("c@x.co", "Ada Noether", false),
        ] {
            let mut u = user(email, name);
            u.set("banned", b(banned));
            block_on(store.create("user", u)).unwrap();
        }
        let found = block_on(store.find_many(
            "user",
            FindManyQuery::new(vec![WhereClause::eq("banned", b(false))]).sorted("email", true),
        ))
        .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].get("email"), Some(&Value::str("a@x.co")));

        let found = block_on(store.find_many(
            "user",
            FindManyQuery::new(vec![
                WhereClause::new("name", FilterOp::Contains, s("ada")),
                WhereClause::or("banned", FilterOp::Eq, b(true)),
            ]),
        ))
        .unwrap();
        assert_eq!(found.len(), 3, "case-insensitive contains, or banned");

        let found = block_on(store.find_many(
            "user",
            FindManyQuery::new(vec![WhereClause::new(
                "name",
                FilterOp::StartsWith,
                s("alan"),
            )]),
        ))
        .unwrap();
        assert_eq!(found.len(), 1);

        let found = block_on(store.find_many(
            "user",
            FindManyQuery::new(vec![WhereClause::new(
                "email",
                FilterOp::In,
                array![s("a@x.co"), s("c@x.co")],
            )]),
        ))
        .unwrap();
        assert_eq!(found.len(), 2);

        let found = block_on(store.find_many(
            "user",
            FindManyQuery::new(vec![WhereClause::new("image", FilterOp::Eq, Value::Null)]),
        ))
        .unwrap();
        assert_eq!(found.len(), 3, "null equals null");

        let found = block_on(
            store.find_many(
                "user",
                FindManyQuery::new(vec![WhereClause::new(
                    "createdAt",
                    FilterOp::Gt,
                    s("2025-01-01T00:00:00Z"),
                )])
                .page(2, 1),
            ),
        )
        .unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].get("email"), Some(&Value::str("b@x.co")));

        assert_eq!(
            block_on(store.count("user", &[WhereClause::eq("banned", b(true))])).unwrap(),
            1
        );
    }

    #[test]
    fn and_binds_tighter_than_or_in_both_stores() {
        let mut r = user("a@x.co", "Ada");
        r.set("banned", b(true));
        let clauses = vec![
            WhereClause::eq("email", s("a@x.co")),
            WhereClause::eq("banned", b(false)),
            WhereClause::or("email", FilterOp::Eq, s("z@x.co")),
        ];
        // (a AND not banned) OR z → false for this record.
        assert!(!matches(&r, &clauses));
        let store = store();
        block_on(store.create("user", r)).unwrap();
        assert_eq!(block_on(store.count("user", &clauses)).unwrap(), 0);
    }

    #[test]
    fn update_touches_updated_at_and_returns_the_record() {
        let store = store();
        let created = block_on(store.create("user", user("ada@example.com", "Ada"))).unwrap();
        let id = created
            .get("id")
            .and_then(Value::as_str)
            .unwrap()
            .to_string();
        let updated = block_on(store.update(
            "user",
            &[WhereClause::id(&id)],
            object! {"name" => s("Ada L.")},
        ))
        .unwrap()
        .unwrap();
        assert_eq!(updated.get("name"), Some(&Value::str("Ada L.")));
        assert_ne!(updated.get("updatedAt"), created.get("updatedAt"));
        assert_eq!(
            block_on(store.update_many(
                "user",
                &[WhereClause::eq("banned", b(true))],
                object! {"name" => s("x")}
            ))
            .unwrap(),
            0
        );
    }

    #[test]
    fn delete_returns_how_many() {
        let store = store();
        block_on(store.create("user", user("a@x.co", "A"))).unwrap();
        block_on(store.create("user", user("b@x.co", "B"))).unwrap();
        assert_eq!(
            block_on(store.delete_many(
                "user",
                &[WhereClause::new("email", FilterOp::EndsWith, s("x.co"))]
            ))
            .unwrap(),
            2
        );
        assert_eq!(block_on(store.count("user", &[])).unwrap(), 0);
    }

    #[test]
    fn unknown_fields_are_refused_and_extras_survive() {
        let store = store();
        let err = block_on(store.find_one(
            "user",
            &[WhereClause::eq("1=1; DROP TABLE user", Value::Null)],
        ))
        .unwrap_err();
        assert_eq!(err.status, 500);
        let err = block_on(store.find_one("nope", &[])).unwrap_err();
        assert_eq!(err.status, 500);

        let mut u = user("ada@example.com", "Ada");
        u.set("plan", s("pro"));
        block_on(store.create("user", u)).unwrap();
        let found =
            block_on(store.find_one("user", &[WhereClause::eq("email", s("ada@example.com"))]))
                .unwrap()
                .unwrap();
        assert_eq!(found.get("plan"), Some(&Value::str("pro")));
        let err = block_on(store.update("user", &[], object! {"plan" => s("basic")})).unwrap_err();
        assert_eq!(err.status, 500);
    }

    #[test]
    fn typed_helpers_round_trip() {
        let store = store();
        let mut record = user("ada@example.com", "Ada");
        record.set("id", s("user_typed"));
        record.set("role", s("user"));
        let u = User::from_record(&record).unwrap();
        let created = block_on(store.create_typed(&u)).unwrap();
        assert_eq!(created.id, "user_typed", "an id given is kept");
        assert_eq!(created.email, "ada@example.com");
        let found = block_on(
            store.find_one_typed::<User>(&[WhereClause::eq("email", s("ada@example.com"))]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.id, created.id);
        let a = Account {
            id: "acc".into(),
            user_id: created.id.clone(),
            account_id: created.id.clone(),
            provider_id: "credential".into(),
            access_token: None,
            refresh_token: None,
            id_token: None,
            access_token_expires_at: None,
            refresh_token_expires_at: None,
            scope: None,
            password: Some("hash".into()),
            created_at: String::new(),
            updated_at: String::new(),
            extra: RecordMap::new(),
        };
        block_on(store.create_typed(&a)).unwrap();
        let accounts =
            block_on(
                store.find_many_typed::<Account>(FindManyQuery::new(vec![WhereClause::eq(
                    "userId",
                    s(created.id.clone()),
                )])),
            )
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].password.as_deref(), Some("hash"));
    }

    #[test]
    fn memory_store_agrees_with_sqlite() {
        let memory = MemoryStore::new();
        let sqlite = store();
        for store_i in 0..4 {
            let mut u = user(&format!("u{store_i}@x.co"), "U");
            u.set("banned", b(store_i % 2 == 1));
            block_on(memory.create("user", u.clone())).unwrap();
            block_on(sqlite.create("user", u)).unwrap();
        }
        let query = FindManyQuery::new(vec![
            WhereClause::eq("banned", b(false)),
            WhereClause::or("email", FilterOp::StartsWith, s("u1")),
        ])
        .sorted("email", false);
        let a = block_on(memory.find_many("user", query.clone())).unwrap();
        let b = block_on(sqlite.find_many("user", query)).unwrap();
        let emails = |rows: &[RecordMap]| {
            rows.iter()
                .filter_map(|r| r.get("email").and_then(Value::as_str).map(str::to_string))
                .collect::<Vec<_>>()
        };
        assert_eq!(emails(&a), emails(&b));
        // Not banned, or u1: u3 is banned and starts with u3, so it is out.
        assert_eq!(emails(&a), vec!["u2@x.co", "u1@x.co", "u0@x.co"]);
    }

    fn block_on<T: Future>(fut: T) -> T::Output {
        uflite::db::block_on(fut)
    }
}
