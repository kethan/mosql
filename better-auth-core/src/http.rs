//! The HTTP engine, shared with the platform crate.
//!
//! The implementation lives in `nanobase::http`, so the platform's API and
//! this engine speak the same protocol. This module re-exports it, and the
//! rest of the engine keeps using `crate::http::…` unchanged.

pub use nanobase::http::{
    form_encode, parse_query, percent_decode, percent_encode, status_text, ApiError, Request,
    Response, MAX_BODY_BYTES,
};
// The socket half of the platform's HTTP module. A Worker is handed a
// request and returns a response; there is no connection to read from.
#[cfg(feature = "native")]
pub use nanobase::http::{read_request, serve, serve_on, write_response, Incoming};
