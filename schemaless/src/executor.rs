//! The seam between a schema-less collection and a database driver.
//!
//! umosql's `createSQLAdapter({ database, execute })` takes a closure that runs
//! one statement. That closure is the whole driver contract, and it is why
//! umosql can bundle no driver at all. This trait is the same idea in Rust:
//! [`SqlCollection`](crate::SqlCollection) only ever calls
//! [`SqlExecutor::execute`], so a driver is whatever can answer it — rusqlite,
//! tokio-postgres, mysql_async, a serverless HTTP driver, or a test double.

use core::fmt;
use core::future::Future;
use core::pin::Pin;
use std::sync::Arc;

use mosql::Value;

/// One statement's result, normalised across drivers.
///
/// The JavaScript adapter unwraps four different shapes (`{rows, rowCount}`,
/// `{rows, affectedRows, insertId}`, `{changes, lastInsertRowid}`, a bare
/// array); a driver does that unwrapping once, here, and the collection only
/// ever sees this.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecResult {
    /// Rows as objects. Empty for statements that return none.
    pub rows: Vec<Value>,
    /// `affectedRows`, `rowCount` or `changes`, whichever the driver reports.
    pub affected: i64,
    /// `lastInsertRowid`, `insertId`, or the `RETURNING` row's id.
    pub insert_id: Option<Value>,
}

impl ExecResult {
    /// A statement that matched or changed `affected` rows.
    pub fn affected(affected: i64) -> Self {
        ExecResult {
            rows: Vec::new(),
            affected,
            insert_id: None,
        }
    }

    /// A statement that returned `rows`.
    pub fn rows(rows: Vec<Value>) -> Self {
        ExecResult {
            rows,
            affected: 0,
            insert_id: None,
        }
    }
}

/// A driver error, kept structured so the collection can classify it.
///
/// The JavaScript adapter reads `e.code` and `e.errno` off the driver's error
/// and falls back to matching the message. Both are kept here for the same
/// reason: `42P01` and `no such table` mean the same thing, and which one a
/// driver reports varies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlError {
    /// SQLSTATE (`42P01`, `42703`) or a driver code.
    pub code: Option<String>,
    /// MySQL-style numeric error (`1146`, `1054`).
    pub errno: Option<i64>,
    /// The driver's message, lower-cased for classification.
    pub message: String,
}

/// What a failed statement says is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Something else went wrong.
    Nothing,
    /// The table does not exist yet.
    Table,
    /// The table exists but a column does not.
    Column,
}

impl SqlError {
    /// An error with only a message.
    pub fn new(message: impl Into<String>) -> Self {
        SqlError {
            code: None,
            errno: None,
            message: message.into(),
        }
    }

    /// An error carrying a SQLSTATE or driver code.
    pub fn with_code(code: impl Into<String>, message: impl Into<String>) -> Self {
        SqlError {
            code: Some(code.into()),
            errno: None,
            message: message.into(),
        }
    }

    /// `classifyError(e)` — is the table missing, a column missing, or neither?
    ///
    /// A missing column is checked first: Postgres reports it as
    /// `column "x" ... does not exist`, which would otherwise satisfy the
    /// generic missing-table `does not exist` match.
    pub fn classify(&self) -> Missing {
        let msg = self.message.to_lowercase();
        let missing_column = self.code.as_deref() == Some("42703")
            || self.errno == Some(1054)
            || msg.contains("no such column")
            || msg.contains("unknown column");
        let missing_table = !missing_column
            && (self.code.as_deref() == Some("42P01")
                || self.errno == Some(1146)
                || msg.contains("no such table")
                || msg.contains("does not exist")
                || msg.contains("unknown table"));
        match (missing_table, missing_column) {
            (true, _) => Missing::Table,
            (false, true) => Missing::Column,
            _ => Missing::Nothing,
        }
    }
}

impl fmt::Display for SqlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SqlError {}

/// A statement the query builder refused to build.
impl From<mosql::Error> for SqlError {
    fn from(e: mosql::Error) -> Self {
        SqlError::new(e.0)
    }
}

impl From<SqlError> for mosql::Error {
    fn from(e: SqlError) -> Self {
        mosql::Error::new(e.message)
    }
}

/// An owned, sendable future — the shape a `dyn SqlExecutor` can return.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Runs one statement. `params` is empty for everything the query builder
/// generates — umosql inlines escaped literals — but a driver may use it for
/// its own statements.
///
/// The future is boxed so the trait stays object-safe: one client holds one
/// `Arc<dyn SqlExecutor>`, whatever the driver is. An implementation is
/// `Box::pin(async move { … })`, and the allocation is noise next to the
/// round trip it wraps.
pub trait SqlExecutor: Send + Sync {
    /// Execute `sql` and return the normalised result.
    fn execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [Value],
    ) -> BoxFuture<'a, Result<ExecResult, SqlError>>;
}

/// Share one executor between adapters, as a connection pool would be.
impl<T: SqlExecutor + ?Sized> SqlExecutor for Arc<T> {
    fn execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [Value],
    ) -> BoxFuture<'a, Result<ExecResult, SqlError>> {
        (**self).execute(sql, params)
    }
}

/// Borrow an executor without taking ownership of it.
impl<T: SqlExecutor + ?Sized> SqlExecutor for &T {
    fn execute<'a>(
        &'a self,
        sql: &'a str,
        params: &'a [Value],
    ) -> BoxFuture<'a, Result<ExecResult, SqlError>> {
        (**self).execute(sql, params)
    }
}
