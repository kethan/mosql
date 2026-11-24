//! One error type for the whole engine, in the shape clients see.
//!
//! Every failure leaves the engine as `{"code": ERROR_CODE, "message": "…"}` with
//! an HTTP status that says which kind of failure it is. The codes are Better
//! Auth's, so a client written against it branches the same way here.

use mosql::Value;
use uflite::error::Error as ServiceError;

pub type AuthResult<T> = Result<T, AuthError>;

/// A failure with a status, a machine-readable code and a human-readable line.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl AuthError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        AuthError {
            status,
            code,
            message: message.into(),
        }
    }

    /// 400 — the request cannot be acted on as sent.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(400, code, message)
    }

    /// 401 — no or invalid credentials.
    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(401, code, message)
    }

    /// 403 — authenticated, not allowed.
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(403, code, message)
    }

    /// 404 — the thing named does not exist.
    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(404, code, message)
    }

    /// 500 — the engine or its store failed, not the caller.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "UNKNOWN_ERROR", message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::bad_request("INVALID_REQUEST", message)
    }

    pub fn invalid_token(message: impl Into<String>) -> Self {
        Self::unauthorized("INVALID_TOKEN", message)
    }

    pub fn unauthorized_plain() -> Self {
        Self::unauthorized("UNAUTHORIZED", "unauthorized")
    }

    /// The wire form.
    pub fn to_json(&self) -> Value {
        let mut body = mosql::Object::new();
        body.set("code", Value::str(self.code));
        body.set("message", Value::str(self.message.clone()));
        Value::Object(body)
    }
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for AuthError {}

impl From<ServiceError> for AuthError {
    fn from(e: ServiceError) -> Self {
        match e.status() {
            409 => AuthError::new(409, "CONFLICT", e.to_string()),
            _ => AuthError::internal(format!("store: {e}")),
        }
    }
}

impl From<mosql::Error> for AuthError {
    fn from(e: mosql::Error) -> Self {
        AuthError::invalid_request(format!("not valid JSON: {e}"))
    }
}

impl From<std::io::Error> for AuthError {
    fn from(e: std::io::Error) -> Self {
        AuthError::internal(format!("i/o: {e}"))
    }
}
