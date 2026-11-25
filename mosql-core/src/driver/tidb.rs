//! TiDB Cloud Serverless.
//!
//! ```text
//! POST https://http-{host}/v1beta/sql
//! Authorization: Basic {base64(user:password)}
//! TiDB-Database: {database}
//!
//! {"statements":["SELECT 1"]}
//! ```
//!
//! The endpoint is not the database's own host: TiDB puts a **proxy** in
//! front, and the proxy's host is the database's host with an `http-`
//! in front of it.
//!
//! The proxy keeps a session, and the session travels in a header:
//! `TiDB-Session` comes back with an answer and goes out with the next
//! request. That is what makes `BEGIN` … `COMMIT` work — the two
//! statements have to reach the same session, or they are two separate
//! conversations.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mosql::{Object, Value};

use crate::driver::sql::inline_mysql_params;
use crate::driver::{
    error_from, parse_json, to_json, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};

/// A TiDB Cloud Serverless database.
pub struct TidbDriver<T: HttpTransport> {
    config: DriverConfig,
    transport: T,
}

impl<T: HttpTransport> TidbDriver<T> {
    /// A driver for a TiDB database.
    pub fn new(config: DriverConfig, transport: T) -> TidbDriver<T> {
        TidbDriver { config, transport }
    }

    /// The configuration the driver was made from.
    pub fn config(&self) -> &DriverConfig {
        &self.config
    }

    /// The endpoint the driver posts to.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// The headers a request carries.
    fn headers(&self, session: Option<&str>) -> Vec<(String, String)> {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(basic) = self.config.basic_auth() {
            headers.push(("authorization".to_string(), basic));
        }
        if let Some(database) = self.config.database.as_deref() {
            headers.push(("tidb-database".to_string(), database.to_string()));
        }
        if let Some(session) = session {
            headers.push(("tidb-session".to_string(), session.to_string()));
        }
        headers
    }

    /// The body for a list of statements.
    fn body(&self, stmts: &[String]) -> String {
        let mut body = Object::new();
        body.set(
            "statements",
            Value::Array(stmts.iter().map(|s| Value::str(s.clone())).collect()),
        );
        to_json(&Value::Object(body))
    }

    /// Post the statements and read the answer.
    ///
    /// The session token comes back — in the `TiDB-Session` header, or
    /// in the body for a proxy that puts it there — and the caller
    /// keeps it: it is what holds a transaction together.
    async fn run(
        &self,
        stmts: &[String],
        session: Option<&str>,
    ) -> Result<(Vec<QueryResult>, Option<String>), DbError> {
        let headers = self.headers(session);
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer, response_headers) = self
            .transport
            .post_json_with_headers(&self.config.endpoint, &header_refs, self.body(stmts))
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "tidb"));
        }
        let value = parse_json(&answer)?;
        // The proxy says "no" in the body, with a 200.
        if let Some(code) = value.get("code").and_then(|v| v.as_f64()) {
            if code as i64 != 200 {
                let message = value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("the statement failed")
                    .to_string();
                return Err(DbError::with_code(
                    format!("{}", code as i64),
                    message,
                    Some(status),
                ));
            }
        }
        let results = value
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        // The session, wherever the proxy put it.
        let session = response_headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("tidb-session"))
            .map(|(_, value)| value.clone())
            .or_else(|| {
                value
                    .get("session")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            });
        Ok((results.iter().map(read_result).collect(), session))
    }
}

/// One result, as TiDB gives it.
fn read_result(entry: &Value) -> QueryResult {
    let mut out = QueryResult::empty();
    let result = entry.get("result").unwrap_or(entry);
    if let Some(fields) = result.get("fields").and_then(|v| v.as_array()) {
        for field in fields {
            let name = field
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| field.get("field").and_then(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            out.columns.push(name);
        }
    }
    if let Some(rows) = result.get("rows").and_then(|v| v.as_array()) {
        for row in rows {
            let Some(cells) = row.as_array() else {
                continue;
            };
            out.rows.push(cells.iter().map(mysql_value).collect());
        }
    }
    if let Some(affected) = result.get("affectedRows").and_then(|v| v.as_f64()) {
        out.rows_affected = affected.max(0.0) as u64;
    }
    out.last_insert_id = match result.get("insertId").and_then(|v| v.as_f64()) {
        Some(id) if id > 0.0 => Some(id as i64),
        _ => None,
    };
    out
}

/// One cell, as TiDB gives it.
fn mysql_value(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Bool(*flag),
        Value::Int(number) => SqlValue::Int(*number),
        Value::Float(number) => SqlValue::Float(*number),
        Value::Str(text) => SqlValue::Text(text.clone()),
        other => SqlValue::Text(to_json(other)),
    }
}

