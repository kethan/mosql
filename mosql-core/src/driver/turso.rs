//! Turso, over libSQL's HTTP pipeline.
//!
//! Turso speaks one protocol: a **pipeline** is a list of requests, sent
//! as one POST, and the answer is a list of results in the same order.
//! The pipeline is the batch — there is no other way to run several
//! statements in one round trip.
//!
//! ```text
//! POST https://{host}/v2/pipeline
//! Authorization: Bearer {token}
//!
//! {"requests":[{"type":"execute","stmt":{"sql":"SELECT ?","args":["1"]}},
//!              {"type":"close"}]}
//! ```
//!
//! Integers and floats go as **strings**, because a JSON number cannot
//! hold every `i64` exactly and a database row can. Blobs go as
//! base64.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mosql::{Object, Value};

use crate::driver::encoding::base64_encode;
use crate::driver::{
    error_from, parse_json, to_json, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};

/// A Turso database.
pub struct TursoDriver<T: HttpTransport> {
    config: DriverConfig,
    transport: T,
}

impl<T: HttpTransport> TursoDriver<T> {
    /// A driver for a Turso database.
    pub fn new(config: DriverConfig, transport: T) -> TursoDriver<T> {
        TursoDriver { config, transport }
    }

    /// The configuration the driver was made from.
    pub fn config(&self) -> &DriverConfig {
        &self.config
    }

    /// The endpoint the driver posts to.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }

    /// The pipeline for a list of statements.
    fn pipeline(&self, stmts: &[(&str, &[SqlValue])]) -> String {
        let mut requests = Vec::new();
        for (sql, params) in stmts {
            let mut stmt = Object::new();
            stmt.set("sql", Value::str(*sql));
            stmt.set(
                "args",
                Value::Array(params.iter().map(libsql_arg).collect()),
            );
            let mut request = Object::new();
            request.set("type", Value::str("execute"));
            request.set("stmt", Value::Object(stmt));
            requests.push(Value::Object(request));
        }
        // The pipeline is closed at the end, so the connection does not
        // wait for a "close" that never comes.
        let mut close = Object::new();
        close.set("type", Value::str("close"));
        requests.push(Value::Object(close));

        let mut body = Object::new();
        body.set("requests", Value::Array(requests));
        to_json(&Value::Object(body))
    }

    /// The headers a request carries.
    fn headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
        if let Some(token) = self.config.token.as_deref() {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        headers
    }

