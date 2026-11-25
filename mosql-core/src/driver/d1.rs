//! Cloudflare D1.
//!
//! D1's HTTP API takes one statement per request:
//!
//! ```text
//! POST https://api.cloudflare.com/client/v4/accounts/{acct}/d1/database/{db}/query
//! Authorization: Bearer {token}
//!
//! {"sql":"SELECT ?","params":["1"]}
//! ```
//!
//! and answers with a list — one entry per statement, in the order they
//! were sent. There is no batch endpoint on this API, so a batch is run
//! one statement at a time, in order, and stops at the first failure.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mosql::{Object, Value};

use crate::driver::{
    error_from, parse_json, to_json, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};

/// A Cloudflare D1 database.
pub struct D1Driver<T: HttpTransport> {
    config: DriverConfig,
    transport: T,
}

impl<T: HttpTransport> D1Driver<T> {
    /// A driver for a D1 database.
    pub fn new(config: DriverConfig, transport: T) -> D1Driver<T> {
        D1Driver { config, transport }
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
        if let Some(token) = self.config.token.as_deref() {
            headers.push(("authorization".to_string(), format!("Bearer {token}")));
        }
        headers
    }

    /// The body for one statement.
    fn body(&self, sql: &str, params: &[SqlValue]) -> String {
        let mut body = Object::new();
        body.set("sql", Value::str(sql));
        body.set(
            "params",
            Value::Array(params.iter().map(d1_param).collect()),
        );
        to_json(&Value::Object(body))
    }

    /// Post one statement and read the answer.
    async fn run(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        let headers = self.headers();
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer) = self
            .transport
            .post_json(&self.config.endpoint, &header_refs, self.body(sql, params))
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "d1"));
        }
        let value = parse_json(&answer)?;
        if let Some(success) = value.get("success").and_then(|v| v.as_bool()) {
            if !success {
                return Err(d1_error(&value));
            }
        }
        // The answer is a list of results, one per statement sent.
        let results = value
            .get("result")
            .and_then(|v| v.as_array())
            .ok_or_else(|| DbError::new("the answer has no result"))?;
        let Some(first) = results.first() else {
            return Ok(QueryResult::empty());
        };
        if let Some(errors) = first.get("errors").and_then(|v| v.as_array()) {
            if let Some(error) = errors.first() {
                return Err(d1_error(error));
            }
        }
        Ok(read_result(first))
    }
}

/// D1's own error shape.
///
/// The error is either at the top level or in the first entry of an
/// `errors` list, depending on where it happened.
fn d1_error(value: &Value) -> DbError {
    let detail = value
        .get("errors")
        .and_then(|v| v.as_array())
        .and_then(|list| list.first())
        .cloned()
        .unwrap_or_else(|| value.clone());
    let message = detail
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("the statement failed")
        .to_string();
    let code = detail
        .get("code")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    DbError::with_code(
        code.unwrap_or_else(|| "D1_ERROR".to_string()),
        message,
        None,
    )
}

/// One value, as D1 wants it.
///
/// D1 takes its parameters as JSON, and it takes a number as a number —
/// but a JSON number is a double, so a whole number that does not fit
/// exactly would be rounded. D1's own clients send whole numbers as
/// strings, and so does this one.
fn d1_param(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(flag) => Value::Int(if *flag { 1 } else { 0 }),
        SqlValue::Int(number) => Value::str(number.to_string()),
        SqlValue::Float(number) => Value::Float(*number),
        SqlValue::Text(text) => Value::str(text.clone()),
        SqlValue::Blob(bytes) => Value::str(crate::driver::encoding::base64_encode(bytes)),
    }
}

