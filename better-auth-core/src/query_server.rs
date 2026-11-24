//! The reference server for the store protocol.
//!
//! A store adapter — [`crate::store::http_store::HttpAuthStore`],
//! [`crate::store::tcp_store::TcpAuthStore`] — needs something on the other
//! end of the socket. This is it: a server that wraps any [`AuthStore`] and
//! speaks the protocol in [`crate::query`], over HTTP or over TCP.
//!
//! It is what a deployment uses when the database is somewhere else: the
//! database service runs this, the engine talks to it over the network, and
//! neither has to know about the other's transport.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;

use mosql::Value;

use crate::error::AuthResult;
use crate::http::{Request, Response};
use crate::query::{QueryOp, QueryRequest, QueryResponse};
use crate::store::{AuthStore, FindManyQuery};

/// A server over a store.
pub struct QueryServer {
    store: Arc<dyn AuthStore>,
}

impl QueryServer {
    pub fn new(store: Arc<dyn AuthStore>) -> Self {
        QueryServer { store }
    }

    /// The store this server is over.
    pub fn store(&self) -> &Arc<dyn AuthStore> {
        &self.store
    }

    /// Run one request and answer it.
    pub fn handle(&self, request: &QueryRequest) -> QueryResponse {
        match self.run(request) {
            Ok(response) => response,
            Err(e) => QueryResponse::failed(e.to_string()),
        }
    }

    /// Run one request, over a store.
    fn run(&self, request: &QueryRequest) -> AuthResult<QueryResponse> {
        let store = Arc::clone(&self.store);
        let model = request.model.as_str();
        let r#where = request.r#where.clone();
        let data = request.data.clone();
        let query = FindManyQuery {
            r#where: r#where.clone(),
            limit: request.limit,
            offset: request.offset,
            sort_by: request.sort_by.clone(),
        };
        // The store's methods are futures; the server has no runtime of its
        // own, so it drives them with the same block-on the binary uses.
        let response = uflite::db::block_on(async move {
            match request.op {
                QueryOp::Create => {
                    let row = store.create(model, data).await?;
                    Ok(QueryResponse::one(row))
                }
                QueryOp::FindOne => {
                    let row = store.find_one(model, &r#where).await?;
                    Ok(match row {
                        Some(row) => QueryResponse::one(row),
                        None => QueryResponse::empty(),
                    })
                }
                QueryOp::FindMany => {
                    let rows = store.find_many(model, query).await?;
                    Ok(QueryResponse {
                        count: rows.len() as u64,
                        rows,
                        error: None,
                    })
                }
                QueryOp::Count => {
                    let count = store.count(model, &r#where).await?;
                    Ok(QueryResponse::count(count))
                }
                QueryOp::Update => {
                    let row = store.update(model, &r#where, data).await?;
                    Ok(match row {
                        Some(row) => QueryResponse::one(row),
                        None => QueryResponse::empty(),
                    })
                }
                QueryOp::UpdateMany => {
                    let count = store.update_many(model, &r#where, data).await?;
                    Ok(QueryResponse::count(count))
                }
                QueryOp::Delete => {
                    store.delete(model, &r#where).await?;
                    Ok(QueryResponse::count(1))
                }
                QueryOp::DeleteMany => {
                    let count = store.delete_many(model, &r#where).await?;
                    Ok(QueryResponse::count(count))
                }
            }
        });
        response
    }

    /// Answer an HTTP request: `POST /query` with the request as JSON.
    pub fn handle_http(&self, req: &Request) -> Response {
        if req.method != "POST" {
            return Response::json(
                405,
                &error_value("METHOD_NOT_ALLOWED", "only POST is allowed"),
            );
        }
        let body = match req.json() {
            Ok(body) => body,
            Err(e) => return Response::json(400, &error_value("INVALID_REQUEST", &e.message)),
        };
        let request = match QueryRequest::from_value(&body) {
            Ok(request) => request,
            Err(e) => return Response::json(400, &error_value("INVALID_REQUEST", &e.message)),
        };
        let response = self.handle(&request);
        Response::json(200, &response.to_value())
    }

    /// Answer one TCP request: a JSON line in, a JSON line out.
    ///
    /// Returns whether the connection should stay open.
    pub fn handle_tcp(&self, reader: &mut BufReader<TcpStream>, writer: &mut TcpStream) -> bool {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        let line = line.trim();
        if line.is_empty() {
            return false;
        }
        let reply = match mosql::parse_json(line) {
            Ok(value) => match QueryRequest::from_value(&value) {
                Ok(request) => {
                    let response = self.handle(&request);
                    response.to_value()
                }
                Err(e) => error_value("INVALID_REQUEST", &e.message),
            },
            Err(e) => error_value("INVALID_REQUEST", &format!("not valid JSON: {e}")),
        };
        let mut out = mosql::to_json(&reply);
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            return false;
        }
        let _ = writer.flush();
        true
    }
}

/// The error body the server answers with.
fn error_value(code: &str, message: &str) -> Value {
    let mut body = mosql::Object::new();
    body.set("code", Value::str(code));
    body.set("message", Value::str(message));
    Value::Object(body)
}

/// Serve the protocol over HTTP until the listener fails.
pub fn serve_http(
    server: Arc<QueryServer>,
    addr: SocketAddr,
    threads: usize,
) -> std::io::Result<()> {
    serve_http_on(server, std::net::TcpListener::bind(addr)?, threads)
}

/// Serve the protocol over HTTP on a listener the caller already holds.
///
/// The listener is taken first, so a client that connects the moment the
/// address is known finds the server there.
pub fn serve_http_on(
    server: Arc<QueryServer>,
    listener: std::net::TcpListener,
    threads: usize,
) -> std::io::Result<()> {
    crate::http::serve_on(listener, threads, move |req| {
        if req.path == "/query" {
            server.handle_http(&req)
        } else {
            Response::json(404, &error_value("NOT_FOUND", "no such route"))
        }
    })
}

/// Serve the protocol over TCP until the listener fails.
///
/// One connection carries as many requests as the client sends: each line is
/// a request, each reply is a line.
pub fn serve_tcp(server: Arc<QueryServer>, listener: TcpListener) -> std::io::Result<()> {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let server = Arc::clone(&server);
        std::thread::spawn(move || {
            let Ok(write_half) = stream.try_clone() else {
                return;
            };
            let mut reader = BufReader::new(stream);
            let mut writer = write_half;
            // A connection carries as many requests as the client sends: each
            // line is a request, each reply is a line.
            while server.handle_tcp(&mut reader, &mut writer) {}
        });
    }
    Ok(())
}

/// The error a server replies with, for tests.
#[cfg(test)]
pub fn error(code: &str, message: &str) -> Value {
    error_value(code, message)
}
