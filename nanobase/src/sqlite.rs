//! SQLite: the engine the platform's own data lives in.
//!
//! The amalgamation is compiled by `build.rs`; this module is the binding. It
//! covers what a catalog needs — open, prepare, bind, step, read — and no more,
//! because every extra function is more `unsafe` to justify.
//!
//! Two rules make the `unsafe` here tractable:
//!
//! * **SQLite copies what it is given.** Text is bound with `SQLITE_TRANSIENT`,
//!   so a Rust `String` can be dropped the moment `bind` returns. Binding with
//!   `SQLITE_STATIC` would be faster and would let SQLite read freed memory as
//!   soon as the caller's slice goes away.
//! * **Rows own their data.** [`Row`] copies every column out of SQLite before
//!   `step` returns, so there is no lifetime tying a row to a statement.
//!
//! A connection is opened per operation rather than held. The platform catalog
//! is read on startup and written on a management change, so opening is not on
//! any hot path — and it means nothing here has to claim `Send` for a raw
//! pointer.

use std::ffi::{c_char, c_double, c_int, c_void, CStr};
use std::path::Path;
use std::ptr::NonNull;

use mosql::Value;
use uflite::error::{Error, Result};

#[allow(non_camel_case_types)]
mod ffi {
    use super::{c_char, c_double, c_int, c_void};

    /// Opaque. Only ever handled behind a pointer.
    #[repr(C)]
    pub struct sqlite3 {
        _private: [u8; 0],
    }

    #[repr(C)]
    pub struct sqlite3_stmt {
        _private: [u8; 0],
    }

    pub type destructor = Option<unsafe extern "C" fn(*mut c_void)>;

    pub const SQLITE_OK: c_int = 0;
    pub const SQLITE_ROW: c_int = 100;
    pub const SQLITE_DONE: c_int = 101;

    pub const SQLITE_INTEGER: c_int = 1;
    pub const SQLITE_FLOAT: c_int = 2;
    pub const SQLITE_NULL: c_int = 5;

    pub const SQLITE_OPEN_READWRITE: c_int = 0x0000_0002;
    pub const SQLITE_OPEN_CREATE: c_int = 0x0000_0004;
    /// Serialized mode: safe to move a connection between threads.
    pub const SQLITE_OPEN_FULLMUTEX: c_int = 0x0001_0000;

