//! HTTP/1.1 on `std::net`: just enough for a JSON API, and no more.
//!
//! The engine speaks one shape of HTTP — small JSON or form requests, JSON
//! or redirect replies — and a general-purpose server stack would be most of
//! its binary size for features this never uses. So: a fixed pool of worker
//! threads pulling accepted sockets from a queue, one request parsed and
//! answered at a time per connection, keep-alive honoured, chunked bodies
//! read, anything larger than a megabyte refused.
//!
//! The handler is a plain function, so the caller decides how to run it. The
//! binary wraps the engine's boxed futures in `uflite::db::block_on`; a
//! tokio host can drive them its own way and skip this module entirely.
//!
//! The engine lives here, in the platform crate, so the platform's own API
//! and the authentication server that embeds it speak exactly the same
//! protocol and fail in exactly the same way.

#[cfg(feature = "native")]
use std::collections::VecDeque;
#[cfg(feature = "native")]
use std::io::{BufRead, BufReader, Read as IoRead, Write};
use std::net::SocketAddr;
#[cfg(feature = "native")]
use std::net::{TcpListener, TcpStream};
#[cfg(feature = "native")]
use std::sync::{Arc, Condvar, Mutex};

use mosql::Value;

/// A failure with a status, a machine-readable code and a human-readable line.
///
/// Every failure leaves the engine as `{"code": CODE, "message": "…"}` with
/// an HTTP status that says which kind of failure it is.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiError {
    pub status: u16,
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            status,
            code,
            message: message.into(),
        }
    }

    /// 400 — the request cannot be acted on as sent.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(400, code, message)
    }

    /// 401 — no or invalid credentials.
    pub fn unauthorized(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(401, code, message)
    }

    /// 403 — authenticated, not allowed.
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(403, code, message)
    }

    /// 404 — the thing named does not exist.
    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(404, code, message)
    }

    /// 500 — the engine or its store failed, not the caller.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "UNKNOWN_ERROR", message)
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::bad_request("INVALID_REQUEST", message)
    }

    pub fn invalid_token(message: impl Into<String>) -> Self {
        Self::unauthorized("INVALID_TOKEN", message)
    }

    pub fn unauthorized_plain() -> Self {
        Self::unauthorized("UNAUTHORIZED", "unauthorized")
    }

    /// The wire form.
    pub fn to_json(&self) -> Value {
        let mut body = mosql::Object::new();
        body.set("code", Value::str(self.code));
        body.set("message", Value::str(self.message.clone()));
        Value::Object(body)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for ApiError {}

impl From<uflite::error::Error> for ApiError {
    fn from(e: uflite::error::Error) -> Self {
        match e.status() {
            409 => ApiError::new(409, "CONFLICT", e.to_string()),
            _ => ApiError::internal(format!("store: {e}")),
        }
    }
}

impl From<mosql::Error> for ApiError {
    fn from(e: mosql::Error) -> Self {
        ApiError::invalid_request(format!("not valid JSON: {e}"))
    }
}

#[cfg(any(feature = "native", feature = "curl"))]
impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError::internal(format!("i/o: {e}"))
    }
}

/// The largest request body accepted. Verification emails and sign-ups are
/// a few hundred bytes; a megabyte is already generous.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;
#[cfg(feature = "native")]
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// One request, parsed. Header names are lower-cased for lookup.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Path without the query string, percent-decoded.
    pub path: String,
    pub query: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub remote: SocketAddr,
}

