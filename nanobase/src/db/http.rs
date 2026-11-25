//! A database behind an HTTP API.
//!
//! This is the serverless shape: the platform does not open a database, it
//! asks a service that has one. The service speaks the protocol in
//! [`crate::db`] — one JSON request per operation, a JSON reply per
//! operation — so it can be a database's own HTTP API, a thin proxy in
//! front of one, or the reference server in [`crate::db::DbServer`].
//!
//! The transport is the host's choice, the same as everywhere else in the
//! platform: a plain `std::net` client for `http://`, and `curl` when the
//! service is behind `https://`.

use std::sync::Arc;

use mosql::Value;
use uflite::error::{Error, Result};

use crate::db::{AppDb, DbRequest, DbResponse, Row};
use crate::httpclient::{CurlHttpClient, HttpClient, HttpRequest};

/// Rows in a remote database, over HTTP.
pub struct HttpDb {
    /// The service's base URL, without a trailing slash.
    base: String,
    /// The path a request is posted to.
    path: String,
    /// A bearer token, if the service wants one.
    token: Option<String>,
    client: Arc<dyn HttpClient>,
}

impl HttpDb {
    /// A database over `base`, posting requests to `{base}/db`.
    pub fn new(base: impl Into<String>, client: Arc<dyn HttpClient>) -> Self {
        let base = base.into();
        let base = base.trim_end_matches('/').to_string();
        HttpDb {
            base,
            path: "/db".to_string(),
            token: None,
            client,
        }
    }

    /// A database over `base`, posting requests to `path`.
    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Send a bearer token with every request.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// The service's base URL.
    pub fn base(&self) -> &str {
        &self.base
    }

    /// Send one request and read the reply.
    fn call(&self, request: &DbRequest) -> Result<DbResponse> {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(token) = &self.token {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        let http = HttpRequest {
            method: "POST".to_string(),
            url: format!("{}{}", self.base, self.path),
            headers,
            body: mosql::to_json(&request.to_value()).into_bytes(),
        };
        let response = self.client.request(&http).map_err(|e| {
            Error::internal(format!("the database at {} did not answer: {e}", self.base))
        })?;
        let text = String::from_utf8_lossy(&response.body).to_string();
        let value = mosql::parse_json(&text)
            .map_err(|e| Error::internal(format!("the database sent invalid JSON: {e}")))?;
        if response.status >= 400 {
            let message = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or(text.trim())
                .to_string();
            return Err(Error::internal(format!(
                "the database answered {}: {message}",
                response.status
            )));
        }
        let response = DbResponse::from_value(&value).map_err(|e| {
            Error::internal(format!(
                "the database sent an answer this client cannot read: {e}"
            ))
        })?;
        if let Some(error) = response.error {
            return Err(Error::internal(error));
        }
        Ok(response)
    }
}

impl AppDb for HttpDb {
    fn kind(&self) -> &'static str {
        "http"
    }

    fn collections(&self) -> Result<Vec<String>> {
        let request = DbRequest::new(crate::db::DbOp::Collections, "");
        Ok(self.call(&request)?.collections)
    }

    fn rows(&self, collection: &str) -> Result<Vec<Row>> {
        let request = DbRequest::new(crate::db::DbOp::Rows, collection);
        Ok(self.call(&request)?.rows)
    }

    fn find(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        let mut request = DbRequest::new(crate::db::DbOp::Find, collection);
        request.query = query.clone();
        request.limit = limit;
        request.offset = offset;
        Ok(self.call(&request)?.rows)
    }

    fn find_one(&self, collection: &str, query: &Value) -> Result<Option<Row>> {
        let mut request = DbRequest::new(crate::db::DbOp::FindOne, collection);
        request.query = query.clone();
        Ok(self.call(&request)?.rows.into_iter().next())
    }

    fn count(&self, collection: &str, query: &Value) -> Result<u64> {
        let mut request = DbRequest::new(crate::db::DbOp::Count, collection);
        request.query = query.clone();
        Ok(self.call(&request)?.count)
    }

    fn insert(&self, collection: &str, row: Row) -> Result<Row> {
        let mut request = DbRequest::new(crate::db::DbOp::Insert, collection);
        request.data = row;
        Ok(self
            .call(&request)?
            .rows
            .into_iter()
            .next()
            .unwrap_or(Value::Null))
    }

    fn update(&self, collection: &str, query: &Value, changes: &Value) -> Result<Option<Row>> {
        let mut request = DbRequest::new(crate::db::DbOp::Update, collection);
        request.query = query.clone();
        request.data = changes.clone();
        Ok(self.call(&request)?.rows.into_iter().next())
    }

    fn remove(&self, collection: &str, query: &Value) -> Result<u64> {
        let mut request = DbRequest::new(crate::db::DbOp::Remove, collection);
        request.query = query.clone();
        Ok(self.call(&request)?.count)
    }
}

/// A database over `base`, with TLS through `curl`.
pub fn https(base: impl Into<String>) -> HttpDb {
    HttpDb::new(base, Arc::new(CurlHttpClient))
}
