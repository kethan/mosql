//! The app database: where an app's own rows live.
//!
//! An app's document says which engine its rows are in — a local SQLite
//! file, memory, a serverless database behind an HTTP API, a database
//! service on a socket, and later a SQL server. The rows themselves are
//! collections of documents, the same shape everywhere, so the code that
//! serves an app does not change when the engine does.
//!
//! [`AppDb`] is that seam: the operations an app needs, and nothing else.
//! Four adapters ship with it:
//!
//! * [`memory::MemoryDb`] — in the process, and gone when it stops.
//! * [`sqlite::SqliteDb`] — a local file, one table per collection.
//! * [`http::HttpDb`] — a serverless database behind an HTTP API.
//! * [`provider::ProviderDb`] — the database itself, in the protocol it
//!   speaks: libSQL, D1, PostgreSQL, MySQL.
//! * [`tcp::TcpDb`] — a database service on a socket.
//!
//! The two remote adapters speak one protocol — a JSON request per
//! operation, a JSON reply per operation — so a service can answer over
//! HTTP, over TCP, or both. [`DbServer`] is the reference implementation
//! of the other end, over any [`AppDb`].
//!
//! The provider adapter is the odd one out: it speaks the *database's*
//! protocol, not this platform's. That is what lets an app's rows live in
//! Neon, in TiDB, in PlanetScale, in Turso or in D1 without a database
//! service in front of them — which is the whole point of a serverless
//! deployment.
//!
//! The filters are documents, not SQL: `{"done": false}` finds the rows
//! that are not done. The matcher is `mosql`'s, the same one the in-process
//! engine uses, so a query means the same thing in every adapter.

pub mod factory;
#[cfg(feature = "curl")]
pub mod http;
pub mod memory;
#[cfg(feature = "curl")]
pub mod provider;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "native")]
pub mod tcp;

use std::sync::Arc;

use mosql::Value;
use uflite::error::Result;

use crate::http::{ApiError, Request, Response};

/// One row. A document, like everything else in an app.
pub type Row = Value;

/// A collection of rows an app's rows live in.
///
/// The operations are the ones an app's runtime needs: list, find, insert,
/// update, remove. There is no transaction: an app's engine may be a
/// serverless database that cannot hold one open.
pub trait AppDb: Send + Sync {
    /// What the engine is called: `memory`, `sqlite`, `http` or `tcp`.
    fn kind(&self) -> &'static str;

    /// The collections that exist.
    fn collections(&self) -> Result<Vec<String>>;

    /// Every row of a collection.
    fn rows(&self, collection: &str) -> Result<Vec<Row>>;

    /// The rows matching a filter.
    fn find(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>>;

    /// The first row matching a filter.
    fn find_one(&self, collection: &str, query: &Value) -> Result<Option<Row>>;

    /// How many rows match a filter.
    fn count(&self, collection: &str, query: &Value) -> Result<u64>;

    /// Add a row, giving it an id if it has none.
    fn insert(&self, collection: &str, row: Row) -> Result<Row>;

    /// Change the first row matching a filter.
    fn update(&self, collection: &str, query: &Value, changes: &Value) -> Result<Option<Row>>;

    /// Remove every row matching a filter; returns how many went.
    fn remove(&self, collection: &str, query: &Value) -> Result<u64>;
}

/// A collection name reduced to characters that are safe in a table name.
///
/// A collection name is a name an app chose, so it is quoted rather than
/// trusted — and reduced, so that a name cannot become a statement.
pub(crate) fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

/// Apply a change document to a row.
///
/// A change is either a set of fields to merge — `{"done": true}` — or an
/// update document with operators, `{"$set": {"done": true}}`. Both are
/// what the JS side accepts, and both mean the same thing here as in the
/// in-process engine: this is the same rule, written out once, so every
/// adapter changes a row the same way.
pub(crate) fn apply_changes(row: &mut Row, changes: &Value) -> Result<()> {
    let Some(object) = changes.as_object() else {
        return Err(uflite::error::Error::internal(
            "a change must be a document",
        ));
    };
    for (op, fields) in object.iter() {
        if op.starts_with('$') {
            let Some(fields) = fields.as_object() else {
                return Err(uflite::error::Error::internal(format!(
                    "{op} requires an object"
                )));
            };
            mosql::memory::apply_update(op, fields, row, false)?;
        } else {
            mosql::memory::set_path(row, op, fields.clone());
        }
    }
    Ok(())
}

/// What a request asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DbOp {
    /// The collections that exist.
    Collections,
    /// Every row of a collection.
    Rows,
    /// The rows matching a filter.
    Find,
    /// The first row matching a filter.
    FindOne,
    /// How many rows match a filter.
    Count,
    /// Add a row.
    Insert,
    /// Change the rows matching a filter.
    Update,
    /// Remove the rows matching a filter.
    Remove,
}

