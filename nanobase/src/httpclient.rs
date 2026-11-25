//! The outbound HTTP client: one request, one reply, blocking.
//!
//! The platform needs to reach other services — a serverless database over
//! its HTTP API, an OAuth provider, a webhook — and it has no registry
//! access to pull an HTTP stack in. So the client is written here, on
//! `std::net`, in the same spirit as the server: plain HTTP directly, and
//! TLS through `curl`, which every server has and which is the only way to
//! reach an `https://` endpoint without a TLS library.
//!
//! The host chooses the transport. [`PlainHttpClient`] speaks `http://`
//! only; [`CurlHttpClient`] speaks both.

#[cfg(feature = "curl")]
use std::io::Write;
#[cfg(feature = "native")]
use std::io::{BufRead, BufReader, Read as IoRead};
#[cfg(feature = "native")]
use std::net::TcpStream;
#[cfg(feature = "curl")]
use std::process::{Command, Stdio};

use mosql::Value;

use crate::http::{parse_query, ApiError};

/// The result type of a client call.
pub type ApiResult<T> = std::result::Result<T, ApiError>;

/// One HTTP request, as the OAuth routes make them.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
    /// The reply's own headers, in the order they arrived.
    ///
    /// Some services answer with something in a header rather than in the
    /// body — TiDB keeps its session there — so a client that cannot see
    /// them cannot hold a transaction open.
    pub headers: Vec<(String, String)>,
}

impl HttpResponse {
    /// The body as JSON, or an error naming the status.
    pub fn json(self) -> ApiResult<Value> {
        let text = std::str::from_utf8(&self.body)
            .map_err(|_| ApiError::internal("upstream returned non-UTF-8"))?;
        if text.trim().is_empty() {
            return Ok(Value::Object(mosql::Object::new()));
        }
        mosql::parse_json(text)
            .map_err(|e| ApiError::internal(format!("upstream returned invalid JSON ({e})")))
    }

    /// The body as form fields — several token endpoints answer in
    /// `x-www-form-urlencoded` even when asked for JSON.
    pub fn form(&self) -> Vec<(String, String)> {
        parse_query(&String::from_utf8_lossy(&self.body))
    }
}

/// Sends a request and returns the reply. Blocking: the server runs each
/// request on a worker thread. The host provides the transport — a plain
/// `std::net` one for `http://`, a TLS-capable one in production.
#[cfg(feature = "curl")]
pub trait HttpClient: Send + Sync {
    fn request(&self, request: &HttpRequest) -> ApiResult<HttpResponse>;
}

/// A shared client is still a client.
///
/// This is what lets a caller keep a handle on the client it handed over —
/// a test that wants to see what was asked of it, say — without giving up
/// the single trait the engine talks to.
#[cfg(feature = "curl")]
impl<C: HttpClient + ?Sized> HttpClient for std::sync::Arc<C> {
    fn request(&self, request: &HttpRequest) -> ApiResult<HttpResponse> {
        (**self).request(request)
    }
}

/// The asynchronous face of the outbound client.
///
/// A Worker has no blocking I/O: its transport is a promise that resolves
/// when the platform says so. So the engine talks to this trait, and the
/// native transports answer through [`SyncHttpClient`], which wraps a
/// blocking call in a future that is already done.
pub trait AsyncHttpClient: Send + Sync {
    fn request<'a>(
        &'a self,
        request: &'a HttpRequest,
    ) -> crate::platform::BoxFuture<'a, ApiResult<HttpResponse>>;
}

/// Wraps a blocking [`HttpClient`] as an [`AsyncHttpClient`].
///
/// The work happens when the future is first polled, and the answer is
/// there on the same poll — a blocking transport has nothing to wait for.
#[cfg(feature = "native")]
pub struct SyncHttpClient<C>(pub C);

#[cfg(feature = "native")]
impl<C: HttpClient> AsyncHttpClient for SyncHttpClient<C> {
    fn request<'a>(
        &'a self,
        request: &'a HttpRequest,
    ) -> crate::platform::BoxFuture<'a, ApiResult<HttpResponse>> {
        Box::pin(async move { self.0.request(request) })
    }
}

/// A plain `std::net` HTTP/1.1 client. `https://` is refused: there is no
/// TLS in this build, and pretending otherwise would be worse than an
/// error the operator can act on.
#[cfg(feature = "native")]
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainHttpClient;

#[cfg(feature = "native")]
impl HttpClient for PlainHttpClient {
    fn request(&self, request: &HttpRequest) -> ApiResult<HttpResponse> {
        let (host, port, path) = split_url(&request.url)?;
        if request.url.starts_with("https://") {
            return Err(ApiError::internal(
                "https:// needs a TLS-capable HttpClient; configure the host's client or use CurlHttpClient",
            ));
        }
        let mut stream = TcpStream::connect((host.as_str(), port))
            .map_err(|e| ApiError::internal(format!("cannot reach {host}:{port}: {e}")))?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(15)))
            .map_err(ApiError::from)?;
        let mut head = format!(
            "{} {} HTTP/1.1\r\nhost: {host}\r\nconnection: close\r\naccept-encoding: identity\r\n",
            request.method, path
        );
        for (name, value) in &request.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !request
            .headers
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        {
            head.push_str(&format!("content-length: {}\r\n", request.body.len()));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).map_err(ApiError::from)?;
        stream.write_all(&request.body).map_err(ApiError::from)?;
        read_http_reply(&stream)
    }
}