impl<T: HttpTransport> ServerlessDriver for TidbDriver<T> {
    fn dialect(&self) -> Dialect {
        Dialect::Mysql
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        let statement = inline_mysql_params(sql, params)?;
        let (results, _) = self.run(&[statement], None).await?;
        Ok(results.into_iter().next().unwrap_or_default())
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        if stmts.is_empty() {
            return Ok(Vec::new());
        }
        let mut statements = Vec::with_capacity(stmts.len());
        for (sql, params) in stmts {
            statements.push(inline_mysql_params(sql, params)?);
        }

        // The session travels with the request: without it the BEGIN and
        // the COMMIT would be two different conversations, and the
        // transaction would be a wish.
        let mut session: Option<String> = None;
        let (_, opened) = self.run(&["BEGIN".to_string()], session.as_deref()).await?;
        if let Some(opened) = opened {
            session = Some(opened);
        }

        let mut results: Vec<QueryResult> = Vec::new();
        for statement in &statements {
            match self
                .run(core::slice::from_ref(statement), session.as_deref())
                .await
            {
                Ok((answers, next)) => {
                    if let Some(next) = next {
                        session = Some(next);
                    }
                    results.extend(answers);
                }
                Err(e) => {
                    // The transaction is open and something failed: undo
                    // it, so the rows are not left half-written.
                    let _ = self
                        .run(&["ROLLBACK".to_string()], session.as_deref())
                        .await;
                    return Err(e);
                }
            }
        }

        self.run(&["COMMIT".to_string()], session.as_deref())
            .await?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{MockResponse, MockTransport};

    fn driver(
        responses: Vec<(u16, String)>,
    ) -> (
        TidbDriver<alloc::sync::Arc<MockTransport>>,
        alloc::sync::Arc<MockTransport>,
    ) {
        let config = DriverConfig::from_url(
            "mysql://user:pass@gateway-01.us-east-1.aws.tidbcloud.com:4000/mydb",
        )
        .unwrap();
        let transport = alloc::sync::Arc::new(MockTransport::with_responses(responses));
        (
            TidbDriver::new(config, alloc::sync::Arc::clone(&transport)),
            transport,
        )
    }

    #[test]
    fn the_endpoint_is_the_proxy() {
        let (driver, _) = driver(vec![]);
        assert_eq!(
            driver.endpoint(),
            "https://http-gateway-01.us-east-1.aws.tidbcloud.com/v1beta/sql"
        );
    }

    #[test]
    fn the_headers_carry_the_credentials_and_the_database() {
        let (driver, _) = driver(vec![]);
        let headers = driver.headers(None);
        assert!(headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Basic dXNlcjpwYXNz"));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "tidb-database" && v == "mydb"));
    }

    #[test]
    fn a_statement_is_posted() {
        let (driver, transport) = driver(vec![(
            200,
            r#"{"code":200,"message":"","data":[{"type":"EXECUTE","result":{"fields":[{"name":"n"}],"rows":[[1]],"affectedRows":0}}]}"#
                .to_string(),
        )]);
        let result =
            crate::driver::block_on(driver.execute("SELECT ?", &[SqlValue::Int(1)])).unwrap();
        assert_eq!(result.columns, vec!["n"]);
        assert_eq!(result.rows[0][0], SqlValue::Int(1));
        let request = transport.last_request().unwrap();
        let body = request.json().unwrap();
        let statements = body.get("statements").and_then(|v| v.as_array()).unwrap();
        assert_eq!(statements[0].as_str(), Some("SELECT 1"));
    }

    #[test]
    fn a_batch_is_a_transaction() {
        let (driver, transport) = driver(vec![
            (200, r#"{"code":200,"data":[]}"#.to_string()),
            (
                200,
                r#"{"code":200,"data":[{"result":{"fields":[],"rows":[[1]]}}]}"#.to_string(),
            ),
            (200, r#"{"code":200,"data":[]}"#.to_string()),
        ]);
        let results = crate::driver::block_on(driver.batch(&[("SELECT 1", &[])])).unwrap();
        assert_eq!(results.len(), 1);
        let requests = transport.requests();
        assert_eq!(requests.len(), 3, "BEGIN, the statement and COMMIT");
        let first = requests[0].json().unwrap();
        assert_eq!(
            first.get("statements").and_then(|v| v.as_array()).unwrap()[0].as_str(),
            Some("BEGIN")
        );
        let last = requests[2].json().unwrap();
        assert_eq!(
            last.get("statements").and_then(|v| v.as_array()).unwrap()[0].as_str(),
            Some("COMMIT")
        );
    }

    #[test]
    fn a_session_travels_with_the_request() {
        // The session comes back in a response *header*, and goes out in
        // a request header. Without it the BEGIN and the COMMIT would be
        // two different conversations.
        let transport = alloc::sync::Arc::new(MockTransport::with_full_responses(vec![
            MockResponse::with_headers(
                200,
                r#"{"code":200,"data":[]}"#.to_string(),
                vec![("TiDB-Session".to_string(), "abc".to_string())],
            ),
            MockResponse::with_headers(
                200,
                r#"{"code":200,"data":[]}"#.to_string(),
                vec![("TiDB-Session".to_string(), "def".to_string())],
            ),
            MockResponse::with_headers(
                200,
                r#"{"code":200,"data":[]}"#.to_string(),
                vec![("TiDB-Session".to_string(), "ghi".to_string())],
            ),
        ]));
        let config = DriverConfig::from_url(
            "mysql://user:pass@gateway-01.us-east-1.aws.tidbcloud.com:4000/mydb",
        )
        .unwrap();
        let driver = TidbDriver::new(config, alloc::sync::Arc::clone(&transport));
        crate::driver::block_on(driver.batch(&[("SELECT 1", &[])])).unwrap();
        let requests = transport.requests();
        // BEGIN, the statement, COMMIT.
        assert_eq!(requests.len(), 3);
        // The BEGIN carries no session: there is not one yet.
        assert_eq!(requests[0].header("tidb-session"), None);
        // The statement carries the session the BEGIN returned.
        assert_eq!(requests[1].header("tidb-session"), Some("abc"));
        // And the COMMIT carries the session the statement returned.
        assert_eq!(requests[2].header("tidb-session"), Some("def"));
    }

    #[test]
    fn a_failure_rolls_the_transaction_back() {
        let (driver, transport) = driver(vec![
            (200, r#"{"code":200,"data":[]}"#.to_string()),
            (
                200,
                r#"{"code":1105,"message":"unknown column 'x'","data":null}"#.to_string(),
            ),
            (200, r#"{"code":200,"data":[]}"#.to_string()),
        ]);
        let err = crate::driver::block_on(driver.batch(&[("SELECT x", &[])])).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("1105"));
        let requests = transport.requests();
        assert_eq!(requests.len(), 3, "BEGIN, the statement and the ROLLBACK");
        let statements = requests[2]
            .json()
            .unwrap()
            .get("statements")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap();
        assert_eq!(statements[0].as_str(), Some("ROLLBACK"));
    }

    #[test]
    fn a_failure_in_the_body_is_an_error() {
        let (driver, _) = driver(vec![(
            200,
            r#"{"code":1105,"message":"unknown column 'x'","data":null}"#.to_string(),
        )]);
        let err = crate::driver::block_on(driver.execute("SELECT x", &[])).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("1105"));
        assert!(err.message.contains("unknown column"), "{}", err.message);
    }

    #[test]
    fn an_http_error_is_an_error() {
        let (driver, _) = driver(vec![(401, r#"{"message":"no"}"#.to_string())]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.status, Some(401));
    }
}
