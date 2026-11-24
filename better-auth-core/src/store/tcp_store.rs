//! A store that keeps its records in a remote database over TCP.
//!
//! The same protocol as the HTTP store — [`crate::query`] — carried over a
//! plain socket: one JSON request per line, one JSON reply per line. It is
//! the shape a database service speaks when it is on the same network as
//! the engine and does not want an HTTP stack in front of it.
//!
//! A connection is opened per request. That is deliberate: the engine's
//! calls are short and infrequent, and a pool would need a reaper for
//! connections a peer dropped without saying so.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::error::{AuthError, AuthResult};
use crate::models::RecordMap;
use crate::query::{QueryOp, QueryRequest, QueryResponse};
use crate::store::{AuthStore, BoxFuture, FindManyQuery, WhereClause};

/// A store reached over TCP.
pub struct TcpAuthStore {
    address: String,
    token: Option<String>,
    timeout: Duration,
}

impl TcpAuthStore {
    /// A store talking to `host:port`.
    pub fn new(address: impl Into<String>) -> Self {
        TcpAuthStore {
            address: address.into(),
            token: None,
            timeout: Duration::from_secs(15),
        }
    }

    /// Send a bearer token with every request.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// How long to wait for the service.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The address this store talks to.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Send one query and read the reply.
    fn query(&self, request: &QueryRequest) -> AuthResult<QueryResponse> {
        let mut stream = TcpStream::connect(&self.address).map_err(|e| {
            AuthError::internal(format!("cannot reach the store at {}: {e}", self.address))
        })?;
        stream.set_read_timeout(Some(self.timeout)).ok();
        stream.set_write_timeout(Some(self.timeout)).ok();
        let mut line = mosql::to_json(&request.to_value());
        line.push('\n');
        if let Some(token) = &self.token {
            // The token rides in a header line before the request, the way
            // HTTP would carry it, so the service can tell one caller from
            // another.
            let mut head = format!("authorization: Bearer {token}\n");
            head.push_str(&line);
            line = head;
        }
        stream
            .write_all(line.as_bytes())
            .map_err(|e| AuthError::internal(format!("cannot send to the store: {e}")))?;
        stream.flush().ok();
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|e| AuthError::internal(format!("cannot read from the store: {e}")))?,
        );
        let mut reply = String::new();
        reader
            .read_line(&mut reply)
            .map_err(|e| AuthError::internal(format!("cannot read from the store: {e}")))?;
        if reply.trim().is_empty() {
            return Err(AuthError::internal("the store closed the connection"));
        }
        let value = mosql::parse_json(reply.trim())
            .map_err(|e| AuthError::internal(format!("the store sent invalid JSON: {e}")))?;
        // An error reply is `{"error": "…"}`; a good one has rows or a count.
        if let Some(message) = value.get("error").and_then(|v| v.as_str()) {
            return Err(AuthError::internal(message.to_string()));
        }
        QueryResponse::from_value(&value)
    }
}

impl AuthStore for TcpAuthStore {
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
