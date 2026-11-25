//! The engine, and the two routes it serves.
//!
//! The Worker serves the authentication API — every route in
//! `better_auth_core::routes` — and, when a bucket is bound, a small blob
//! API beside it. Both are ordinary HTTP: the host hands over a request,
//! the engine answers.
//!
//! The configuration comes from the host too, because a Worker has no
//! environment of its own: the base URL, the signing key and the allowed
//! origins are whatever the deployment says they are.

use std::sync::Arc;

use better_auth_core::auth::{Auth, AuthConfig};
use better_auth_core::http::{Request, Response};
use better_auth_core::routes;
use mosql::Value;

use crate::blob::{key_of, Blobs, PREFIX as BLOB_PREFIX};
use crate::bus::Bus;
use crate::cache::CachedStore;
use crate::d1::D1Store;
use crate::fetch::FetchClient;

/// The engine, and what it was built with.
pub struct Engine {
    /// The authentication engine.
    pub auth: Arc<Auth>,
    /// The blob adapter, when the deployment bound a bucket.
    pub blobs: Option<Blobs>,
}

/// Build the engine from the host's configuration.
///
/// The configuration is a JSON object:
///
/// ```json
/// {
///   "baseUrl": "https://auth.example.com",
///   "secret": "the signing key",
///   "allowedOrigins": ["https://app.example.com"],
///   "cache": true,
///   "blobs": true
/// }
/// ```
pub fn build(config: &Value, bus: Bus) -> Result<Engine, String> {
    let base_url = config
        .get("baseUrl")
        .and_then(|v| v.as_str())
        .ok_or("the configuration has no baseUrl")?;
    let secret = config
        .get("secret")
        .and_then(|v| v.as_str())
        .ok_or("the configuration has no secret")?;
    let mut auth_config = AuthConfig::new(base_url, secret.as_bytes().to_vec());
    if let Some(origins) = config.get("allowedOrigins").and_then(|v| v.as_array()) {
        auth_config.allowed_origins = origins
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
    }
    if let Some(flag) = config
        .get("requireEmailVerification")
        .and_then(|v| v.as_bool())
    {
        auth_config.require_email_verification = flag;
    }
    if let Some(flag) = config.get("allowAnonymous").and_then(|v| v.as_bool()) {
        auth_config.allow_anonymous = flag;
    }
    if let Some(ttl) = config.get("accessTtlSecs").and_then(|v| v.as_f64()) {
        auth_config.access_ttl_secs = ttl as i64;
    }
    if let Some(ttl) = config.get("refreshTtlSecs").and_then(|v| v.as_f64()) {
        auth_config.refresh_ttl_secs = ttl as i64;
    }

    let database = Arc::new(D1Store::new(bus.clone()));
    let wants_cache = config
        .get("cache")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let store: Arc<dyn better_auth_core::store::AuthStore> = if wants_cache {
        CachedStore::new(database, bus.clone())
    } else {
        database
    };

    let auth = Auth::new(store, auth_config).with_http(FetchClient::new(bus.clone()));

    let wants_blobs = config
        .get("blobs")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let blobs = wants_blobs.then(|| Blobs::new(bus));

    Ok(Engine {
        auth: Arc::new(auth),
        blobs,
    })
}

/// Answer one request.
pub async fn handle(engine: &Engine, request: Request) -> Response {
    if request.path.starts_with(BLOB_PREFIX) {
        return match &engine.blobs {
            Some(blobs) => blob(blobs, &request).await,
            None => Response::json(
                404,
                &error_value("NOT_FOUND", "this deployment has no blob store"),
            ),
        };
    }
    routes::route(engine.auth.clone(), request).await
}

/// The blob routes.
async fn blob(blobs: &Blobs, request: &Request) -> Response {
    let Some(key) = key_of(&request.path) else {
        return Response::json(404, &error_value("NOT_FOUND", "no blob was named"));
    };
    match request.method.as_str() {
        "GET" => match blobs.get(key).await {
            Ok(Some(blob)) => {
                let mut response = Response {
                    status: 200,
                    headers: Vec::new(),
                    body: blob.body,
                };
                response.headers.push((
                    "content-type".into(),
                    blob.content_type
                        .unwrap_or_else(|| "application/octet-stream".into()),
                ));
                response
            }
            Ok(None) => Response::json(404, &error_value("NOT_FOUND", "no such blob")),
            Err(message) => Response::json(502, &error_value("BLOB_FAILED", &message)),
        },
        "PUT" | "POST" => {
            let content_type = request.header("content-type").map(str::to_string);
            match blobs.put(key, &request.body, content_type.as_deref()).await {
                Ok(()) => {
                    let mut value = mosql::Object::new();
                    value.set("key", Value::str(key));
                    value.set("size", Value::Int(request.body.len() as i64));
                    Response::json(201, &Value::Object(value))
                }
                Err(message) => Response::json(502, &error_value("BLOB_FAILED", &message)),
            }
        }
        "DELETE" => match blobs.delete(key).await {
            Ok(()) => {
                let mut value = mosql::Object::new();
                value.set("key", Value::str(key));
                value.set("deleted", Value::Bool(true));
                Response::json(200, &Value::Object(value))
            }
            Err(message) => Response::json(502, &error_value("BLOB_FAILED", &message)),
        },
        _ => Response::json(
            405,
            &error_value("METHOD_NOT_ALLOWED", "blobs take GET, PUT and DELETE"),
        ),
    }
}

/// The failure shape every route uses.
fn error_value(code: &str, message: &str) -> Value {
    let mut value = mosql::Object::new();
    value.set("code", Value::str(code));
    value.set("message", Value::str(message));
    Value::Object(value)
}

/// Read a request from the host's JSON.
pub fn parse_request(value: &Value) -> Result<Request, String> {
    let method = value
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .to_ascii_uppercase();
    let path = value
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or("the request has no path")?;
    let path = better_auth_core::http::percent_decode(path);
    let mut query = Vec::new();
    if let Some(items) = value.get("query").and_then(|v| v.as_array()) {
        for item in items {
            if let Some(pair) = item.as_array() {
                if let (Some(name), Some(val)) = (pair.first(), pair.get(1)) {
                    query.push((
                        name.as_str().unwrap_or_default().to_string(),
                        val.as_str().unwrap_or_default().to_string(),
                    ));
                }
            }
        }
    }
    let mut headers = Vec::new();
    if let Some(items) = value.get("headers").and_then(|v| v.as_array()) {
        for item in items {
            if let Some(pair) = item.as_array() {
                if let (Some(name), Some(val)) = (pair.first(), pair.get(1)) {
                    headers.push((
                        name.as_str().unwrap_or_default().to_ascii_lowercase(),
                        val.as_str().unwrap_or_default().to_string(),
                    ));
                }
            }
        }
    }
    let body = value
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .as_bytes()
        .to_vec();
    let remote = value
        .get("remote")
        .and_then(|v| v.as_str())
        .and_then(|text| text.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:0".parse().expect("a socket address"));
    Ok(Request {
        method,
        path,
        query,
        headers,
        body,
        remote,
    })
}

/// Write a response as the host's JSON.
pub fn encode_response(response: &Response) -> Value {
    let mut value = mosql::Object::new();
    value.set("status", Value::Int(response.status as i64));
    let headers = response
        .headers
        .iter()
        .map(|(name, val)| Value::Array(vec![Value::str(name), Value::str(val)]))
        .collect();
    value.set("headers", Value::Array(headers));
    value.set(
        "body",
        Value::str(String::from_utf8_lossy(&response.body).into_owned()),
    );
    Value::Object(value)
}