impl Request {
    /// A request built by hand, for tests and embedders.
    pub fn new(method: &str, path: &str) -> Self {
        let (path, query) = split_query(path);
        Request {
            method: method.to_ascii_uppercase(),
            path: percent_decode(&path),
            query: parse_query(&query),
            headers: Vec::new(),
            body: Vec::new(),
            remote: "127.0.0.1:0".parse().expect("socket address"),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers
            .push((name.to_ascii_lowercase(), value.to_string()));
        self
    }

    pub fn with_json(mut self, body: Value) -> Self {
        self.body = mosql::to_json(&body).into_bytes();
        self.headers
            .push(("content-type".into(), "application/json".into()));
        self
    }

    pub fn with_form(mut self, fields: &[(&str, &str)]) -> Self {
        self.body = form_encode(fields).into_bytes();
        self.headers.push((
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        ));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The token in `Authorization: Bearer <token>`.
    pub fn bearer_token(&self) -> Option<&str> {
        let value = self.header("authorization")?;
        let (scheme, token) = value.split_once(' ')?;
        if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
            Some(token.trim())
        } else {
            None
        }
    }

    pub fn query_param(&self, key: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The body as a JSON value. An empty body is an empty object, so a
    /// route that takes nothing does not have to special-case it.
    pub fn json(&self) -> Result<Value, ApiError> {
        if self.body.iter().all(|b| b.is_ascii_whitespace()) {
            return Ok(Value::Object(mosql::Object::new()));
        }
        let text = std::str::from_utf8(&self.body)
            .map_err(|_| ApiError::invalid_request("body is not UTF-8"))?;
        mosql::parse_json(text)
            .map_err(|e| ApiError::invalid_request(format!("body is not JSON: {e}")))
    }

    /// The body as `application/x-www-form-urlencoded` fields — what Apple
    /// posts to the callback, and what token endpoints expect back.
    pub fn form(&self) -> Vec<(String, String)> {
        parse_query(&String::from_utf8_lossy(&self.body))
    }

    pub fn content_type(&self) -> &str {
        self.header("content-type").unwrap_or("")
    }

    pub fn is_form(&self) -> bool {
        self.content_type()
            .starts_with("application/x-www-form-urlencoded")
    }

    pub fn client_ip(&self) -> String {
        self.header("x-forwarded-for")
            .and_then(|v| v.split(',').next())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.remote.ip().to_string())
    }
}

/// One response, ready to write.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn json(status: u16, value: &Value) -> Response {
        Response {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: mosql::to_json(value).into_bytes(),
        }
    }

    pub fn error(error: &ApiError) -> Response {
        Response::json(error.status, &error.to_json())
    }

    pub fn redirect(location: &str) -> Response {
        Response {
            status: 302,
            headers: vec![
                ("location".into(), location.to_string()),
                ("content-type".into(), "text/plain".into()),
            ],
            body: Vec::new(),
        }
    }

    pub fn text(status: u16, body: &str) -> Response {
        Response {
            status,
            headers: vec![("content-type".into(), "text/plain; charset=utf-8".into())],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Response {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    #[cfg(feature = "native")]
    fn reason(&self) -> &'static str {
        status_text(self.status)
    }
}

pub fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        302 => "Found",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        _ => "Internal Server Error",
    }
}

// --------------------------------------------------------------- encoding ---

pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 3 <= bytes.len() => {
                // Two hex digits follow, or the `%` is literal.
                match std::str::from_utf8(&bytes[i + 1..i + 3])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                {
                    Some(byte) => {
                        out.push(byte);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Split `key=value&key=value`, decoding both sides.
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (percent_decode(k), percent_decode(v)),
            None => (percent_decode(part), String::new()),
        })
        .collect()
}

pub fn form_encode(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn split_query(target: &str) -> (String, String) {
    match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target.to_string(), String::new()),
    }
}

// ---------------------------------------------------------------- reading ---

/// What reading from a connection produced.
#[cfg(feature = "native")]
pub enum Incoming {
    /// The peer closed before sending anything: a clean end.
    Eof,
    Request(Request),
    /// Headers or body over the limit. The caller answers 413 and closes.
    TooLarge,
    /// Something unreadable. The caller answers 400 and closes.
    Malformed,
}

