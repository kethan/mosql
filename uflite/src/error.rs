//! The one error type every service returns.
//!
//! uflite's JavaScript services throw plain `Error` objects and attach an HTTP
//! `status` where they know one (404 for a missing record, 413 for an oversized
//! file). Everything without a status is mapped to 400 by the REST layer when it
//! is a `TypeError`/`RangeError`, and to 500 otherwise. [`Error`] carries that
//! mapping explicitly so a consumer can turn it into a response directly.

use core::fmt;
use std::error::Error as StdError;

use mosql::Value;

/// A service error. [`Error::status`] is the HTTP status uflite would respond
/// with; [`Display`](fmt::Display) is the JavaScript error message, verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// 400 — invalid input. JavaScript `TypeError`/`RangeError` without a status.
    BadRequest(String),
    /// 404 — no record or file for the given id/key. JavaScript `NotFound`.
    NotFound(String),
    /// 413 — body exceeds `maxFileSize`.
    TooLarge(String),
    /// 500 — anything else: a multi-operation that is not allowed, an adapter
    /// that lacks a capability, a provider failure.
    Internal(String),
    /// The collection rejected a query or update (unknown operator, bad value).
    Query(mosql::Error),
}

pub type Result<T> = core::result::Result<T, Error>;

impl From<mosql::Error> for Error {
    fn from(e: mosql::Error) -> Self {
        Error::Query(e)
    }
}

impl Error {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Error::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Error::NotFound(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }

    /// HTTP status, following the mapping uflite's REST layer applies.
    pub fn status(&self) -> u16 {
        match self {
            Error::BadRequest(_) => 400,
            Error::NotFound(_) => 404,
            Error::TooLarge(_) => 413,
            Error::Internal(_) | Error::Query(_) => 500,
        }
    }

    /// The JavaScript `error.name` the REST layer would see.
    pub fn name(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "TypeError",
            Error::NotFound(_) => "NotFound",
            Error::TooLarge(_) => "RangeError",
            Error::Internal(_) | Error::Query(_) => "Error",
        }
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, Error::NotFound(_))
    }

    /// `No record found for id '<id>'`, the message every missing record gets.
    pub fn record_not_found(id: &Value) -> Self {
        Error::NotFound(format!("No record found for id '{}'", id.to_js_string()))
    }

    /// The error an adapter without a capability produces, for `TableDb`
    /// implementors that want to decline an operation explicitly.
    pub fn unsupported(method: &str) -> Self {
        Error::Internal(format!("The database adapter does not support {method}"))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadRequest(m) | Error::NotFound(m) | Error::TooLarge(m) | Error::Internal(m) => {
                f.write_str(m)
            }
            Error::Query(e) => fmt::Display::fmt(e, f),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Error::Query(e) => Some(e),
            _ => None,
        }
    }
}

impl From<Error> for Value {
    /// The JSON body uflite's REST layer serialises for this error.
    fn from(e: Error) -> Value {
        let mut o = mosql::Object::new();
        o.set("name", Value::str(e.name()));
        o.set("message", Value::str(e.to_string()));
        o.set("code", Value::Int(e.status() as i64));
        o.set("status", Value::Int(e.status() as i64));
        Value::Object(o)
    }
}