/// One result, as D1 gives it.
fn read_result(result: &Value) -> QueryResult {
    let mut out = QueryResult::empty();
    let meta = result.get("meta");
    if let Some(columns) = result.get("columns").and_then(|v| v.as_array()) {
        for column in columns {
            out.columns.push(
                column
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }
    if let Some(rows) = result.get("results").and_then(|v| v.as_array()) {
        for row in rows {
            let Some(cells) = row.as_array() else {
                continue;
            };
            out.rows.push(cells.iter().map(d1_value).collect());
        }
    }
    if let Some(changes) = meta.and_then(|m| m.get("changes")).and_then(|v| v.as_f64()) {
        out.rows_affected = changes.max(0.0) as u64;
    }
    out.last_insert_id = match meta.and_then(|m| m.get("last_row_id")) {
        Some(Value::Int(id)) => Some(*id),
        Some(Value::Float(id)) => Some(*id as i64),
        _ => None,
    };
    out
}

/// One cell, as D1 gives it.
fn d1_value(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Bool(*flag),
        Value::Int(number) => SqlValue::Int(*number),
        Value::Float(number) => SqlValue::Float(*number),
        Value::Str(text) => SqlValue::Text(text.clone()),
        other => SqlValue::Text(to_json(other)),
    }
}

impl<T: HttpTransport> ServerlessDriver for D1Driver<T> {
    fn dialect(&self) -> Dialect {
        Dialect::Sqlite
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        self.run(sql, params).await
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        // This API has no batch endpoint, so the statements go one at a
        // time, in order. The first failure stops the rest: a batch that
        // carried on after a failure would be a batch nobody could
        // reason about.
        let mut out = Vec::new();
        for (sql, params) in stmts {
            out.push(self.run(sql, params).await?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::MockTransport;

    fn driver(
        responses: Vec<(u16, String)>,
    ) -> (
        D1Driver<alloc::sync::Arc<MockTransport>>,
        alloc::sync::Arc<MockTransport>,
    ) {
        let config =
            DriverConfig::from_url("d1://account123:token456@api.cloudflare.com/mydb").unwrap();
        let transport = alloc::sync::Arc::new(MockTransport::with_responses(responses));
        (
            D1Driver::new(config, alloc::sync::Arc::clone(&transport)),
            transport,
        )
    }

    #[test]
    fn the_endpoint_is_the_d1_query_endpoint() {
        let (driver, _) = driver(vec![]);
        assert_eq!(
            driver.endpoint(),
            "https://api.cloudflare.com/client/v4/accounts/account123/d1/database/mydb/query"
        );
    }

    #[test]
    fn a_statement_is_posted_with_its_params() {
        let (driver, transport) = driver(vec![(
            200,
            r#"{"success":true,"result":[{"success":true,"results":[],"meta":{"changes":0}}]}"#
                .to_string(),
        )]);
        crate::driver::block_on(driver.execute("SELECT ?", &[SqlValue::Int(7)])).unwrap();
        let request = transport.last_request().unwrap();
        assert_eq!(
            request.url,
            "https://api.cloudflare.com/client/v4/accounts/account123/d1/database/mydb/query"
        );
        assert_eq!(request.header("authorization"), Some("Bearer token456"));
        let body = request.json().unwrap();
        assert_eq!(body.get("sql").and_then(|v| v.as_str()), Some("SELECT ?"));
        let params = body.get("params").and_then(|v| v.as_array()).unwrap();
        assert_eq!(params[0].as_str(), Some("7"));
    }

    #[test]
    fn a_result_is_read() {
        let (driver, _) = driver(vec![(
            200,
            r#"{"success":true,"result":[{"success":true,
                "columns":[{"name":"id"},{"name":"name"}],
                "results":[[1,"ada"]],
                "meta":{"changes":1,"last_row_id":42}}]}"#
                .to_string(),
        )]);
        let result = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap();
        assert_eq!(result.columns, vec!["id", "name"]);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0][0], SqlValue::Int(1));
        assert_eq!(result.rows[0][1], SqlValue::Text("ada".to_string()));
        assert_eq!(result.rows_affected, 1);
        assert_eq!(result.last_insert_id, Some(42));
    }

    #[test]
    fn a_failure_in_the_answer_is_an_error() {
        let (driver, _) = driver(vec![(
            200,
            r#"{"success":false,"errors":[{"code":"1002","message":"no such table: t"}]}"#
                .to_string(),
        )]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("1002"));
        assert!(err.message.contains("no such table"), "{}", err.message);
    }

    #[test]
    fn an_http_error_is_an_error() {
        let (driver, _) = driver(vec![(
            403,
            r#"{"errors":[{"code":"9109","message":"bad token"}]}"#.to_string(),
        )]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.status, Some(403));
    }

    #[test]
    fn a_batch_runs_the_statements_in_order() {
        let (driver, transport) = driver(vec![
            (
                200,
                r#"{"success":true,"result":[{"success":true,"results":[],"meta":{"changes":1}}]}"#
                    .to_string(),
            ),
            (
                200,
                r#"{"success":true,"result":[{"success":true,"results":[],"meta":{"changes":2}}]}"#
                    .to_string(),
            ),
        ]);
        let results = crate::driver::block_on(driver.batch(&[("a", &[]), ("b", &[])])).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].rows_affected, 1);
        assert_eq!(results[1].rows_affected, 2);
        let requests = transport.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0]
                .json()
                .unwrap()
                .get("sql")
                .and_then(|v| v.as_str()),
            Some("a")
        );
        assert_eq!(
            requests[1]
                .json()
                .unwrap()
                .get("sql")
                .and_then(|v| v.as_str()),
            Some("b")
        );
    }

    #[test]
    fn a_batch_stops_at_the_first_failure() {
        let (driver, transport) = driver(vec![
            (
                200,
                r#"{"success":true,"result":[{"success":true,"results":[],"meta":{"changes":1}}]}"#
                    .to_string(),
            ),
            (
                200,
                r#"{"success":false,"errors":[{"code":"1002","message":"no such table: t"}]}"#
                    .to_string(),
            ),
        ]);
        let err = crate::driver::block_on(driver.batch(&[("a", &[]), ("b", &[]), ("c", &[])]))
            .unwrap_err();
        assert_eq!(err.code.as_deref(), Some("1002"));
        assert_eq!(
            transport.requests().len(),
            2,
            "the third statement never went"
        );
    }
}