impl DbOp {
    /// The name on the wire.
    pub fn name(&self) -> &'static str {
        match self {
            DbOp::Collections => "collections",
            DbOp::Rows => "rows",
            DbOp::Find => "find",
            DbOp::FindOne => "findOne",
            DbOp::Count => "count",
            DbOp::Insert => "insert",
            DbOp::Update => "update",
            DbOp::Remove => "remove",
        }
    }

    /// The operation a name stands for.
    pub fn from_name(name: &str) -> Option<DbOp> {
        Some(match name {
            "collections" => DbOp::Collections,
            "rows" => DbOp::Rows,
            "find" => DbOp::Find,
            "findOne" => DbOp::FindOne,
            "count" => DbOp::Count,
            "insert" => DbOp::Insert,
            "update" => DbOp::Update,
            "remove" => DbOp::Remove,
            _ => return None,
        })
    }
}

/// One request: an operation, a collection, and whatever it needs.
#[derive(Debug, Clone)]
pub struct DbRequest {
    pub op: DbOp,
    pub collection: String,
    /// The filter, as a document.
    pub query: Value,
    /// The row to add, or the changes to make.
    pub data: Value,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

impl DbRequest {
    pub fn new(op: DbOp, collection: impl Into<String>) -> Self {
        DbRequest {
            op,
            collection: collection.into(),
            query: Value::Object(mosql::Object::new()),
            data: Value::Object(mosql::Object::new()),
            limit: None,
            offset: None,
        }
    }

    /// The request as JSON.
    pub fn to_value(&self) -> Value {
        let mut body = mosql::Object::new();
        body.set("op", Value::str(self.op.name()));
        body.set("collection", Value::str(self.collection.clone()));
        if !self.query.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            body.set("query", self.query.clone());
        }
        if !self.data.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            body.set("data", self.data.clone());
        }
        if let Some(limit) = self.limit {
            body.set("limit", Value::Int(limit as i64));
        }
        if let Some(offset) = self.offset {
            body.set("offset", Value::Int(offset as i64));
        }
        Value::Object(body)
    }

    /// The request from JSON.
    pub fn from_value(value: &Value) -> std::result::Result<DbRequest, ApiError> {
        let op = value
            .get("op")
            .and_then(|v| v.as_str())
            .and_then(DbOp::from_name)
            .ok_or_else(|| ApiError::invalid_request("the request has no known operation"))?;
        let collection = value
            .get("collection")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let mut request = DbRequest::new(op, collection);
        if let Some(query) = value.get("query") {
            request.query = query.clone();
        }
        if let Some(data) = value.get("data") {
            request.data = data.clone();
        }
        request.limit = value
            .get("limit")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64);
        request.offset = value
            .get("offset")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64);
        Ok(request)
    }
}

/// What a reply says.
#[derive(Debug, Clone, Default)]
pub struct DbResponse {
    /// The rows, when the operation produced any.
    pub rows: Vec<Row>,
    /// The count, when the operation produced one.
    pub count: u64,
    /// The names, when the operation listed collections.
    pub collections: Vec<String>,
    /// What went wrong, when something did.
    pub error: Option<String>,
}

impl DbResponse {
    /// A reply with rows.
    pub fn rows(rows: Vec<Row>) -> Self {
        DbResponse {
            count: rows.len() as u64,
            rows,
            ..DbResponse::default()
        }
    }

    /// A reply with a count.
    pub fn count(count: u64) -> Self {
        DbResponse {
            count,
            ..DbResponse::default()
        }
    }

    /// A reply that carries an error instead of rows.
    pub fn failed(message: impl Into<String>) -> Self {
        DbResponse {
            error: Some(message.into()),
            ..DbResponse::default()
        }
    }

