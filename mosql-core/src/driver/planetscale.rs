//! PlanetScale, over Vitess.
//!
//! ```text
//! POST https://{host}/psdb.v1alpha1.Database/Execute
//! Authorization: Basic {base64(user:password)}
//!
//! {"query":"SELECT 1"}
//! ```
//!
//! Vitess has no place for a parameter list in this request, so the
//! values are put into the statement — escaped, so that a value cannot
//! change the statement.
//!
//! Vitess keeps a session too, and it is in the body:
//!
//! ```json
//! {"result":{"fields":[{"name":"id","type":"INT64"}],"rows":[[1]]},
//!  "session":{"session_id":"…"}}
//! ```
//!
//! The session goes back out with the next request, so a `BEGIN` and a
//! `COMMIT` reach the same session — and a statement that fails gets a
//! `ROLLBACK` before the error is returned.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mosql::{Object, Value};

use crate::driver::sql::inline_mysql_params;
use crate::driver::{
    error_from, parse_json, to_json, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};

/// A PlanetScale database.
pub struct PlanetScaleDriver<T: HttpTransport> {
    config: DriverConfig,
    transport: T,
}

impl<T: HttpTransport> PlanetScaleDriver<T> {
    /// A driver for a PlanetScale database.
    pub fn new(config: DriverConfig, transport: T) -> PlanetScaleDriver<T> {
        PlanetScaleDriver { config, transport }
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
    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(basic) = self.config.basic_auth() {
            headers.push(("authorization".to_string(), basic));
        }
        headers
    }

    /// The body for one statement.
    ///
    /// The values are inlined into the statement, the way MySQL's own
    /// clients do it: Vitess takes the statement as text, and a value
    /// that is escaped cannot change what the statement says.
    fn body(
        &self,
        sql: &str,
        params: &[SqlValue],
        session: Option<&Value>,
    ) -> Result<String, DbError> {
        let statement = inline_mysql_params(sql, params)?;
        let mut body = Object::new();
        body.set("query", Value::str(statement));
        if let Some(session) = session {
            body.set("session", session.clone());
        }
        Ok(to_json(&Value::Object(body)))
    }

    /// Post one statement and read the answer.
    async fn run(
        &self,
        sql: &str,
        params: &[SqlValue],
        session: Option<&Value>,
    ) -> Result<(QueryResult, Option<Value>), DbError> {
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer) = self
            .transport
            .post_json(
                &self.config.endpoint,
                &header_refs,
                self.body(sql, params, session)?,
            )
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "planetscale"));
        }
        let value = parse_json(&answer)?;
        // Vitess says "no" in the body, with a 200.
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
        if let Some(error) = value.get("error") {
            let message = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("the statement failed")
                .to_string();
            let code = error
                .get("code")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            return Err(DbError::with_code(
                code.unwrap_or_else(|| "VITESS_ERROR".to_string()),
                message,
                None,
            ));
        }
        let next = value.get("session").cloned();
        let result = value.get("result").cloned().unwrap_or(Value::Null);
        Ok((read_result(&result), next))
    }
}