    /// Read the pipeline's answer.
    fn read(body: &str) -> Result<Vec<QueryResult>, DbError> {
        let value = parse_json(body)?;
        let results = value
            .get("results")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DbError::new("the answer has no results"))?;
        let mut out = Vec::new();
        for result in results {
            let kind = result.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if kind == "error" {
                let error = result.get("error");
                let message = error
                    .and_then(|e| e.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("the statement failed")
                    .to_string();
                let code = error
                    .and_then(|e| e.get("code"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                return Err(DbError::with_code(
                    code.unwrap_or_else(|| "SQLITE_ERROR".to_string()),
                    message,
                    None,
                ));
            }
            // `{"type":"ok","response":{"type":"execute","result":{…}}}`
            let payload = result
                .get("response")
                .and_then(|r| r.get("result"))
                .cloned()
                .unwrap_or(Value::Null);
            out.push(read_result(&payload));
        }
        Ok(out)
    }
}

/// One value, as libSQL's pipeline wants it.
fn libsql_arg(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(flag) => Value::Bool(*flag),
        // A number goes as a string: a JSON number cannot hold every
        // i64 exactly, and a row can.
        SqlValue::Int(number) => Value::str(number.to_string()),
        SqlValue::Float(number) => Value::str(number.to_string()),
        SqlValue::Text(text) => Value::str(text.clone()),
        SqlValue::Blob(bytes) => Value::str(base64_encode(bytes)),
    }
}

/// One result, as libSQL's pipeline gives it.
fn read_result(payload: &Value) -> QueryResult {
    let mut out = QueryResult::empty();
    if let Some(cols) = payload.get("cols").and_then(|v| v.as_array()) {
        for col in cols {
            let name = col
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            out.columns.push(name);
        }
    }
    if let Some(rows) = payload.get("rows").and_then(|v| v.as_array()) {
        for row in rows {
            let Some(cells) = row.as_array() else {
                continue;
            };
            out.rows.push(
                cells
                    .iter()
                    .map(|cell| match cell {
                        Value::Str(text) => SqlValue::Text(text.clone()),
                        Value::Int(number) => SqlValue::Int(*number),
                        Value::Float(number) => SqlValue::Float(*number),
                        Value::Bool(flag) => SqlValue::Bool(*flag),
                        Value::Null => SqlValue::Null,
                        other => SqlValue::Text(to_json(other)),
                    })
                    .collect(),
            );
        }
    }
    if let Some(affected) = payload.get("affected_row_count").and_then(|v| v.as_f64()) {
        out.rows_affected = affected.max(0.0) as u64;
    }
    out.last_insert_id = match payload.get("last_insert_rowid") {
        Some(Value::Int(id)) => Some(*id),
        Some(Value::Float(id)) => Some(*id as i64),
        _ => None,
    };
    out
}

impl<T: HttpTransport> ServerlessDriver for TursoDriver<T> {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        let body = self.pipeline(&[(sql, params)]);
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer) = self
            .transport
            .post_json(&self.config.endpoint, &header_refs, body)
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "turso"));
        }
        let mut results = Self::read(&answer)?;
        if results.is_empty() {
            return Ok(QueryResult::empty());
        }
        Ok(results.remove(0))
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        if stmts.is_empty() {
            return Ok(Vec::new());
        }
        let body = self.pipeline(stmts);
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer) = self
            .transport
            .post_json(&self.config.endpoint, &header_refs, body)
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "turso"));
        }
        // The pipeline ends with a `close`, which has no result. Drop it.
        let mut results = Self::read(&answer)?;
        results.truncate(stmts.len());
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
        TursoDriver<alloc::sync::Arc<MockTransport>>,
        alloc::sync::Arc<MockTransport>,
    ) {
        let config = DriverConfig::from_url("libsql://db.turso.io?authToken=secret").unwrap();
        let transport = alloc::sync::Arc::new(MockTransport::with_responses(responses));
        (
            TursoDriver::new(config, alloc::sync::Arc::clone(&transport)),
            transport,
        )
    }

    #[test]
    fn the_endpoint_and_the_token() {
        let (driver, _) = driver(vec![]);
        assert_eq!(driver.endpoint(), "https://db.turso.io/v2/pipeline");
        assert_eq!(driver.config().token.as_deref(), Some("secret"));
    }

    #[test]
    fn a_statement_is_posted_as_a_pipeline() {
        let (driver, transport) = driver(vec![(
            200,
            r#"{"results":[{"type":"ok","response":{"type":"execute","result":{"cols":[],"rows":[]}}}]}"#
                .to_string(),
        )]);
        let body = driver.pipeline(&[("SELECT ?", &[SqlValue::Int(1)])]);
        let value = mosql::parse_json(&body).unwrap();
        let requests = value.get("requests").and_then(|v| v.as_array()).unwrap();
        assert_eq!(requests.len(), 2, "the statement and the close");
        assert_eq!(
            requests[0].get("type").and_then(|v| v.as_str()),
            Some("execute")
        );
        let stmt = requests[0].get("stmt").unwrap();
        assert_eq!(stmt.get("sql").and_then(|v| v.as_str()), Some("SELECT ?"));
        let args = stmt.get("args").and_then(|v| v.as_array()).unwrap();
        assert_eq!(args[0].as_str(), Some("1"), "an integer goes as a string");
        assert_eq!(
            requests[1].get("type").and_then(|v| v.as_str()),
            Some("close")
        );
        let _ = transport;
    }

    #[test]
    fn a_statement_goes_to_the_endpoint() {
        let (driver, transport) = driver(vec![(
            200,
            r#"{"results":[{"type":"ok","response":{"type":"execute","result":{"cols":[],"rows":[]}}}]}"#
                .to_string(),
        )]);
        let result = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap();
        assert_eq!(result.rows.len(), 0);
        let request = transport.last_request().expect("a request");
        assert_eq!(request.url, "https://db.turso.io/v2/pipeline");
        assert_eq!(request.header("authorization"), Some("Bearer secret"));
        assert_eq!(request.header("content-type"), Some("application/json"));
    }

    #[test]
    fn an_http_error_is_an_error() {
        let (driver, _) = driver(vec![(401, r#"{"error":"bad token"}"#.to_string())]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.status, Some(401));
    }

    #[test]
    fn a_blob_goes_as_base64() {
        let (driver, _) = driver(vec![]);
        let body = driver.pipeline(&[("SELECT ?", &[SqlValue::Blob(vec![1, 2, 3])])]);
        let value = mosql::parse_json(&body).unwrap();
        let args = value.get("requests").and_then(|v| v.as_array()).unwrap()[0]
            .get("stmt")
            .and_then(|s| s.get("args"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(args[0].as_str(), Some("AQID"));
    }

    #[test]
    fn a_result_is_read() {
        let answer = r#"{"results":[{"type":"ok","response":{"type":"execute","result":{
            "cols":[{"name":"id"},{"name":"name"}],
            "rows":[[1,"ada"],[2,"grace"]],
            "affected_row_count":2,
            "last_insert_rowid":7}}}]}"#;
        let results = TursoDriver::<MockTransport>::read(answer).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].columns, vec!["id", "name"]);
        assert_eq!(results[0].rows.len(), 2);
        assert_eq!(results[0].rows[0][0], SqlValue::Int(1));
        assert_eq!(results[0].rows[0][1], SqlValue::Text("ada".to_string()));
        assert_eq!(results[0].rows_affected, 2);
        assert_eq!(results[0].last_insert_id, Some(7));
    }

    #[test]
    fn an_error_in_the_answer_is_an_error() {
        let answer = r#"{"results":[{"type":"error","error":{"message":"no such table: t","code":"SQLITE_ERROR"}}]}"#;
        let err = TursoDriver::<MockTransport>::read(answer).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("SQLITE_ERROR"));
        assert!(err.message.contains("no such table"), "{}", err.message);
    }

    #[test]
    fn the_headers_carry_the_token() {
        let (driver, _) = driver(vec![]);
        let headers = driver.headers();
        assert!(headers
            .iter()
            .any(|(k, v)| k == "authorization" && v == "Bearer secret"));
        assert!(headers
            .iter()
            .any(|(k, v)| k == "content-type" && v == "application/json"));
    }
}