    /// The reply as JSON.
    pub fn to_value(&self) -> Value {
        let mut body = mosql::Object::new();
        if !self.rows.is_empty() {
            body.set("rows", Value::Array(self.rows.clone()));
        }
        body.set("count", Value::Int(self.count as i64));
        if !self.collections.is_empty() {
            body.set(
                "collections",
                Value::Array(self.collections.iter().cloned().map(Value::str).collect()),
            );
        }
        if let Some(error) = &self.error {
            body.set("error", Value::str(error.clone()));
        }
        Value::Object(body)
    }

    /// The reply from JSON.
    pub fn from_value(value: &Value) -> std::result::Result<DbResponse, ApiError> {
        let mut response = DbResponse::default();
        if let Some(rows) = value.get("rows").and_then(|v| v.as_array()) {
            response.rows = rows.clone();
        }
        response.count = value
            .get("count")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64)
            .unwrap_or(response.rows.len() as u64);
        if let Some(names) = value.get("collections").and_then(|v| v.as_array()) {
            response.collections = names
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect();
        }
        response.error = value
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(response)
    }
}

/// The other end of the protocol: a server over any database.
///
/// It is what a deployment runs when an app's rows live in a service of
/// their own, and what the remote adapters are tested against.
pub struct DbServer {
    db: Arc<dyn AppDb>,
    /// The token a caller has to offer, when the service wants one.
    token: Option<String>,
}

impl DbServer {
    pub fn new(db: Arc<dyn AppDb>) -> Self {
        DbServer { db, token: None }
    }

    /// Ask callers for a bearer token.
    ///
    /// A request without the right token is refused, whether it arrives
    /// over HTTP or over a socket.
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// The database this server is over.
    pub fn db(&self) -> &Arc<dyn AppDb> {
        &self.db
    }

    /// Whether a caller may in.
    ///
    /// The header is read as HTTP would: `Bearer <token>`.
    pub fn allowed(&self, authorization: Option<&str>) -> bool {
        let offered = authorization
            .and_then(|value| value.strip_prefix("Bearer "))
            .or_else(|| {
                authorization.and_then(|value| {
                    value
                        .split_once(' ')
                        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
                        .map(|(_, token)| token.trim())
                })
            });
        crate::wire::token_ok(self.token.as_deref(), offered)
    }

    /// Run one request and answer it.
    pub fn handle(&self, request: &DbRequest) -> DbResponse {
        let db = Arc::clone(&self.db);
        let collection = request.collection.clone();
        let query = request.query.clone();
        let data = request.data.clone();
        let limit = request.limit;
        let offset = request.offset;
        let result: Result<DbResponse> = match request.op {
            DbOp::Collections => db.collections().map(|names| DbResponse {
                collections: names,
                ..DbResponse::default()
            }),
            DbOp::Rows => db.rows(&collection).map(DbResponse::rows),
            DbOp::Find => db
                .find(&collection, &query, limit, offset)
                .map(DbResponse::rows),
            DbOp::FindOne => db.find_one(&collection, &query).map(|row| match row {
                Some(row) => DbResponse::rows(vec![row]),
                None => DbResponse::default(),
            }),
            DbOp::Count => db.count(&collection, &query).map(DbResponse::count),
            DbOp::Insert => db
                .insert(&collection, data)
                .map(|row| DbResponse::rows(vec![row])),
            DbOp::Update => db.update(&collection, &query, &data).map(|row| match row {
                Some(row) => DbResponse::rows(vec![row]),
                None => DbResponse::default(),
            }),
            DbOp::Remove => db.remove(&collection, &query).map(DbResponse::count),
        };
        match result {
            Ok(response) => response,
            Err(e) => DbResponse::failed(e.to_string()),
        }
    }

    /// Answer an HTTP request: `POST /db` with the request as JSON.
    pub fn handle_http(&self, req: &Request) -> Response {
        if req.method != "POST" {
            return Response::json(
                405,
                &error_value("METHOD_NOT_ALLOWED", "only POST is allowed"),
            );
        }
        if !self.allowed(req.header("authorization")) {
            return Response::json(
                401,
                &error_value("UNAUTHORIZED", "the service wants a bearer token"),
            );
        }
        let body = match req.json() {
            Ok(body) => body,
            Err(e) => return Response::json(400, &error_value("INVALID_REQUEST", &e.message)),
        };
        let request = match DbRequest::from_value(&body) {
            Ok(request) => request,
            Err(e) => return Response::json(400, &error_value("INVALID_REQUEST", &e.message)),
        };
        Response::json(200, &self.handle(&request).to_value())
    }
}

