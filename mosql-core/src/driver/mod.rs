//! The serverless drivers.
//!
//! A driver is a small thing: it holds a configuration and a transport,
//! and it turns SQL and parameters into an HTTP request and the answer
//! back into rows. Everything here is transport-agnostic — the bytes
//! travel by whatever means the host provides — so a wire protocol can
//! be tested with a mock and nothing else.

pub mod d1;
pub mod encoding;
pub mod neon;
pub mod planetscale;
pub mod sql;
pub mod tidb;
pub mod transport;
pub mod turso;
pub mod url;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub use url::{detect_provider, DriverConfig};

#[cfg(feature = "transport-curl")]
pub use transport::CurlTransport;
#[cfg(feature = "transport-worker")]
pub use transport::HostTransport;

/// A value in a row, or a value going in.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    /// SQL `NULL`.
    Null,
    /// `TRUE` or `FALSE`.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// A number with a fraction.
    Float(f64),
    /// Text.
    Text(String),
    /// Bytes.
    Blob(Vec<u8>),
}

impl SqlValue {
    /// Is this `NULL`?
    pub fn is_null(&self) -> bool {
        matches!(self, SqlValue::Null)
    }

    /// The value as text, if it is text.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            SqlValue::Text(text) => Some(text),
            _ => None,
        }
    }

    /// The value as a whole number, if it is one.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            SqlValue::Int(number) => Some(*number),
            _ => None,
        }
    }
}

/// The answer to a statement.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueryResult {
    /// The names of the columns, in order.
    pub columns: Vec<String>,
    /// The rows, in order.
    pub rows: Vec<Vec<SqlValue>>,
    /// How many rows the statement changed.
    pub rows_affected: u64,
    /// The id the statement generated, if it generated one.
    pub last_insert_id: Option<i64>,
}

impl QueryResult {
    /// An answer with no rows: the shape a statement that changes
    /// something gives.
    pub fn empty() -> QueryResult {
        QueryResult::default()
    }
}

/// Which SQL a provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// SQLite: Turso, libSQL, Cloudflare D1.
    Sqlite,
    /// PostgreSQL: Neon, Vercel Postgres.
    Postgres,
    /// MySQL: TiDB Cloud Serverless, PlanetScale.
    Mysql,
}

impl Dialect {
    /// The dialect's name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Dialect::Sqlite => "sqlite",
            Dialect::Postgres => "postgres",
            Dialect::Mysql => "mysql",
        }
    }
}

/// Which provider a driver is talking to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// Turso, over libSQL.
    Turso,
    /// Cloudflare D1.
    D1,
    /// Neon, or Vercel Postgres.
    Neon,
    /// TiDB Cloud Serverless.
    Tidb,
    /// PlanetScale, over Vitess.
    PlanetScale,
}

impl Provider {
    /// The dialect the provider speaks.
    pub fn dialect(&self) -> Dialect {
        match self {
            Provider::Turso | Provider::D1 => Dialect::Sqlite,
            Provider::Neon => Dialect::Postgres,
            Provider::Tidb | Provider::PlanetScale => Dialect::Mysql,
        }
    }

    /// The provider's name.
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Turso => "turso",
            Provider::D1 => "d1",
            Provider::Neon => "neon",
            Provider::Tidb => "tidb",
            Provider::PlanetScale => "planetscale",
        }
    }
}

/// Something went wrong.
#[derive(Debug, Clone, PartialEq)]
pub struct DbError {
    /// The provider's own code, if it gave one.
    pub code: Option<String>,
    /// What went wrong, in words.
    pub message: String,
    /// The HTTP status, if the failure was an HTTP one.
    pub status: Option<u16>,
}

impl DbError {
    /// A failure with nothing but words.
    pub fn new(message: impl Into<String>) -> DbError {
        DbError {
            code: None,
            message: message.into(),
            status: None,
        }
    }

    /// A failure with a code and a status.
    pub fn with_code(
        code: impl Into<String>,
        message: impl Into<String>,
        status: Option<u16>,
    ) -> DbError {
        DbError {
            code: Some(code.into()),
            message: message.into(),
            status,
        }
    }
}

