//! The three things an HTTP API needs and forgets: security headers, CORS,
//! and a rate limit.
//!
//! A browser is the harshest client there is. It will not send a request
//! without a preflight, it will not tell the operator's server that a
//! request came from another site, and it will happily make ten thousand
//! requests a second if a page asks it to. Each of those three problems
//! has one answer, and this module holds all three.
//!
//! * **Security headers** are what Helmet does for a Node server: a
//!   `Content-Security-Policy` that says where a response's contents may
//!   come from, `X-Content-Type-Options` so a browser does not guess a
//!   content type, `X-Frame-Options` so the API cannot be framed, and the
//!   rest of the small set. The content policy differs by what is being
//!   answered — an API returns JSON and nothing may load it, and the
//!   console returns HTML that may only load the console's own files.
//! * **CORS** is per origin, not per server. The platform allows the
//!   origins an operator names, and an app may narrow or widen that with
//!   `config.cors` in its own document. An origin that is not allowed gets
//!   no CORS headers at all, which is the browser's way of saying no.
//! * **The rate limit** is per client, per window, and it is a counter
//!   rather than a queue: over the limit means `429` and a `Retry-After`,
//!   not a slow answer. The window slides, so a burst does not buy a
//!   second allowance immediately afterwards.
//!
//! All three are opt-in in the sense that they never change what a
//! request *means*. A CORS header that is missing changes nothing for
//! `curl`, and a security header changes nothing for any client at all.
//! They change what a *browser* is allowed to do with the answer.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::http::{Request, Response};

// ------------------------------------------------------- security headers --

/// The content policy for an API answer.
///
/// An API answer is data, not a document: nothing may be loaded from it,
/// it may not be framed, and it may not be used as a base for anything.
pub const API_CSP: &str =
    "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

/// The content policy for the console.
///
/// The console is a page, so it may load its own stylesheet and its own
/// script, and it may talk to its own API — and nothing else. A page
/// served from the console may not pull in a font from a CDN or a script
/// from a paste site.
pub const CONSOLE_CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; \
                              connect-src 'self'; font-src 'self'; frame-ancestors 'none'; base-uri 'none'; \
                              form-action 'self'; object-src 'none'";

/// The headers that make a response harder to abuse.
///
/// Every one of them is a header a browser reads and a script cannot
/// forge, so they are worth setting even where the platform is behind a
/// reverse proxy that sets its own.
fn base_headers(csp: &'static str) -> Vec<(&'static str, &'static str)> {
    vec![
        ("content-security-policy", csp),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-resource-policy", "same-origin"),
        ("cross-origin-opener-policy", "same-origin"),
        ("x-permitted-cross-domain-policies", "none"),
        (
            "permissions-policy",
            "accelerometer=(), camera=(), geolocation=(), gyroscope=(), magnetometer=(), microphone=(), payment=(), usb=()",
        ),
    ]
}

/// Add the security headers to a response.
///
/// `console` says whether the answer is the console's own HTML rather
/// than API data: the two get different content policies, because an
/// API answer may load nothing at all and the console must load its own
/// script.
pub fn secure(response: Response, console: bool) -> Response {
    let csp = if console { CONSOLE_CSP } else { API_CSP };
    let mut response = response;
    for (name, value) in base_headers(csp) {
        response = response.with_header(name, value);
    }
    response
}

// -------------------------------------------------------------------- cors --

/// What a browser may do with this API.
///
/// The platform's own list comes from the operator (`--cors`, or
/// `NANOBASE_CORS_ORIGINS`); an app may override it with `config.cors`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CorsPolicy {
    /// The origins allowed. `*` allows any origin.
    pub origins: Vec<String>,
    /// Whether a credentialed request is allowed. Credentials and `*` do
    /// not go together: a browser refuses `Access-Control-Allow-Origin:
    /// *` on a credentialed request, so the origin is echoed instead.
    pub allow_credentials: bool,
    /// How long a browser may cache the preflight answer, in seconds.
    pub max_age: u64,
}

impl CorsPolicy {
    /// A policy that allows nothing. The default, and the right one for a
    /// platform nobody has opened to a browser yet.
    pub fn closed() -> CorsPolicy {
        CorsPolicy::default()
    }

    /// A policy from a list of origins.
    pub fn from_origins(origins: &str) -> CorsPolicy {
        CorsPolicy {
            origins: origins
                .split(',')
                .map(|o| o.trim().to_string())
                .filter(|o| !o.is_empty())
                .collect(),
            allow_credentials: false,
            max_age: 600,
        }
    }

    /// Is anything allowed at all?
    pub fn is_open(&self) -> bool {
        !self.origins.is_empty()
    }

