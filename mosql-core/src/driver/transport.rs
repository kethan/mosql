//! Transports: how the bytes actually travel.
//!
//! The driver does not care, and that is the point. Three of the five
//! providers are happy with any transport; two of them need a session
//! back, which is why [`HttpTransport::post_json_with_headers`] exists.
//!
//! | feature | transport | where it runs |
//! | --- | --- | --- |
//! | `transport-curl` | [`CurlTransport`] | a native build: the system `curl`, which brings TLS |
//! | `transport-worker` | [`HostTransport`] | a Worker, or any host with its own `fetch` |
//! | `transport-ureq` | reserved | a build that may use the registry — see the README |

#[cfg(any(feature = "transport-curl", feature = "transport-worker"))]
use alloc::string::String;
#[cfg(any(feature = "transport-curl", feature = "transport-worker"))]
use alloc::vec::Vec;

#[cfg(any(feature = "transport-curl", feature = "transport-worker"))]
use crate::driver::{DbError, HttpTransport};

/// A transport that posts through the system `curl`.
///
/// This is the platform's own answer to "HTTPS without a registry":
/// every server has `curl`, and it brings TLS. The body goes in
/// through stdin, so nothing is spelled out on a command line.
#[cfg(all(feature = "transport-curl", feature = "std"))]
pub struct CurlTransport;

#[cfg(all(feature = "transport-curl", feature = "std"))]
impl Default for CurlTransport {
    fn default() -> CurlTransport {
        CurlTransport
    }
}

#[cfg(all(feature = "transport-curl", feature = "std"))]
impl HttpTransport for CurlTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError> {
        let (status, body, _) = self.post_json_with_headers(url, headers, body).await?;
        Ok((status, body))
    }

    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String, Vec<(String, String)>), DbError> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let mut command = Command::new("curl");
        command
            .arg("-sS")
            .arg("--max-time")
            .arg("20")
            .arg("-X")
            .arg("POST")
            .arg("-D")
            .arg("-")
            .arg("-w")
            .arg("\\n%{http_code}")
            .arg("--data-binary")
            .arg("@-");
        for (name, value) in headers {
            command.arg("-H").arg(format!("{name}: {value}"));
        }
        command.arg(url);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|e| DbError::new(format!("cannot start curl: {e}")))?;
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(body.as_bytes())
            .map_err(|e| DbError::new(format!("cannot write to curl: {e}")))?;
        let output = child
            .wait_with_output()
            .map_err(|e| DbError::new(format!("curl failed: {e}")))?;
        // curl reports a failed connection on stderr and exits non-zero.
        // Without this an empty body would read as an empty answer, and a
        // database that is not there would look like a database that said
        // nothing.
        if !output.status.success() {
            return Err(DbError::new(format!(
                "curl failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        let bytes = output.stdout;
        let (headers, rest) = split_headers(&bytes);
        // The last line is the status curl appended.
        let status = rest
            .rsplit(|b| *b == b'\n')
            .find(|s| !s.is_empty())
            .and_then(|s| std::str::from_utf8(s).ok())
            .and_then(|s| s.trim().parse::<u16>().ok())
            .ok_or_else(|| {
                DbError::new(format!(
                    "curl failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ))
            })?;
        let body = match rest.iter().rposition(|b| *b == b'\n') {
            Some(index) => rest[..index].to_vec(),
            None => Vec::new(),
        };
        Ok((status, String::from_utf8_lossy(&body).into_owned(), headers))
    }
}

/// Split a raw HTTP reply into its headers and its body.
///
/// The headers are the lines before the blank line; the body is
/// everything after it.
#[cfg(all(feature = "transport-curl", feature = "std"))]
fn split_headers(raw: &[u8]) -> (Vec<(String, String)>, Vec<u8>) {
    let mut headers = Vec::new();
    // Find the blank line.
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
    let body = if split < raw.len() {
        // Skip the blank line.
        let mut body_start = split;
        while body_start < raw.len() && (raw[body_start] == b'\n' || raw[body_start] == b'\r') {
            body_start += 1;
        }
        raw[body_start..].to_vec()
    } else {
        Vec::new()
    };
    (headers, body)
}

/// A transport over a host-provided `fetch`.
///
/// This is what a Cloudflare Worker has: `fetch` is a promise, and the
/// host owns the networking. The caller supplies the function, so the
/// crate has no opinion about how the request is made — and so the same
/// driver works in a Worker, in a test, and on a server.
#[cfg(feature = "transport-worker")]
pub struct HostTransport<F> {
    fetch: F,
}

#[cfg(feature = "transport-worker")]
impl<F> HostTransport<F> {
    /// A transport that asks the host to make the request.
    ///
    /// The function is given the URL, the headers and the body, and
    /// answers with the status, the body and the response headers.
    pub fn new(fetch: F) -> HostTransport<F> {
        HostTransport { fetch }
    }
}

#[cfg(feature = "transport-worker")]
impl<F, Fut> HttpTransport for HostTransport<F>
where
    F: Fn(&str, &[(&str, &str)], String) -> Fut + Send + Sync,
    Fut:
        core::future::Future<Output = Result<(u16, String, Vec<(String, String)>), DbError>> + Send,
{
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError> {
        let (status, body, _) = self.post_json_with_headers(url, headers, body).await?;
        Ok((status, body))
    }

    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String, Vec<(String, String)>), DbError> {
        (self.fetch)(url, headers, body).await
    }
}

#[cfg(test)]
mod tests {
    #[cfg(all(feature = "transport-curl", feature = "std"))]
    mod curl {
        use super::split_headers;

        #[test]
        fn the_headers_and_the_body_are_split() {
            let raw = b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nTiDB-Session: abc\r\n\r\n{\"a\":1}";
            let (headers, body) = split_headers(raw);
            assert!(headers
                .iter()
                .any(|(name, value)| name == "content-type" && value == "application/json"));
            assert!(headers
                .iter()
                .any(|(name, value)| name == "TiDB-Session" && value == "abc"));
            assert_eq!(String::from_utf8_lossy(&body), "{\"a\":1}");
        }

        #[test]
        fn a_reply_with_no_headers_still_has_a_body() {
            let raw = b"HTTP/1.1 200 OK\r\n\r\n{\"a\":1}";
            let (headers, body) = split_headers(raw);
            assert!(headers.is_empty());
            assert_eq!(String::from_utf8_lossy(&body), "{\"a\":1}");
        }
    }
}