impl core::fmt::Display for DbError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match (&self.code, self.status) {
            (Some(code), Some(status)) => {
                write!(f, "{} ({code}, HTTP {status})", self.message)
            }
            (Some(code), None) => write!(f, "{} ({code})", self.message),
            (None, Some(status)) => write!(f, "{} (HTTP {status})", self.message),
            (None, None) => write!(f, "{}", self.message),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for DbError {}

/// How the bytes travel.
///
/// The driver does not care. A native build posts through the system
/// `curl`; a Worker posts through the host's `fetch`; a test answers
/// from a string.
#[allow(async_fn_in_trait)]
pub trait HttpTransport: Send + Sync {
    /// Post a JSON body and read the answer.
    ///
    /// The answer is the status and the body, both as they arrived: the
    /// driver decides what a status means.
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError>;

    /// Post a JSON body and read the answer **with its headers**.
    ///
    /// Two of the five providers keep a session, and one of them puts
    /// the session in a response *header*. A transport that cannot see
    /// its own response headers is enough for the other three, and for
    /// TiDB the session also travels in the body — so the default is
    /// the simple answer with no headers at all.
    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String, Vec<(String, String)>), DbError> {
        let (status, body) = self.post_json(url, headers, body).await?;
        Ok((status, body, Vec::new()))
    }
}

/// A database that is reached over HTTP.
#[allow(async_fn_in_trait)]
pub trait ServerlessDriver: Send + Sync {
    /// Which SQL the driver speaks.
    fn dialect(&self) -> Dialect;

    /// Run one statement.
    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError>;

    /// Run several statements, in one round trip where the provider
    /// allows it.
    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError>;
}

/// A driver of any of the five providers.
///
/// An enum rather than a trait object: the dispatch is a jump table, not
/// a virtual call, and the crate has no allocator in the hot path.
pub enum AnyServerlessDriver<T: HttpTransport> {
    /// Turso, over libSQL.
    Turso(turso::TursoDriver<T>),
    /// Cloudflare D1.
    D1(d1::D1Driver<T>),
    /// Neon, or Vercel Postgres.
    Neon(neon::NeonDriver<T>),
    /// TiDB Cloud Serverless.
    Tidb(tidb::TidbDriver<T>),
    /// PlanetScale, over Vitess.
    PlanetScale(planetscale::PlanetScaleDriver<T>),
}

impl<T: HttpTransport> AnyServerlessDriver<T> {
    /// Read a URL, work out the provider, and make the driver.
    pub fn from_url(url: &str, transport: T) -> Result<Self, DbError> {
        let config = DriverConfig::from_url(url)?;
        Ok(AnyServerlessDriver::from_config(config, transport))
    }

    /// Make the driver for a configuration that has already been read.
    pub fn from_config(config: DriverConfig, transport: T) -> Self {
        match config.provider {
            Provider::Turso => {
                AnyServerlessDriver::Turso(turso::TursoDriver::new(config, transport))
            }
            Provider::D1 => AnyServerlessDriver::D1(d1::D1Driver::new(config, transport)),
            Provider::Neon => AnyServerlessDriver::Neon(neon::NeonDriver::new(config, transport)),
            Provider::Tidb => AnyServerlessDriver::Tidb(tidb::TidbDriver::new(config, transport)),
            Provider::PlanetScale => AnyServerlessDriver::PlanetScale(
                planetscale::PlanetScaleDriver::new(config, transport),
            ),
        }
    }

    /// The configuration the driver was made from.
    pub fn config(&self) -> &DriverConfig {
        match self {
            AnyServerlessDriver::Turso(driver) => driver.config(),
            AnyServerlessDriver::D1(driver) => driver.config(),
            AnyServerlessDriver::Neon(driver) => driver.config(),
            AnyServerlessDriver::Tidb(driver) => driver.config(),
            AnyServerlessDriver::PlanetScale(driver) => driver.config(),
        }
    }
}