    /// May this origin make a request?
    pub fn allows(&self, origin: &str) -> bool {
        if self.origins.is_empty() {
            return false;
        }
        self.origins
            .iter()
            .any(|allowed| allowed == "*" || allowed.eq_ignore_ascii_case(origin))
    }

    /// The value for `Access-Control-Allow-Origin`, if there is one.
    ///
    /// A credentialed policy echoes the origin rather than saying `*`,
    /// because a browser will not attach credentials to a wildcard.
    fn allow_origin(&self, origin: &str) -> Option<String> {
        if !self.allows(origin) {
            return None;
        }
        if self.allow_credentials || !self.origins.iter().any(|o| o == "*") {
            Some(origin.to_string())
        } else {
            Some("*".to_string())
        }
    }

    /// Add the CORS headers to a response.
    ///
    /// A response for an origin that is not allowed gets no CORS headers,
    /// and the browser refuses to hand it to the page. That is the
    /// correct answer, not a missing one.
    pub fn apply(&self, req: &Request, response: Response) -> Response {
        if !self.is_open() {
            return response;
        }
        let Some(origin) = req.header("origin") else {
            // Not a browser request. Nothing to allow.
            return response;
        };
        let mut response = response.with_header("vary", "origin");
        let Some(allow) = self.allow_origin(origin) else {
            return response;
        };
        response = response.with_header("access-control-allow-origin", &allow);
        if self.allow_credentials {
            response = response.with_header("access-control-allow-credentials", "true");
        }
        response
    }

    /// The answer to a preflight: `OPTIONS` with no body.
    ///
    /// The browser asks before the real request, and the answer has to
    /// name the methods and headers the real one will use — otherwise
    /// the browser gives up without sending it.
    pub fn preflight(&self, req: &Request) -> Response {
        let mut response = Response {
            status: 204,
            headers: Vec::new(),
            body: Vec::new(),
        };
        // An origin that is not allowed gets a bare 204: no allow-origin,
        // and no allow-methods either, because a preflight that names the
        // methods without naming the origin tells the browser nothing it
        // can act on.
        let Some(origin) = req.header("origin") else {
            return response;
        };
        if !self.allows(origin) {
            return response;
        }
        response = self.apply(req, response);
        response = response.with_header(
            "access-control-allow-methods",
            "GET, POST, PUT, PATCH, DELETE, OPTIONS",
        );
        response = response
            .with_header(
                "access-control-allow-headers",
                "authorization, content-type, x-requested-with",
            )
            .with_header("access-control-max-age", &self.max_age.to_string());
        response
    }
}

// -------------------------------------------------------------- rate limit --

/// One client's allowance in one window.
#[derive(Debug)]
struct Window {
    count: u32,
    started: Instant,
}

/// A counter per client, over a sliding window.
///
/// The window slides rather than resets on a boundary, so a client that
/// spends its whole allowance cannot immediately spend another one by
/// waiting for the clock to tick over.
#[derive(Debug)]
pub struct RateLimiter {
    clients: Mutex<HashMap<String, Window>>,
    limit: u32,
    window: Duration,
}

/// How long a client must wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryAfter {
    pub seconds: u64,
}

impl RateLimiter {
    /// A limiter that allows `limit` requests per `window`.
    pub fn new(limit: u32, window: Duration) -> RateLimiter {
        RateLimiter {
            clients: Mutex::new(HashMap::new()),
            limit,
            window,
        }
    }

    /// The limit, in requests per window.
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Count one request against a client.
    ///
    /// `Ok(())` means the request is allowed. `Err` carries how long the
    /// client has to wait, which is what the `Retry-After` header says.
    pub fn check(&self, client: &str) -> Result<(), RetryAfter> {
        let mut clients = self.clients.lock().expect("rate limiter poisoned");
        // Forget clients whose window has long expired, so a long-lived
        // server does not remember every address it has ever seen.
        if clients.len() > 4096 {
            let window = self.window;
            clients.retain(|_, w| w.started.elapsed() < window * 2);
        }
        let now = Instant::now();
        let entry = clients.entry(client.to_string()).or_insert(Window {
            count: 0,
            started: now,
        });
        let elapsed = now.duration_since(entry.started);
        if elapsed >= self.window {
            // The window has rolled over: the allowance is whole again.
            entry.count = 0;
            entry.started = now;
        } else if entry.count >= self.limit {
            let left = self.window - elapsed;
            return Err(RetryAfter {
                seconds: left.as_secs().max(1),
            });
        }
        entry.count += 1;
        Ok(())
    }
}

