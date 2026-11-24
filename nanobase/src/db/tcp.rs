//! A database service on a socket.
//!
//! The same protocol as the HTTP adapter — [`crate::db`] — carried over a
//! plain socket: one JSON request per line, one JSON reply per line. It is
//! the shape a database service speaks when it is on the same network as
//! the platform and does not want an HTTP stack in front of it.
//!
//! A connection is opened per request. The engine's calls are short and
//! infrequent, and a pool would need a reaper for connections a peer
//! dropped without saying so.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

use mosql::Value;
use uflite::error::{Error, Result};

use crate::db::{AppDb, DbRequest, DbResponse, Row};

/// Rows in a remote database, over TCP.
pub struct TcpDb {
    address: String,
    token: Option<String>,
    timeout: Duration,
}

impl TcpDb {
    /// A database talking to `host:port`.
    pub fn new(address: impl Into<String>) -> Self {
        TcpDb {
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

    /// The address this database talks to.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Send one request and read the reply.
    fn call(&self, request: &DbRequest) -> Result<DbResponse> {
        let mut stream = TcpStream::connect(&self.address).map_err(|e| {
            Error::internal(format!(
                "cannot reach the database at {}: {e}",
                self.address
            ))
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
            .map_err(|e| Error::internal(format!("cannot send to the database: {e}")))?;
        stream.flush().ok();
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|e| Error::internal(format!("cannot read from the database: {e}")))?,
        );
        let mut reply = String::new();
        reader
            .read_line(&mut reply)
            .map_err(|e| Error::internal(format!("cannot read from the database: {e}")))?;
        if reply.trim().is_empty() {
            return Err(Error::internal("the database closed the connection"));
        }
        let value = mosql::parse_json(reply.trim())
            .map_err(|e| Error::internal(format!("the database sent invalid JSON: {e}")))?;
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

impl AppDb for TcpDb {
    fn kind(&self) -> &'static str {
        "tcp"
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