impl<T: HttpTransport> ServerlessDriver for AnyServerlessDriver<T> {
    fn dialect(&self) -> Dialect {
        match self {
            AnyServerlessDriver::Turso(driver) => driver.dialect(),
            AnyServerlessDriver::D1(driver) => driver.dialect(),
            AnyServerlessDriver::Neon(driver) => driver.dialect(),
            AnyServerlessDriver::Tidb(driver) => driver.dialect(),
            AnyServerlessDriver::PlanetScale(driver) => driver.dialect(),
        }
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        match self {
            AnyServerlessDriver::Turso(driver) => driver.execute(sql, params).await,
            AnyServerlessDriver::D1(driver) => driver.execute(sql, params).await,
            AnyServerlessDriver::Neon(driver) => driver.execute(sql, params).await,
            AnyServerlessDriver::Tidb(driver) => driver.execute(sql, params).await,
            AnyServerlessDriver::PlanetScale(driver) => driver.execute(sql, params).await,
        }
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        match self {
            AnyServerlessDriver::Turso(driver) => driver.batch(stmts).await,
            AnyServerlessDriver::D1(driver) => driver.batch(stmts).await,
            AnyServerlessDriver::Neon(driver) => driver.batch(stmts).await,
            AnyServerlessDriver::Tidb(driver) => driver.batch(stmts).await,
            AnyServerlessDriver::PlanetScale(driver) => driver.batch(stmts).await,
        }
    }
}

/// Run a future to its end.
///
/// The crate has no executor — it is a library, and an executor is the
/// host's business. This one is for the tests, and for a caller who
/// wants a straight line: the futures here never really wait, because a
/// transport that has an answer gives it at once.
#[cfg(any(feature = "std", test))]
pub fn block_on<F: core::future::Future>(future: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);

    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = core::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}

/// The body of a JSON request, as text.
///
/// The crate's own writer, so the bytes on the wire are the bytes the
/// value model says they are.
pub(crate) fn to_json(value: &mosql::Value) -> String {
    mosql::to_json(value)
}

/// Read a JSON answer.
pub(crate) fn parse_json(text: &str) -> Result<mosql::Value, DbError> {
    mosql::parse_json(text).map_err(|e| DbError::new(format!("the answer is not JSON: {e}")))
}

