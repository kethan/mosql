//! One error type for the whole engine, in the shape clients see.
//!
//! Every failure leaves the engine as `{"code": ERROR_CODE, "message": "…"}` with
//! an HTTP status that says which kind of failure it is. The codes are Better
//! Auth's, so a client written against it branches the same way here.
//!
//! The type itself lives in the platform crate, next to the HTTP engine that
//! writes it out, so the platform's API and this engine cannot drift apart.

/// A failure with a status, a machine-readable code and a human-readable line.
pub use nanobase::http::ApiError as AuthError;

pub type AuthResult<T> = Result<T, AuthError>;