    extern "C" {
        pub fn sqlite3_open_v2(
            filename: *const c_char,
            db: *mut *mut sqlite3,
            flags: c_int,
            vfs: *const c_char,
        ) -> c_int;
        pub fn sqlite3_close_v2(db: *mut sqlite3) -> c_int;
        pub fn sqlite3_errmsg(db: *mut sqlite3) -> *const c_char;
        pub fn sqlite3_prepare_v2(
            db: *mut sqlite3,
            sql: *const c_char,
            len: c_int,
            stmt: *mut *mut sqlite3_stmt,
            tail: *mut *const c_char,
        ) -> c_int;
        pub fn sqlite3_step(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_column_count(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_column_name(stmt: *mut sqlite3_stmt, i: c_int) -> *const c_char;
        pub fn sqlite3_column_type(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
        pub fn sqlite3_column_int64(stmt: *mut sqlite3_stmt, i: c_int) -> i64;
        pub fn sqlite3_column_double(stmt: *mut sqlite3_stmt, i: c_int) -> c_double;
        pub fn sqlite3_column_text(stmt: *mut sqlite3_stmt, i: c_int) -> *const u8;
        pub fn sqlite3_column_bytes(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
        pub fn sqlite3_bind_parameter_count(stmt: *mut sqlite3_stmt) -> c_int;
        pub fn sqlite3_bind_int64(stmt: *mut sqlite3_stmt, i: c_int, v: i64) -> c_int;
        pub fn sqlite3_bind_double(stmt: *mut sqlite3_stmt, i: c_int, v: c_double) -> c_int;
        pub fn sqlite3_bind_text(
            stmt: *mut sqlite3_stmt,
            i: c_int,
            v: *const c_char,
            n: c_int,
            destructor: destructor,
        ) -> c_int;
        pub fn sqlite3_bind_null(stmt: *mut sqlite3_stmt, i: c_int) -> c_int;
        pub fn sqlite3_changes64(db: *mut sqlite3) -> i64;
        pub fn sqlite3_exec(
            db: *mut sqlite3,
            sql: *const c_char,
            callback: *const c_void,
            arg: *mut c_void,
            errmsg: *mut *mut c_char,
        ) -> c_int;
    }
}

/// `SQLITE_TRANSIENT`: the sentinel that makes SQLite copy a bound string
/// before the call returns.
///
/// In C this is `((sqlite3_destructor_type)-1)`. It is not a real function
/// pointer, which is why it cannot be written as one — and it must not be
/// replaced by a no-op function, because SQLite would then treat the string as
/// static and read the caller's freed buffer.
fn transient() -> ffi::destructor {
    unsafe { std::mem::transmute::<isize, ffi::destructor>(-1) }
}

/// Turn a result code plus the connection's message into a service error.
///
/// A constraint violation is the caller's mistake — a duplicate name, a missing
/// foreign key — so it is a 409 rather than a 500. Everything else about a
/// local file failing really is an internal error.
unsafe fn error(db: *mut ffi::sqlite3, code: c_int) -> Error {
    let message = unsafe {
        CStr::from_ptr(ffi::sqlite3_errmsg(db))
            .to_string_lossy()
            .into_owned()
    };
    // Primary result codes are the low 8 bits of the extended code.
    match code & 0xff {
        19 => Error::conflict(message),
        5 | 6 => Error::unavailable(message),
        _ => Error::internal(format!("sqlite: {message}")),
    }
}

/// An open SQLite database.
pub struct Connection {
    db: NonNull<ffi::sqlite3>,
}

// Safety: the connection is opened with SQLITE_OPEN_FULLMUTEX, which makes
// SQLite serialize access internally, so moving it between threads is sound.
unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

impl Connection {
    /// Open or create a file database.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with(path.to_string_lossy().as_ref())
    }

    /// A private in-memory database, for tests.
    pub fn open_memory() -> Result<Self> {
        Self::open_with(":memory:")
    }

    fn open_with(path: &str) -> Result<Self> {
        let cpath = std::ffi::CString::new(path)
            .map_err(|_| Error::bad_request("database path contains a NUL byte"))?;
        let mut db: *mut ffi::sqlite3 = std::ptr::null_mut();
        // SAFETY: `cpath` is a valid NUL-terminated string that outlives the
        // call, and `db` is a valid out-pointer.
        let code = unsafe {
            ffi::sqlite3_open_v2(
                cpath.as_ptr(),
                &mut db,
                ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE | ffi::SQLITE_OPEN_FULLMUTEX,
                std::ptr::null(),
            )
        };
        if code != ffi::SQLITE_OK {
            let message = unsafe {
                CStr::from_ptr(ffi::sqlite3_errmsg(db))
                    .to_string_lossy()
                    .into_owned()
            };
            if !db.is_null() {
                unsafe { ffi::sqlite3_close_v2(db) };
            }
            return Err(Error::internal(format!("cannot open database: {message}")));
        }
        // SAFETY: `sqlite3_open_v2` succeeded, so `db` is non-null and valid.
        let conn = Connection {
            db: unsafe { NonNull::new_unchecked(db) },
        };
        conn.pragma()?;
        Ok(conn)
    }

    /// The settings a platform catalog wants.
    ///
    /// WAL lets a reader and a writer proceed at once, `NORMAL` durability is
    /// the right trade for data that is not a bank ledger, and a busy timeout
    /// turns a concurrent write into a short wait instead of an error.
    fn pragma(&self) -> Result<()> {
        self.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;",
        )
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        let csql = std::ffi::CString::new(sql)
            .map_err(|_| Error::bad_request("SQL contains a NUL byte"))?;
        // SAFETY: the statement text is valid for the call, and the trailing
        // arguments ask SQLite to report errors through `errmsg` instead.
        let code = unsafe {
            ffi::sqlite3_exec(
                self.db.as_ptr(),
                csql.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if code == ffi::SQLITE_OK {
            Ok(())
        } else {
            Err(unsafe { error(self.db.as_ptr(), code) })
        }
    }

    /// Run one statement with parameters, returning the number of rows changed.
    pub fn execute(&self, sql: &str, params: &[Value]) -> Result<u64> {
        let mut stmt = self.prepare(sql)?;
        stmt.bind(params)?;
        stmt.step()?;
        // SAFETY: the connection is open.
        Ok(unsafe { ffi::sqlite3_changes64(self.db.as_ptr()) }.max(0) as u64)
    }

    /// Run one statement and collect every row it produces.
    pub fn query(&self, sql: &str, params: &[Value]) -> Result<Vec<Row>> {
        let mut stmt = self.prepare(sql)?;
        stmt.bind(params)?;
        let mut rows = Vec::new();
        while let Some(row) = stmt.step()? {
            rows.push(row);
        }
        Ok(rows)
    }

    /// Run one statement and return its first row.
    pub fn query_one(&self, sql: &str, params: &[Value]) -> Result<Option<Row>> {
        let mut stmt = self.prepare(sql)?;
        stmt.bind(params)?;
        stmt.step()
    }

    pub fn prepare(&self, sql: &str) -> Result<Statement<'_>> {
        let csql = std::ffi::CString::new(sql)
            .map_err(|_| Error::bad_request("SQL contains a NUL byte"))?;
        let mut stmt: *mut ffi::sqlite3_stmt = std::ptr::null_mut();
        // SAFETY: `csql` is NUL-terminated and outlives the call. A negative
        // length tells SQLite to read up to the NUL itself.
        let code = unsafe {
            ffi::sqlite3_prepare_v2(
                self.db.as_ptr(),
                csql.as_ptr(),
                -1,
                &mut stmt,
                std::ptr::null_mut(),
            )
        };
        if code != ffi::SQLITE_OK {
            return Err(unsafe { error(self.db.as_ptr(), code) });
        }
        // SAFETY: success guarantees a non-null statement.
        Ok(Statement {
            conn: self,
            stmt: unsafe { NonNull::new_unchecked(stmt) },
        })
    }

    /// Run `f` inside a transaction, rolling back if it fails.
    ///
    /// The catalog is rewritten wholesale on save; without a transaction a
    /// crash halfway would leave it half-updated, which is worse than not
    /// updating at all.
    pub fn transaction<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.execute_batch("BEGIN IMMEDIATE")?;
        match f(self) {
            Ok(value) => {
                self.execute_batch("COMMIT")?;
                Ok(value)
            }
            Err(e) => {
                // A failed rollback is not worth reporting over the original
                // error, which is the one that explains what went wrong.
                let _ = self.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: the pointer came from a successful `sqlite3_open_v2` and is
        // closed exactly once, here.
        unsafe { ffi::sqlite3_close_v2(self.db.as_ptr()) };
    }
}

/// A prepared statement, borrowing the connection it came from.
pub struct Statement<'a> {
    conn: &'a Connection,
    stmt: NonNull<ffi::sqlite3_stmt>,
}

impl<'a> Statement<'a> {
    /// Bind `params` to `?1`, `?2`, … in order.
    pub fn bind(&mut self, params: &[Value]) -> Result<()> {
        // SAFETY: the statement is valid while it is alive.
        let expected = unsafe { ffi::sqlite3_bind_parameter_count(self.stmt.as_ptr()) };
        if params.len() != expected as usize {
            return Err(Error::internal(format!(
                "statement takes {expected} parameter(s), {} given",
                params.len()
            )));
        }
        for (i, value) in params.iter().enumerate() {
            let index = i as c_int + 1;
            let code = unsafe { self.bind_one(index, value) };
            if code != ffi::SQLITE_OK {
                return Err(unsafe { error(self.conn.db.as_ptr(), code) });
            }
        }
        Ok(())
    }

    /// # Safety
    ///
    /// The statement must be valid and `index` within range.
    unsafe fn bind_one(&self, index: c_int, value: &Value) -> c_int {
        let stmt = self.stmt.as_ptr();
        match value {
            Value::Null => unsafe { ffi::sqlite3_bind_null(stmt, index) },
            Value::Bool(b) => unsafe { ffi::sqlite3_bind_int64(stmt, index, i64::from(*b)) },
            Value::Int(i) => unsafe { ffi::sqlite3_bind_int64(stmt, index, *i) },
            Value::Float(f) => unsafe { ffi::sqlite3_bind_double(stmt, index, *f) },
            // JSON is stored as text, which is what SQLite's JSON functions
            // expect and what an app's own schema declares.
            Value::Array(_) | Value::Object(_) => unsafe {
                let json = mosql::to_json(value);
                ffi::sqlite3_bind_text(
                    stmt,
                    index,
                    json.as_ptr() as *const c_char,
                    json.len() as c_int,
                    transient(),
                )
            },
            Value::Str(s) | Value::Date(s) => unsafe {
                ffi::sqlite3_bind_text(
                    stmt,
                    index,
                    s.as_ptr() as *const c_char,
                    s.len() as c_int,
                    transient(),
                )
            },
        }
    }

    /// Advance one row. `Ok(None)` means the statement is finished.
    pub fn step(&mut self) -> Result<Option<Row>> {
        // SAFETY: the statement is valid, and a non-final code is an error.
        let code = unsafe { ffi::sqlite3_step(self.stmt.as_ptr()) };
        match code {
            ffi::SQLITE_ROW => Ok(Some(unsafe { self.read_row() })),
            ffi::SQLITE_DONE => Ok(None),
            _ => Err(unsafe { error(self.conn.db.as_ptr(), code) }),
        }
    }

    /// Copy the current row out of SQLite.
    ///
    /// # Safety
    ///
    /// Only valid immediately after a step returned `SQLITE_ROW`.
    unsafe fn read_row(&self) -> Row {
        let stmt = self.stmt.as_ptr();
        let count = unsafe { ffi::sqlite3_column_count(stmt) };
        let mut columns = Vec::with_capacity(count.max(0) as usize);
        let mut values = Vec::with_capacity(count.max(0) as usize);
        for i in 0..count {
            let name = unsafe { ffi::sqlite3_column_name(stmt, i) };
            columns.push(if name.is_null() {
                format!("column{i}")
            } else {
                unsafe { CStr::from_ptr(name) }
                    .to_string_lossy()
                    .into_owned()
            });
            values.push(unsafe { read_column(stmt, i) });
        }
        Row { columns, values }
    }
}

/// # Safety
///
/// `stmt` must be positioned on a row.
unsafe fn read_column(stmt: *mut ffi::sqlite3_stmt, i: c_int) -> Value {
    match unsafe { ffi::sqlite3_column_type(stmt, i) } {
        ffi::SQLITE_INTEGER => Value::Int(unsafe { ffi::sqlite3_column_int64(stmt, i) }),
        ffi::SQLITE_FLOAT => Value::Float(unsafe { ffi::sqlite3_column_double(stmt, i) }),
        ffi::SQLITE_NULL => Value::Null,
        _ => {
            // TEXT and BLOB both come back as a string: a BLOB in a platform
            // catalog would be a bug, and lossy UTF-8 keeps it readable rather
            // than panicking on the way out.
            let ptr = unsafe { ffi::sqlite3_column_text(stmt, i) };
            if ptr.is_null() {
                return Value::Null;
            }
            let len = unsafe { ffi::sqlite3_column_bytes(stmt, i) }.max(0) as usize;
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
            Value::Str(String::from_utf8_lossy(bytes).into_owned())
        }
    }
}

impl Drop for Statement<'_> {
    fn drop(&mut self) {
        // SAFETY: finalising a statement exactly once, here.
        unsafe { ffi::sqlite3_finalize(self.stmt.as_ptr()) };
    }
}

/// One row, owning everything it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub columns: Vec<String>,
    pub values: Vec<Value>,
}

impl Row {
    /// A column's value by name.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.columns
            .iter()
            .position(|c| c == name)
            .map(|i| &self.values[i])
    }

    /// A column's value by position.
    pub fn at(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }
}