/// One result, as Vitess gives it.
fn read_result(result: &Value) -> QueryResult {
    let mut out = QueryResult::empty();
    if let Some(fields) = result.get("fields").and_then(|v| v.as_array()) {
        for field in fields {
            let name = field
                .get("name")
                .and_then(|v| v.as_str())
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
    if let Some(affected) = result.get("rows_affected").and_then(|v| v.as_f64()) {
        out.rows_affected = affected.max(0.0) as u64;
    }
    out.last_insert_id = match result.get("insert_id") {
        Some(Value::Int(id)) => Some(*id),
        Some(Value::Float(id)) => Some(*id as i64),
        _ => None,
    };
    out
}

/// One cell, as Vitess gives it.
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

impl<T: HttpTransport> ServerlessDriver for PlanetScaleDriver<T> {
    fn dialect(&self) -> Dialect {
        Dialect::Mysql
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        let (result, _) = self.run(sql, params, None).await?;
        Ok(result)
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        if stmts.is_empty() {
            return Ok(Vec::new());
        }
        let mut session: Option<Value> = None;
        let (_, opened) = self.run("BEGIN", &[], None).await?;
        if let Some(opened) = opened {
            session = Some(opened);
        }

        let mut results: Vec<QueryResult> = Vec::new();
        for (sql, params) in stmts {
            match self.run(sql, params, session.as_ref()).await {
                Ok((result, next)) => {
                    if let Some(next) = next {
                        session = Some(next);
                    }
                    results.push(result);
                }
                Err(e) => {
                    // The transaction is open and something failed: undo
                    // it, so the rows are not left half-written.
                    let _ = self.run("ROLLBACK", &[], session.as_ref()).await;
                    return Err(e);
                }
            }
        }

        self.run("COMMIT", &[], session.as_ref()).await?;
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::MockTransport;

    fn driver(
        responses: Vec<(u16, String)>,
    ) -> (
        PlanetScaleDriver<alloc::sync::Arc<MockTransport>>,
        alloc::sync::Arc<MockTransport>,
    ) {
        let config =
            DriverConfig::from_url("mysql://user:pass@aws.connect.psdb.cloud/mydb").unwrap();
        let transport = alloc::sync::Arc::new(MockTransport::with_responses(responses));
        (
            PlanetScaleDriver::new(config, alloc::sync::Arc::clone(&transport)),
            transport,
        )
    }

    #[test]
    fn the_endpoint_is_the_vitess_one() {
        let (driver, _) = driver(vec![]);
        assert_eq!(
            driver.endpoint(),
            "https://aws.connect.psdb.cloud/psdb.v1alpha1.Database/Execute"
        );
    }

    #[test]
    fn a_statement_is_posted_with_its_parameters() {
        let (driver, transport) = driver(vec![(
            200,
            r#"{"result":{"fields":[{"name":"n","type":"INT64"}],"rows":[[1]],"rows_affected":0}}"#
                .to_string(),
        )]);
        let result =
            crate::driver::block_on(driver.execute("SELECT ?", &[SqlValue::Int(1)])).unwrap();
        assert_eq!(result.columns, vec!["n"]);
        assert_eq!(result.rows[0][0], SqlValue::Int(1));
        let request = transport.last_request().unwrap();
        assert_eq!(request.header("authorization"), Some("Basic dXNlcjpwYXNz"));
        let body = request.json().unwrap();
        // The value is in the statement, escaped.
        assert_eq!(body.get("query").and_then(|v| v.as_str()), Some("SELECT 1"));
        assert!(
            body.get("parameters").is_none(),
            "Vitess has no place for a parameter list"
        );
    }

    #[test]
    fn a_session_travels_with_the_request() {
        let (driver, transport) = driver(vec![
            (
                200,
                r#"{"result":{"fields":[],"rows":[]},"session":{"session_id":"abc"}}"#.to_string(),
            ),
            (
                200,
                r#"{"result":{"fields":[],"rows":[]},"session":{"session_id":"def"}}"#.to_string(),
            ),
        ]);
        crate::driver::block_on(driver.batch(&[("SELECT 1", &[])])).unwrap();
        let requests = transport.requests();
        // BEGIN, the statement, COMMIT.
        assert_eq!(requests.len(), 3);
        // The statement carries the session the BEGIN returned.
        let body = requests[1].json().unwrap();
        let session = body.get("session").unwrap();
        assert_eq!(
            session.get("session_id").and_then(|v| v.as_str()),
            Some("abc")
        );
    }

    #[test]
    fn a_failure_rolls_the_transaction_back() {
        let (driver, transport) = driver(vec![
            (
                200,
                r#"{"result":{"fields":[],"rows":[]},"session":{"session_id":"abc"}}"#.to_string(),
            ),
            (
                200,
                r#"{"error":{"code":"ALREADY_EXISTS","message":"duplicate"}}"#.to_string(),
            ),
            (200, r#"{"result":{"fields":[],"rows":[]}}"#.to_string()),
        ]);
        let err = crate::driver::block_on(driver.batch(&[("INSERT …", &[])])).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("ALREADY_EXISTS"));
        let requests = transport.requests();
        assert_eq!(requests.len(), 3, "BEGIN, the statement and the ROLLBACK");
        let body = requests[2].json().unwrap();
        assert_eq!(body.get("query").and_then(|v| v.as_str()), Some("ROLLBACK"));
    }

    #[test]
    fn an_http_error_is_an_error() {
        let (driver, _) = driver(vec![(401, r#"{"message":"no"}"#.to_string())]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.status, Some(401));
    }
}