/// Serve the protocol over TCP until the listener fails.
///
/// One connection carries as many requests as the client sends: each line
/// is a request, each reply is a line.
#[cfg(feature = "native")]
pub fn serve_tcp(server: Arc<DbServer>, listener: std::net::TcpListener) -> std::io::Result<()> {
    use std::io::BufReader;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || {
            let Ok(write_half) = stream.try_clone() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut writer = write_half;
            while handle_line(&server, &mut reader, &mut writer) {}
        });
    }
    Ok(())
}

#[cfg(feature = "native")]
/// Answer one request on a connection; returns whether to keep it open.
///
/// A request is a JSON line, optionally preceded by header lines — the
/// token rides in one of them, the way HTTP would carry it.
fn handle_line(
    server: &Arc<DbServer>,
    reader: &mut std::io::BufReader<std::net::TcpStream>,
    writer: &mut std::net::TcpStream,
) -> bool {
    use std::io::Write;
    let frame = match crate::wire::read_frame(reader) {
        Ok(Some(frame)) => frame,
        Ok(None) => return false,
        Err(_) => return false,
    };
    let reply = if !crate::wire::token_ok(server.token.as_deref(), frame.token.as_deref()) {
        error_value("UNAUTHORIZED", "the service wants a bearer token")
    } else {
        match mosql::parse_json(&frame.json) {
            Ok(value) => match DbRequest::from_value(&value) {
                Ok(request) => server.handle(&request).to_value(),
                Err(e) => error_value("INVALID_REQUEST", &e.message),
            },
            Err(e) => error_value("INVALID_REQUEST", &format!("not valid JSON: {e}")),
        }
    };
    let mut out = mosql::to_json(&reply);
    out.push('\n');
    if writer.write_all(out.as_bytes()).is_err() {
        return false;
    }
    let _ = writer.flush();
    true
}

/// The error body the server answers with.
///
/// The message is in `message` for a reader that shows it and in `error`
/// for the database protocol, which carries its failures that way.
fn error_value(code: &str, message: &str) -> Value {
    let mut body = mosql::Object::new();
    body.set("code", Value::str(code));
    body.set("message", Value::str(message));
    body.set("error", Value::str(message));
    Value::Object(body)
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;
    use mosql::Object;

    #[test]
    fn a_request_survives_the_round_trip() {
        let mut query = Object::new();
        query.set("done", Value::Bool(false));
        let mut data = Object::new();
        data.set("title", Value::str("write it down"));
        let request = DbRequest {
            op: DbOp::Update,
            collection: "todos".into(),
            query: Value::Object(query),
            data: Value::Object(data),
            limit: Some(10),
            offset: Some(5),
        };
        let value = request.to_value();
        let back = DbRequest::from_value(&value).expect("parsed");
        assert_eq!(back.op, DbOp::Update);
        assert_eq!(back.collection, "todos");
        assert_eq!(
            back.query.get("done").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            back.data.get("title").and_then(|v| v.as_str()),
            Some("write it down")
        );
        assert_eq!(back.limit, Some(10));
        assert_eq!(back.offset, Some(5));
    }

    #[test]
    fn a_reply_survives_the_round_trip() {
        let mut row = Object::new();
        row.set("id", Value::str("r1"));
        let response = DbResponse {
            rows: vec![Value::Object(row)],
            count: 1,
            collections: vec!["todos".into()],
            error: None,
        };
        let back = DbResponse::from_value(&response.to_value()).expect("parsed");
        assert_eq!(back.rows.len(), 1);
        assert_eq!(back.count, 1);
        assert_eq!(back.collections, vec!["todos".to_string()]);
    }

    #[test]
    fn every_operation_has_a_name() {
        for op in [
            DbOp::Collections,
            DbOp::Rows,
            DbOp::Find,
            DbOp::FindOne,
            DbOp::Count,
            DbOp::Insert,
            DbOp::Update,
            DbOp::Remove,
        ] {
            assert_eq!(DbOp::from_name(op.name()), Some(op));
        }
        assert_eq!(DbOp::from_name("nope"), None);
    }
}