/// Reads an HTTP/1.1 reply: status line, headers, body by length or chunked.
#[cfg(feature = "native")]
fn read_http_reply(stream: &TcpStream) -> ApiResult<HttpResponse> {
    let mut reader = BufReader::new(stream.try_clone().map_err(ApiError::from)?);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(ApiError::from)?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| ApiError::internal("upstream sent a malformed status line"))?;
    let mut length: Option<usize> = None;
    let mut chunked = false;
    let mut headers: Vec<(String, String)> = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).map_err(ApiError::from)?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            length = v.trim().parse().ok();
        } else if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
            chunked = true;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let body = if chunked {
        let mut body = Vec::new();
        loop {
            line.clear();
            reader.read_line(&mut line).map_err(ApiError::from)?;
            let size =
                usize::from_str_radix(line.trim().split(';').next().unwrap_or("").trim(), 16)
                    .map_err(|_| ApiError::internal("bad chunk size"))?;
            let mut chunk = vec![0u8; size];
            reader.read_exact(&mut chunk).map_err(ApiError::from)?;
            body.extend_from_slice(&chunk);
            line.clear();
            reader.read_line(&mut line).map_err(ApiError::from)?;
            if size == 0 {
                break body;
            }
        }
    } else {
        let mut body = vec![0u8; length.unwrap_or(0)];
        reader.read_exact(&mut body).map_err(ApiError::from)?;
        body
    };
    Ok(HttpResponse {
        status,
        body,
        headers,
    })
}

/// Splits a URL into its host, port and path.
pub fn split_url(url: &str) -> ApiResult<(String, u16, String)> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| ApiError::internal(format!("bad url {url}")))?;
    let default_port = match scheme {
        "http" => 80,
        "https" => 443,
        _ => return Err(ApiError::internal(format!("unsupported scheme {scheme}"))),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), p.parse().unwrap_or(default_port))
        }
        _ => (authority.to_string(), default_port),
    };
    Ok((host, port, path.to_string()))
}

/// An `HttpClient` that shells out to `curl`, which every server has and
/// which brings TLS. The body goes in through stdin so nothing is spelled
/// out on a command line.
#[cfg(feature = "curl")]
#[derive(Debug, Clone, Copy, Default)]
pub struct CurlHttpClient;

#[cfg(feature = "curl")]
impl HttpClient for CurlHttpClient {
    fn request(&self, request: &HttpRequest) -> ApiResult<HttpResponse> {
        let mut command = Command::new("curl");
        command
            .arg("-sS")
            .arg("--max-time")
            .arg("20")
            .arg("-X")
            .arg(&request.method)
            .arg("-w")
            .arg("\n%{http_code}")
            // The reply's own headers, before the body: some services
            // keep a session in one.
            .arg("-D")
            .arg("-")
            .arg("--data-binary")
            .arg("@-");
        for (name, value) in &request.headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command.arg(&request.url);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| ApiError::internal(format!("cannot start curl: {e}")))?;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(&request.body)
            .map_err(ApiError::from)?;
        let output = child.wait_with_output().map_err(ApiError::from)?;
        // curl reports a failed connection on stderr and exits non-zero.
        // Without this the empty body would be read as an empty answer,
        // and a database that is not there would look like a database that
        // said nothing.
        if !output.status.success() {
            return Err(ApiError::internal(format!(
                "curl failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let mut bytes = output.stdout;
        let status = bytes
            .rsplit(|b| *b == b'\n')
            .find(|s| !s.is_empty())
            .and_then(|s| std::str::from_utf8(s).ok())
            .and_then(|s| s.trim().parse::<u16>().ok())
            .ok_or_else(|| {
                ApiError::internal(format!(
                    "curl failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            })?;
        // Drop the trailing "\n<status>" curl appended.
        if let Some(i) = bytes.iter().rposition(|b| *b == b'\n') {
            bytes.truncate(i);
        }
        // What is left is the headers, a blank line, and the body.
        let (headers, body) = split_reply(&bytes);
        Ok(HttpResponse {
            status,
            body,
            headers,
        })
    }
}

/// The headers of a reply, and its body.
///
/// The headers are the lines before the blank line; the body is
/// everything after it. A reply with no headers has an empty body and
/// no headers, which is what a broken proxy looks like.
#[cfg(feature = "curl")]
fn split_reply(raw: &[u8]) -> (Vec<(String, String)>, Vec<u8>) {
    let mut headers = Vec::new();
    let mut split = raw.len();
    let mut index = 0usize;
    while index + 1 < raw.len() {
        if raw[index] == b'\n' && raw[index + 1] == b'\n' {
            split = index;
            break;
        }
        if index + 3 < raw.len() && &raw[index..index + 4] == b"\r\n\r\n" {
            split = index;
            break;
        }
        index += 1;
    }
    for line in raw[..split].split(|b| *b == b'\n') {
        let line = String::from_utf8_lossy(line);
        let line = line.trim_end_matches('\r');
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let mut body_start = split;
    while body_start < raw.len() && (raw[body_start] == b'\n' || raw[body_start] == b'\r') {
        body_start += 1;
    }
    (headers, raw[body_start.min(raw.len())..].to_vec())
}
