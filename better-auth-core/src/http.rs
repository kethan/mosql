//! The HTTP engine, shared with the platform crate.
//!
//! The implementation lives in `nanobase::http`, so the platform's API and
//! this engine speak the same protocol. This module re-exports it, and the
//! rest of the engine keeps using `crate::http::…` unchanged.

pub use nanobase::http::{
    form_encode, parse_query, percent_decode, percent_encode, read_request, serve, serve_on,
    status_text, write_response, ApiError, Incoming, Request, Response, MAX_BODY_BYTES,
};
