//! A store that keeps its records in a remote database over HTTP.
//!
//! This is the serverless shape: the engine does not open a database, it
//! asks a service that has one. The service speaks the protocol in
//! [`crate::query`] — one JSON request per operation — so it can be a
//! database's own HTTP API, a thin proxy in front of one, or the reference
//! server in [`crate::query_server`].
//!
//! The transport is the host's choice, the same as for OAuth: a plain
//! `std::net` client for `http://`, and `curl` when the service is behind
//! `https://`.

use std::sync::Arc;

use crate::auth::{HttpClient, HttpRequest};
use crate::error::{AuthError, AuthResult};
use crate::models::RecordMap;
use crate::query::{QueryOp, QueryRequest, QueryResponse};
use crate::store::{AuthStore, BoxFuture, FindManyQuery, WhereClause};

/// A store reached over HTTP.
pub struct HttpAuthStore {
    /// The service's base URL, without a trailing slash.
    base: String,
    /// The path the query is posted to.
    path: String,
    /// A bearer token, if the service wants one.
    token: Option<String>,
    client: Arc<dyn HttpClient>,
}

impl HttpAuthStore {
    /// A store over `base`, posting queries to `{base}/query`.
    pub fn new(base: impl Into<String>, client: Arc<dyn HttpClient>) -> Self {
        let base = base.into();
        let base = base.trim_end_matches('/').to_string();
        HttpAuthStore {
            base,
            path: "/query".to_string(),
            token: None,
            client,
        }
    }

    /// A store over `base`, posting queries to `path`.
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

    /// Send one query and read the reply.
    fn query(&self, request: &QueryRequest) -> AuthResult<QueryResponse> {
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
        let response = self.client.request(&http)?;
        if response.status >= 400 {
            let text = String::from_utf8_lossy(&response.body);
            let message = mosql::parse_json(&text)
                .ok()
                .and_then(|v| {
                    v.get("message")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| text.trim().to_string());
            return Err(AuthError::internal(format!(
                "the store answered {}: {message}",
                response.status
            )));
        }
        let value = response.json()?;
        QueryResponse::from_value(&value)
    }
}

impl AuthStore for HttpAuthStore {
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::Create, model).data(data);
            let response = self.query(&request)?;
            response
                .rows
                .into_iter()
                .next()
                .ok_or_else(|| AuthError::internal("the store created nothing"))
        })();
        Box::pin(async move { result })
    }

    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::FindOne, model).where_clause(r#where.to_vec());
            let response = self.query(&request)?;
            Ok(response.rows.into_iter().next())
        })();
        Box::pin(async move { result })
    }

    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::FindMany, model).query(query);
            let response = self.query(&request)?;
            Ok(response.rows)
        })();
        Box::pin(async move { result })
    }

    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::Count, model).where_clause(r#where.to_vec());
            let response = self.query(&request)?;
            Ok(response.count)
        })();
        Box::pin(async move { result })
    }

    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::Update, model)
                .where_clause(r#where.to_vec())
                .data(update);
            let response = self.query(&request)?;
            Ok(response.rows.into_iter().next())
        })();
        Box::pin(async move { result })
    }

    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::UpdateMany, model)
                .where_clause(r#where.to_vec())
                .data(update);
            let response = self.query(&request)?;
            Ok(response.count)
        })();
        Box::pin(async move { result })
    }

    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>> {
        let result = (|| {
            let request = QueryRequest::new(QueryOp::Delete, model).where_clause(r#where.to_vec());
            self.query(&request)?;
            Ok(())
        })();
        Box::pin(async move { result })
    }

    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        let result = (|| {
            let request =
                QueryRequest::new(QueryOp::DeleteMany, model).where_clause(r#where.to_vec());
            let response = self.query(&request)?;
            Ok(response.count)
        })();
        Box::pin(async move { result })
    }
}