/// The platform's HTTP options: what browsers may do, and how much of it.
#[derive(Default)]
pub struct HttpOptions {
    /// The platform's CORS policy. An app may narrow it.
    pub cors: CorsPolicy,
    /// The rate limit, when one is set.
    pub rate_limit: Option<RateLimiter>,
}

impl HttpOptions {
    /// Options that allow nothing and limit nothing.
    pub fn default_closed() -> HttpOptions {
        HttpOptions::default()
    }

    /// Options with a CORS policy.
    pub fn with_cors(cors: CorsPolicy) -> HttpOptions {
        HttpOptions {
            cors,
            rate_limit: None,
        }
    }

    /// Add a rate limit.
    pub fn with_rate_limit(mut self, limit: u32, window: Duration) -> HttpOptions {
        self.rate_limit = Some(RateLimiter::new(limit, window));
        self
    }
}

// ------------------------------------------------------------------ tests --

#[cfg(test)]
mod tests {
    use super::*;

    fn request(origin: Option<&str>) -> Request {
        let mut headers = Vec::new();
        if let Some(origin) = origin {
            headers.push(("origin".to_string(), origin.to_string()));
        }
        Request {
            method: "GET".to_string(),
            path: "/api/apps".to_string(),
            query: Vec::new(),
            headers,
            body: Vec::new(),
            remote: "127.0.0.1:1".parse().expect("address"),
        }
    }

    #[test]
    fn a_closed_policy_allows_nothing() {
        let policy = CorsPolicy::closed();
        assert!(!policy.is_open());
        assert!(!policy.allows("https://app.test"));
    }

    #[test]
    fn an_allowed_origin_gets_the_header() {
        let policy = CorsPolicy::from_origins("https://app.test, https://other.test");
        let response = policy.apply(
            &request(Some("https://app.test")),
            Response::json(200, &mosql::Value::Null),
        );
        assert!(response
            .headers
            .iter()
            .any(|(k, v)| k == "access-control-allow-origin" && v == "https://app.test"));
    }

    #[test]
    fn an_unknown_origin_gets_no_header() {
        let policy = CorsPolicy::from_origins("https://app.test");
        let response = policy.apply(
            &request(Some("https://evil.test")),
            Response::json(200, &mosql::Value::Null),
        );
        assert!(!response
            .headers
            .iter()
            .any(|(k, _)| k == "access-control-allow-origin"));
    }

    #[test]
    fn a_wildcard_with_credentials_echoes_the_origin() {
        let mut policy = CorsPolicy::from_origins("*");
        policy.allow_credentials = true;
        let response = policy.apply(
            &request(Some("https://app.test")),
            Response::json(200, &mosql::Value::Null),
        );
        assert!(response
            .headers
            .iter()
            .any(|(k, v)| k == "access-control-allow-origin" && v == "https://app.test"));
    }

    #[test]
    fn a_request_without_an_origin_is_left_alone() {
        let policy = CorsPolicy::from_origins("*");
        let response = policy.apply(&request(None), Response::json(200, &mosql::Value::Null));
        assert!(!response
            .headers
            .iter()
            .any(|(k, _)| k == "access-control-allow-origin"));
    }

    #[test]
    fn a_preflight_names_the_methods() {
        let policy = CorsPolicy::from_origins("https://app.test");
        let response = policy.preflight(&request(Some("https://app.test")));
        assert_eq!(response.status, 204);
        assert!(response
            .headers
            .iter()
            .any(|(k, v)| k == "access-control-allow-methods" && v.contains("PATCH")));
    }

    #[test]
    fn a_preflight_from_an_unknown_origin_says_nothing() {
        let policy = CorsPolicy::from_origins("https://app.test");
        let response = policy.preflight(&request(Some("https://evil.test")));
        assert_eq!(response.status, 204);
        assert!(!response
            .headers
            .iter()
            .any(|(k, _)| k.starts_with("access-control-allow")));
    }

    #[test]
    fn the_rate_limit_counts_and_then_refuses() {
        let limiter = RateLimiter::new(3, Duration::from_secs(60));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("a").is_ok());
        let refused = limiter
            .check("a")
            .expect_err("the fourth is over the limit");
        assert!(refused.seconds >= 1);
        // Another client has its own allowance.
        assert!(limiter.check("b").is_ok());
    }

    #[test]
    fn the_rate_limit_lets_a_window_expire() {
        let limiter = RateLimiter::new(1, Duration::from_millis(30));
        assert!(limiter.check("a").is_ok());
        assert!(limiter.check("a").is_err());
        std::thread::sleep(Duration::from_millis(60));
        assert!(limiter.check("a").is_ok());
    }
}