/// Read one request from the connection.
#[cfg(feature = "native")]
pub fn read_request(stream: &mut TcpStream) -> Incoming {
    let remote = match stream.peer_addr() {
        Ok(addr) => addr,
        Err(_) => return Incoming::Malformed,
    };
    // The reader is rebuilt per request on purpose: a `BufReader` would hold
    // bytes of the next request after the body, and we read exactly one.
    let mut reader = BufReader::new(stream.try_clone().expect("clone a TcpStream"));

    let mut head = String::new();
    // Read the head byte by byte up to the blank line; `read_line` would
    // block until a newline that a slow or hostile client may never send.
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                return if head.is_empty() {
                    Incoming::Eof
                } else {
                    Incoming::Malformed
                }
            }
            Ok(_) => {
                if head.len() + line.len() > MAX_HEADER_BYTES {
                    return Incoming::TooLarge;
                }
                if line == "\r\n" || line == "\n" {
                    if head.is_empty() {
                        return Incoming::Malformed;
                    }
                    break;
                }
                head.push_str(&line);
            }
            Err(_) => return Incoming::Malformed,
        }
    }

    let mut lines = head.lines();
    let Some(request_line) = lines.next() else {
        return Incoming::Malformed;
    };
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Incoming::Malformed;
    };
    if !version.starts_with("HTTP/1.") {
        return Incoming::Malformed;
    }
    let mut headers = Vec::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let (path, query) = split_query(target);
    let mut request = Request {
        method: method.to_ascii_uppercase(),
        path: percent_decode(&path),
        query: parse_query(&query),
        headers,
        body: Vec::new(),
        remote,
    };

    let content_length: Option<usize> = request
        .header("content-length")
        .and_then(|v| v.trim().parse().ok());
    let chunked = request
        .header("transfer-encoding")
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));

    let body = if chunked {
        match read_chunked(&mut reader) {
            Ok(body) => body,
            Err(ReadError::TooLarge) => return Incoming::TooLarge,
            Err(ReadError::Malformed) => return Incoming::Malformed,
        }
    } else {
        let len = content_length.unwrap_or(0);
        if len > MAX_BODY_BYTES {
            return Incoming::TooLarge;
        }
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            return Incoming::Malformed;
        }
        body
    };
    request.body = body;
    Incoming::Request(request)
}

#[cfg(feature = "native")]
enum ReadError {
    TooLarge,
    Malformed,
}

/// RFC 9112 chunked transfer coding, as browsers send it.
#[cfg(feature = "native")]
fn read_chunked(reader: &mut impl BufRead) -> Result<Vec<u8>, ReadError> {
    let mut body = Vec::new();
    loop {
        let mut size_line = String::new();
        reader
            .read_line(&mut size_line)
            .map_err(|_| ReadError::Malformed)?;
        let size =
            usize::from_str_radix(size_line.trim().split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| ReadError::Malformed)?;
        if body.len() + size > MAX_BODY_BYTES {
            return Err(ReadError::TooLarge);
        }
        let mut chunk = vec![0u8; size];
        reader
            .read_exact(&mut chunk)
            .map_err(|_| ReadError::Malformed)?;
        body.extend_from_slice(&chunk);
        let mut crlf = String::new();
        reader
            .read_line(&mut crlf)
            .map_err(|_| ReadError::Malformed)?;
        if size == 0 {
            return Ok(body);
        }
    }
}

// ---------------------------------------------------------------- writing ---

#[cfg(feature = "native")]
pub fn write_response(
    stream: &mut TcpStream,
    response: &Response,
    keep_alive: bool,
) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, response.reason());
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str(&format!("content-length: {}\r\n", response.body.len()));
    head.push_str(if keep_alive {
        "connection: keep-alive\r\n"
    } else {
        "connection: close\r\n"
    });
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

// ---------------------------------------------------------------- serving ---

/// Accept connections on `addr` and answer them with `handler`, using
/// `threads` workers. Blocks until the listener fails.
#[cfg(feature = "native")]
pub fn serve<H>(addr: SocketAddr, threads: usize, handler: H) -> std::io::Result<()>
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    serve_on(TcpListener::bind(addr)?, threads, handler)
}