/// An error from a provider's answer.
///
/// The providers all say the same thing in different words: a code, a
/// message, and a status.
pub(crate) fn error_from(status: u16, body: &str, provider: &str) -> DbError {
    let parsed = mosql::parse_json(body).ok();
    let code = parsed.as_ref().and_then(|value| {
        value
            .get("code")
            .or_else(|| value.get("error"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let message = parsed
        .as_ref()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("error_description"))
                .or_else(|| value.get("detail"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                format!("{provider} answered HTTP {status} with nothing")
            } else {
                trimmed.chars().take(400).collect()
            }
        });
    DbError::with_code(
        code.unwrap_or_else(|| format!("HTTP_{status}")),
        message,
        Some(status),
    )
}

/// A mock transport, for the tests.
///
/// It records what it was asked and answers with what it was told to.
#[cfg(any(feature = "std", test))]
#[derive(Debug, Default)]
pub struct MockTransport {
    /// The requests it has seen.
    pub requests: std::sync::Mutex<Vec<MockRequest>>,
    /// The answers to give, in order.
    pub responses: std::sync::Mutex<Vec<MockResponse>>,
}

/// One answer the mock gives.
#[cfg(any(feature = "std", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockResponse {
    /// The status.
    pub status: u16,
    /// The body.
    pub body: String,
    /// The headers, if any.
    pub headers: Vec<(String, String)>,
}

#[cfg(any(feature = "std", test))]
impl MockResponse {
    /// An answer with a status and a body.
    pub fn new(status: u16, body: impl Into<String>) -> MockResponse {
        MockResponse {
            status,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    /// An answer with headers as well.
    pub fn with_headers(
        status: u16,
        body: impl Into<String>,
        headers: Vec<(String, String)>,
    ) -> MockResponse {
        MockResponse {
            status,
            body: body.into(),
            headers,
        }
    }
}

/// One request the mock saw.
#[cfg(any(feature = "std", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockRequest {
    /// The URL that was posted to.
    pub url: String,
    /// The headers that were sent.
    pub headers: Vec<(String, String)>,
    /// The body that was sent.
    pub body: String,
}

#[cfg(any(feature = "std", test))]
impl MockRequest {
    /// The value of a header, if it was sent.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The body, read as JSON.
    pub fn json(&self) -> Result<mosql::Value, DbError> {
        parse_json(&self.body)
    }
}

#[cfg(any(feature = "std", test))]
impl MockTransport {
    /// A mock that answers with these replies, in order.
    pub fn with_responses(responses: Vec<(u16, String)>) -> MockTransport {
        MockTransport {
            requests: std::sync::Mutex::new(Vec::new()),
            responses: std::sync::Mutex::new(
                responses
                    .into_iter()
                    .map(|(status, body)| MockResponse::new(status, body))
                    .collect(),
            ),
        }
    }

    /// A mock that answers with these replies, headers and all.
    pub fn with_full_responses(responses: Vec<MockResponse>) -> MockTransport {
        MockTransport {
            requests: std::sync::Mutex::new(Vec::new()),
            responses: std::sync::Mutex::new(responses),
        }
    }

    /// The requests the mock has seen.
    pub fn requests(&self) -> Vec<MockRequest> {
        self.requests.lock().expect("not poisoned").clone()
    }

    /// The last request the mock saw.
    pub fn last_request(&self) -> Option<MockRequest> {
        self.requests.lock().expect("not poisoned").last().cloned()
    }
}

/// A shared transport is still a transport.
///
/// This is what lets a caller keep a handle on the transport it handed
/// over — a test that wants to see what was asked of it, say — without
/// giving up the one trait the driver talks to.
impl<T: HttpTransport + ?Sized> HttpTransport for alloc::sync::Arc<T> {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError> {
        (**self).post_json(url, headers, body).await
    }

    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String, Vec<(String, String)>), DbError> {
        (**self).post_json_with_headers(url, headers, body).await
    }
}

#[cfg(any(feature = "std", test))]
impl HttpTransport for MockTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError> {
        self.requests
            .lock()
            .expect("not poisoned")
            .push(MockRequest {
                url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body,
            });
        let mut responses = self.responses.lock().expect("not poisoned");
        if responses.is_empty() {
            return Ok((200, "{}".to_string()));
        }
        let response = responses.remove(0);
        Ok((response.status, response.body))
    }

    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String, Vec<(String, String)>), DbError> {
        self.requests
            .lock()
            .expect("not poisoned")
            .push(MockRequest {
                url: url.to_string(),
                headers: headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                body,
            });
        let mut responses = self.responses.lock().expect("not poisoned");
        if responses.is_empty() {
            return Ok((200, "{}".to_string(), Vec::new()));
        }
        let response = responses.remove(0);
        Ok((response.status, response.body, response.headers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::sync::Arc;

    /// A mock that answers everything with an empty JSON object.
    fn transport() -> Arc<MockTransport> {
        Arc::new(MockTransport::default())
    }

    #[test]
    fn the_url_picks_the_driver() {
        let cases: [(&str, Provider, Dialect); 5] = [
            (
                "libsql://db.turso.io?authToken=secret",
                Provider::Turso,
                Dialect::Sqlite,
            ),
            (
                "d1://acct:token@api.cloudflare.com/mydb",
                Provider::D1,
                Dialect::Sqlite,
            ),
            (
                "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb",
                Provider::Neon,
                Dialect::Postgres,
            ),
            (
                "mysql://user:pass@gateway-01.us-east-1.aws.tidbcloud.com/mydb",
                Provider::Tidb,
                Dialect::Mysql,
            ),
            (
                "mysql://user:pass@aws.connect.psdb.cloud/mydb",
                Provider::PlanetScale,
                Dialect::Mysql,
            ),
        ];
        for (url, provider, dialect) in cases {
            let driver = AnyServerlessDriver::from_url(url, transport()).unwrap();
            assert_eq!(driver.config().provider, provider, "{url}");
            assert_eq!(driver.dialect(), dialect, "{url}");
        }
    }

    #[test]
    fn the_driver_posts_to_the_provider() {
        // The transport is shared, so the caller keeps a handle on it and
        // can see what the driver asked of it.
        let mock = Arc::new(MockTransport::with_responses(vec![(
            200,
            r#"{"results":[]}"#.to_string(),
        )]));
        let driver = AnyServerlessDriver::from_url(
            "libsql://db.turso.io?authToken=secret",
            Arc::clone(&mock),
        )
        .unwrap();
        block_on(driver.execute("SELECT 1", &[])).unwrap();
        let request = mock.last_request().expect("a request");
        assert_eq!(request.url, "https://db.turso.io/v2/pipeline");
        assert_eq!(request.header("authorization"), Some("Bearer secret"));
    }

    #[test]
    fn a_bad_url_is_an_error() {
        let err =
            match AnyServerlessDriver::from_url("mysql://user:pass@db.example.com/db", transport())
            {
                Ok(_) => panic!("the URL should have been refused"),
                Err(err) => err,
            };
        assert!(err.message.contains("provider=tidb"), "{}", err.message);
    }
}
