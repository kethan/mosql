//! The outbound HTTP client, over `fetch`.
//!
//! The engine reaches OAuth providers, OpenID discovery documents and key
//! sets over HTTP. On a server that is a socket; in a Worker it is
//! `fetch`, which is a promise. So the client asks the host to make the
//! request and waits for the reply, exactly as the store asks the host to
//! run a statement.

use std::sync::Arc;

use mosql::Value;
use nanobase::httpclient::{ApiResult, AsyncHttpClient, HttpRequest, HttpResponse};

use crate::bus::{BoxFuture, Bus};

/// A client that makes its requests through the host.
pub struct FetchClient {
    bus: Bus,
}

impl FetchClient {
    /// A client that asks the host to make its requests.
    pub fn new(bus: Bus) -> Arc<FetchClient> {
        Arc::new(FetchClient { bus })
    }
}

impl AsyncHttpClient for FetchClient {
    fn request<'a>(&'a self, request: &'a HttpRequest) -> BoxFuture<'a, ApiResult<HttpResponse>> {
        Box::pin(async move {
            let mut op = mosql::Object::new();
            op.set("kind", Value::str("fetch"));
            op.set("method", Value::str(&request.method));
            op.set("url", Value::str(&request.url));
            let headers = request
                .headers
                .iter()
                .map(|(name, value)| Value::Array(vec![Value::str(name), Value::str(value)]))
                .collect();
            op.set("headers", Value::Array(headers));
            op.set(
                "body",
                Value::str(String::from_utf8_lossy(&request.body).into_owned()),
            );
            let answer = self
                .bus
                .call(Value::Object(op))
                .await
                .map_err(nanobase::http::ApiError::internal)?;
            if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
                return Err(nanobase::http::ApiError::internal(message.to_string()));
            }
            let status = answer
                .get("status")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .max(0.0) as u16;
            let body = answer
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .as_bytes()
                .to_vec();
            Ok(HttpResponse { status, body })
        })
    }
}
