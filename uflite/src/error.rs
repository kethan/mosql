//! The one error type every service returns.
//!
//! uflite's JavaScript services throw plain `Error` objects and attach an HTTP
//! `status` where they know one (404 for a missing record, 413 for an oversized
//! file). Everything without a status is mapped to 400 by the REST layer when it
//! is a `TypeError`/`RangeError`, and to 500 otherwise. [`Error`] carries that
//! mapping explicitly so a consumer can turn it into a response directly.
//!
//! It also covers the statuses the platform's own API contract needs (401, 403,
//! 409, 422, 429, 503, 405), and [`Error::to_wire`] produces that contract's
//! body — `{ name, message, code, className, errors }` — from any variant.
//! [`Error::name`] and [`From<Error> for Value`] keep the JavaScript shapes they
//! always had; the platform wire shape is a separate, explicit conversion.

use core::fmt;
use std::error::Error as StdError;

use mosql::Value;

/// A service error. [`Error::status`] is the HTTP status to respond with;
/// [`Display`](fmt::Display) is the message, verbatim.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// 400 — invalid input. JavaScript `TypeError`/`RangeError` without a status.
    BadRequest(String),
    /// 401 — missing, invalid or expired credential.
    Unauthorized(String),
    /// 403 — authenticated, but not allowed.
    Forbidden(String),
    /// 404 — no record or file for the given id/key. JavaScript `NotFound`.
    NotFound(String),
    /// 405 — the route exists, the method does not.
    MethodNotAllowed(String),
    /// 409 — duplicate, or an invalid state transition.
    Conflict(String),
    /// 413 — body exceeds `maxFileSize`.
    TooLarge(String),
    /// 422 — well-formed request that violates a domain rule.
    Unprocessable(String),
    /// 429 — request limit exceeded.
    TooManyRequests(String),
    /// 500 — anything else: a multi-operation that is not allowed, an adapter
    /// that lacks a capability, a provider failure.
    Internal(String),
    /// 503 — a dependency is temporarily unavailable.
    Unavailable(String),
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

    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Error::Unauthorized(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Error::Forbidden(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Error::NotFound(msg.into())
    }

    pub fn method_not_allowed(msg: impl Into<String>) -> Self {
        Error::MethodNotAllowed(msg.into())
    }

    pub fn conflict(msg: impl Into<String>) -> Self {
        Error::Conflict(msg.into())
    }

    pub fn too_large(msg: impl Into<String>) -> Self {
        Error::TooLarge(msg.into())
    }

    pub fn unprocessable(msg: impl Into<String>) -> Self {
        Error::Unprocessable(msg.into())
    }

    pub fn too_many_requests(msg: impl Into<String>) -> Self {
        Error::TooManyRequests(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }

    pub fn unavailable(msg: impl Into<String>) -> Self {
        Error::Unavailable(msg.into())
    }

    /// HTTP status, following the mapping uflite's REST layer applies.
    pub fn status(&self) -> u16 {
        match self {
            Error::BadRequest(_) => 400,
            Error::Unauthorized(_) => 401,
            Error::Forbidden(_) => 403,
            Error::NotFound(_) => 404,
            Error::MethodNotAllowed(_) => 405,
            Error::Conflict(_) => 409,
            Error::TooLarge(_) => 413,
            Error::Unprocessable(_) => 422,
            Error::TooManyRequests(_) => 429,
            Error::Internal(_) | Error::Query(_) => 500,
            Error::Unavailable(_) => 503,
        }
    }

    /// The JavaScript `error.name` the REST layer would see.
    ///
    /// The five variants uflite also has keep their JavaScript names; the
    /// platform-only ones have no JavaScript counterpart and use [`wire_name`].
    ///
    /// [`wire_name`]: Error::wire_name
    pub fn name(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "TypeError",
            Error::NotFound(_) => "NotFound",
            Error::TooLarge(_) => "RangeError",
            Error::Internal(_) | Error::Query(_) => "Error",
            Error::Unauthorized(_)
            | Error::Forbidden(_)
            | Error::MethodNotAllowed(_)
            | Error::Conflict(_)
            | Error::Unprocessable(_)
            | Error::TooManyRequests(_)
            | Error::Unavailable(_) => self.wire_name(),
        }
    }

    /// The platform contract's `name`.
    pub fn wire_name(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "BadRequest",
            Error::Unauthorized(_) => "NotAuthenticated",
            Error::Forbidden(_) => "Forbidden",
            Error::NotFound(_) => "NotFound",
            Error::MethodNotAllowed(_) => "MethodNotAllowed",
            Error::Conflict(_) => "Conflict",
            // Not in the contract's table; 413 is the status it implies.
            Error::TooLarge(_) => "PayloadTooLarge",
            Error::Unprocessable(_) => "Unprocessable",
            Error::TooManyRequests(_) => "TooManyRequests",
            Error::Internal(_) | Error::Query(_) => "GeneralError",
            Error::Unavailable(_) => "Unavailable",
        }
    }

    /// The platform contract's `className`.
    pub fn class_name(&self) -> &'static str {
        match self {
            Error::BadRequest(_) => "bad-request",
            Error::Unauthorized(_) => "not-authenticated",
            Error::Forbidden(_) => "forbidden",
            Error::NotFound(_) => "not-found",
            Error::MethodNotAllowed(_) => "method-not-allowed",
            Error::Conflict(_) => "conflict",
            Error::TooLarge(_) => "payload-too-large",
            Error::Unprocessable(_) => "unprocessable",
            Error::TooManyRequests(_) => "too-many-requests",
            Error::Internal(_) | Error::Query(_) => "general",
            Error::Unavailable(_) => "service-unavailable",
        }
    }

    /// The platform contract's error body:
    ///
    /// ```json
    /// { "name": "Conflict", "message": "…", "code": 409,
    ///   "className": "conflict", "errors": {} }
    /// ```
    ///
    /// No stack trace, no provider detail, no `status` field — `code` is the
    /// status.
    pub fn to_wire(&self) -> Value {
        let mut o = mosql::Object::new();
        o.set("name", Value::str(self.wire_name()));
        o.set("message", Value::str(self.to_string()));
        o.set("code", Value::Int(self.status() as i64));
        o.set("className", Value::str(self.class_name()));
        o.set("errors", Value::object());
        Value::Object(o)
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
            Error::BadRequest(m)
            | Error::Unauthorized(m)
            | Error::Forbidden(m)
            | Error::NotFound(m)
            | Error::MethodNotAllowed(m)
            | Error::Conflict(m)
            | Error::TooLarge(m)
            | Error::Unprocessable(m)
            | Error::TooManyRequests(m)
            | Error::Internal(m)
            | Error::Unavailable(m) => f.write_str(m),
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
    /// The JSON body uflite's REST layer serialises for this error. The platform
    /// contract's body is [`Error::to_wire`].
    fn from(e: Error) -> Value {
        let mut o = mosql::Object::new();
        o.set("name", Value::str(e.name()));
        o.set("message", Value::str(e.to_string()));
        o.set("code", Value::Int(e.status() as i64));
        o.set("status", Value::Int(e.status() as i64));
        Value::Object(o)
    }
}
