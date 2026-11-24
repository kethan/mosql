//! The command line's API client.
//!
//! Every command can reach a running platform over HTTP instead of opening
//! the platform database itself: `--url http://host:8090`, or `NANOBASE_URL`.
//! When a URL is given the command is a thin client — it sends a request and
//! prints what comes back — so an operator and a script reach exactly the
//! same API the platform serves.

use crate::httpclient::{CurlHttpClient, HttpClient, HttpRequest, PlainHttpClient};
use mosql::Value;

/// A client for a running platform.
pub struct ApiClient {
    base: String,
    token: Option<String>,
}

impl ApiClient {
    /// A client for `base`, if the environment names one.
    ///
    /// `--url` wins over `NANOBASE_URL`; without either there is no client,
    /// and the command works on the local database instead.
    pub fn from_options(url: Option<&str>, token: Option<&str>) -> Option<ApiClient> {
        let base = url
            .map(str::to_string)
            .or_else(|| std::env::var("NANOBASE_URL").ok())
            .filter(|u| !u.trim().is_empty())?;
        let token = token
            .map(str::to_string)
            .or_else(|| std::env::var("NANOBASE_TOKEN").ok())
            .filter(|t| !t.trim().is_empty());
        Some(ApiClient {
            base: base.trim_end_matches('/').to_string(),
            token,
        })
    }

    /// The base URL this client talks to.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Send a request and return the reply's JSON.
    pub fn request(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
        let url = format!("{}{}", self.base, path);
        let mut headers = vec![("accept".to_string(), "application/json".to_string())];
        if let Some(token) = &self.token {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        let mut payload = Vec::new();
        if let Some(body) = body {
            headers.push(("content-type".to_string(), "application/json".to_string()));
            payload = mosql::to_json(body).into_bytes();
        }
        let request = HttpRequest {
            method: method.to_ascii_uppercase(),
            url,
            headers,
            body: payload,
        };
        // `https://` needs TLS, which only curl brings.
        let client: Box<dyn HttpClient> = if request.url.starts_with("https://") {
            Box::new(CurlHttpClient)
        } else {
            Box::new(PlainHttpClient)
        };
        let response = client.request(&request).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&response.body).to_string();
        let value = if text.trim().is_empty() {
            Value::Object(mosql::Object::new())
        } else {
            mosql::parse_json(&text).map_err(|e| format!("the server sent invalid JSON: {e}"))?
        };
        if response.status >= 400 {
            let message = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("the request failed")
                .to_string();
            let code = value
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("ERROR")
                .to_string();
            return Err(format!("{message} ({code})"));
        }
        Ok(value)
    }
}