/// [`serve`] on a listener the caller already holds.
#[cfg(feature = "native")]
pub fn serve_on<H>(listener: TcpListener, threads: usize, handler: H) -> std::io::Result<()>
where
    H: Fn(Request) -> Response + Send + Sync + 'static,
{
    let handler = Arc::new(handler);
    let queue: Arc<(Mutex<VecDeque<TcpStream>>, Condvar)> = Arc::default();

    for _ in 0..threads.max(1) {
        let queue = Arc::clone(&queue);
        let handler = Arc::clone(&handler);
        std::thread::spawn(move || loop {
            let stream = {
                let (lock, signal) = &*queue;
                let mut pending = lock.lock().expect("accept queue poisoned");
                loop {
                    match pending.pop_front() {
                        Some(stream) => break stream,
                        None => pending = signal.wait(pending).expect("accept queue poisoned"),
                    }
                }
            };
            let _ = handle_connection(stream, handler.as_ref());
        });
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let (lock, signal) = &*queue;
                lock.lock()
                    .expect("accept queue poisoned")
                    .push_back(stream);
                signal.notify_one();
            }
            Err(_) => continue,
        }
    }
    Ok(())
}

/// Answer every request on one connection until it closes or asks to.
#[cfg(feature = "native")]
fn handle_connection<H>(mut stream: TcpStream, handler: &H) -> std::io::Result<()>
where
    H: Fn(Request) -> Response,
{
    // An idle keep-alive connection is dropped rather than held forever.
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(30)));
    let _ = stream.set_nodelay(true);
    loop {
        match read_request(&mut stream) {
            Incoming::Eof => return Ok(()),
            Incoming::TooLarge => {
                let response = Response::error(&ApiError::new(
                    413,
                    "PAYLOAD_TOO_LARGE",
                    "request body too large",
                ));
                return write_response(&mut stream, &response, false);
            }
            Incoming::Malformed => {
                let response = Response::error(&ApiError::invalid_request("malformed request"));
                return write_response(&mut stream, &response, false);
            }
            Incoming::Request(request) => {
                let keep_alive = request
                    .header("connection")
                    .map(|c| !c.eq_ignore_ascii_case("close"))
                    .unwrap_or(true);
                let response = handler(request);
                write_response(&mut stream, &response, keep_alive)?;
                if !keep_alive {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(all(test, feature = "native"))]
mod tests {
    use super::*;

    /// Read one reply from a socket as text: status line, headers, body.
    fn exchange(stream: &mut TcpStream, request: &str) -> String {
        stream.write_all(request.as_bytes()).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut reply = String::new();
        let mut line = String::new();
        let mut length = 0;
        loop {
            line.clear();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
            if let Some(len) = line.strip_prefix("content-length: ") {
                length = len.trim().parse().unwrap();
            }
            reply.push_str(&line);
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap();
        reply.push_str("\r\n");
        reply.push_str(&String::from_utf8(body).unwrap());
        reply
    }

    fn body_of(reply: &str) -> &str {
        reply.split_once("\r\n\r\n").expect("header terminator").1
    }

    #[test]
    fn query_and_form_decoding() {
        let q = parse_query("a=1&b=hello+world&c=x%40y&flag&d=a%26b&e=%zz&f=%");
        assert_eq!(q[0], ("a".into(), "1".into()));
        assert_eq!(q[1], ("b".into(), "hello world".into()));
        assert_eq!(q[2], ("c".into(), "x@y".into()));
        assert_eq!(q[3], ("flag".into(), String::new()));
        assert_eq!(q[4], ("d".into(), "a&b".into()));
        assert_eq!(q[5], ("e".into(), "%zz".into()));
        assert_eq!(q[6], ("f".into(), "%".into()));
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
        let req = Request::new("GET", "/x?token=abc%20def");
        assert_eq!(req.path, "/x");
        assert_eq!(req.query_param("token"), Some("abc def"));
        assert_eq!(Request::new("get", "/x").method, "GET");
    }

    #[test]
    fn bearer_and_bodies() {
        let req = Request::new("POST", "/x")
            .with_header("Authorization", "Bearer tok.en")
            .with_json(mosql::parse_json(r#"{"a":1}"#).unwrap());
        assert_eq!(req.bearer_token(), Some("tok.en"));
        assert_eq!(req.json().unwrap().get("a"), Some(&Value::Int(1)));
        let req = Request::new("POST", "/callback").with_form(&[("code", "a b"), ("state", "s")]);
        assert_eq!(
            req.form(),
            vec![("code".into(), "a b".into()), ("state".into(), "s".into())]
        );
        assert!(req.is_form());
        assert_eq!(Request::new("GET", "/x").bearer_token(), None);
        assert_eq!(
            Request::new("GET", "/x")
                .with_header("Authorization", "Basic abc")
                .bearer_token(),
            None
        );
        assert!(Request::new("POST", "/x")
            .json()
            .unwrap()
            .get("a")
            .is_none());
        let bad = Request {
            body: vec![0xff],
            ..Request::new("POST", "/x")
        };
        assert!(bad.json().is_err());
    }

    #[test]
    fn server_answers_and_keeps_alive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let mut ok = mosql::Object::new();
        ok.set("ok", Value::Bool(true));
        let body = Value::Object(ok);
        std::thread::spawn(move || {
            serve_on(listener, 2, move |req| match req.path.as_str() {
                "/ok" => Response::json(200, &body),
                "/echo" => Response::json(200, &req.json().unwrap_or(Value::Null)),
                _ => Response::error(&ApiError::not_found("NOT_FOUND", "no such route")),
            })
            .unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        let reply = exchange(&mut stream, "GET /ok HTTP/1.1\r\nhost: x\r\n\r\n");
        assert!(reply.starts_with("HTTP/1.1 200 OK\r\n"), "{reply}");
        assert!(reply.contains("\r\nconnection: keep-alive\r\n"));
        assert!(reply.contains("\r\ncontent-type: application/json\r\n"));
        assert_eq!(body_of(&reply), r#"{"ok":true}"#);
        // Second request on the same connection.
        let reply = exchange(&mut stream, "GET /nope HTTP/1.1\r\nhost: x\r\n\r\n");
        assert!(reply.starts_with("HTTP/1.1 404 Not Found\r\n"), "{reply}");
        assert!(body_of(&reply).contains("NOT_FOUND"));
        // Chunked body.
        let reply = exchange(
            &mut stream,
            "POST /echo HTTP/1.1\r\nhost: x\r\ntransfer-encoding: chunked\r\ncontent-type: application/json\r\n\r\n5\r\n{\"a\":\r\n4\r\n123}\r\n0\r\n\r\n",
        );
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert_eq!(body_of(&reply), r#"{"a":123}"#);
        // `Connection: close` is honoured.
        let reply = exchange(
            &mut stream,
            "GET /ok HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n",
        );
        assert!(reply.contains("\r\nconnection: close\r\n"));
        assert_eq!(stream.read(&mut [0u8; 1]).unwrap(), 0);
    }

    #[test]
    fn oversized_body_refused_before_reading() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            serve_on(listener, 1, |_| Response::json(200, &Value::Null)).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        let reply = exchange(
            &mut stream,
            &format!(
                "POST /x HTTP/1.1\r\nhost: x\r\ncontent-length: {}\r\n\r\n",
                MAX_BODY_BYTES + 1
            ),
        );
        assert!(reply.starts_with("HTTP/1.1 413"), "{reply}");
        assert!(body_of(&reply).contains("PAYLOAD_TOO_LARGE"));
        assert!(reply.contains("\r\nconnection: close\r\n"));
    }

    #[test]
    fn malformed_request_answered_400() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            serve_on(listener, 1, |_| Response::json(200, &Value::Null)).unwrap();
        });
        let mut stream = TcpStream::connect(addr).unwrap();
        let reply = exchange(&mut stream, "NOT HTTP AT ALL\r\n\r\n");
        assert!(reply.starts_with("HTTP/1.1 400"), "{reply}");
    }

    #[test]
    fn status_text_known() {
        assert_eq!(status_text(404), "Not Found");
        assert_eq!(status_text(418), "Internal Server Error");
        let r = Response::redirect("https://x.test/cb?token=a");
        assert_eq!(r.status, 302);
        assert_eq!(
            r.headers[0],
            ("location".into(), "https://x.test/cb?token=a".into())
        );
    }
}
