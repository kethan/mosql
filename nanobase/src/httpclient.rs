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

use std::io::{BufRead, BufReader, Read as IoRead, Write};
use std::net::TcpStream;
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
pub trait HttpClient: Send + Sync {
    fn request(&self, request: &HttpRequest) -> ApiResult<HttpResponse>;
}

/// A plain `std::net` HTTP/1.1 client. `https://` is refused: there is no
/// TLS in this build, and pretending otherwise would be worse than an
/// error the operator can act on.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainHttpClient;

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
    Ok(HttpResponse { status, body })
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
#[derive(Debug, Clone, Copy, Default)]
pub struct CurlHttpClient;

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
        Ok(HttpResponse {
            status,
            body: bytes,
        })
    }
}
