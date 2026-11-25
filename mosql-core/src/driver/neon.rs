//! Neon, and Vercel Postgres — the same API, reached two ways.
//!
//! ```text
//! POST https://{host}/sql
//! Neon-Connection-String: postgres://user:pass@host/db
//! Neon-Raw-Text-Output: true
//! Neon-Array-Mode: true
//!
//! {"query":"SELECT $1","params":["1"]}
//! ```
//!
//! Three headers matter:
//!
//! * `Neon-Connection-String` — the whole connection string, because
//!   Neon is a proxy and the pool behind it is chosen by the string.
//! * `Neon-Raw-Text-Output` — the values come as text, so a number does
//!   not lose its shape on the way through JSON.
//! * `Neon-Array-Mode` — the answer is a list, one entry per statement.
//!
//! With `Neon-Batch: true` several statements go in one request and the
//! whole request is one transaction.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use mosql::{Object, Value};

use crate::driver::encoding::hex_decode;
use crate::driver::sql::rewrite_pg_placeholders;
use crate::driver::{
    error_from, parse_json, to_json, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};

/// A Neon database.
pub struct NeonDriver<T: HttpTransport> {
    config: DriverConfig,
    transport: T,
}

impl<T: HttpTransport> NeonDriver<T> {
    /// A driver for a Neon database.
    pub fn new(config: DriverConfig, transport: T) -> NeonDriver<T> {
        NeonDriver { config, transport }
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
    fn headers(&self, batch: bool) -> Vec<(String, String)> {
        let mut headers = vec![
            ("content-type".to_string(), "application/json".to_string()),
            (
                "neon-connection-string".to_string(),
                self.config.connection_string.clone(),
            ),
            ("neon-raw-text-output".to_string(), "true".to_string()),
            ("neon-array-mode".to_string(), "true".to_string()),
        ];
        if batch {
            headers.push(("neon-batch".to_string(), "true".to_string()));
        }
        headers
    }

    /// The body for one statement.
    fn body(&self, sql: &str, params: &[SqlValue]) -> String {
        let mut body = Object::new();
        body.set("query", Value::str(sql));
        body.set(
            "params",
            Value::Array(params.iter().map(neon_param).collect()),
        );
        to_json(&Value::Object(body))
    }

    /// Post the statements and read the answer.
    async fn run(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        let batch = stmts.len() > 1;
        let body = if batch {
            let mut body = Object::new();
            body.set(
                "statements",
                Value::Array(
                    stmts
                        .iter()
                        .map(|(sql, params)| {
                            let mut statement = Object::new();
                            statement.set("query", Value::str(*sql));
                            statement.set(
                                "params",
                                Value::Array(params.iter().map(neon_param).collect()),
                            );
                            Value::Object(statement)
                        })
                        .collect(),
                ),
            );
            to_json(&Value::Object(body))
        } else {
            let (sql, params) = stmts.first().copied().unwrap_or(("", &[]));
            self.body(sql, params)
        };
        let headers = self.headers(batch);
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let (status, answer) = self
            .transport
            .post_json(&self.config.endpoint, &header_refs, body)
            .await?;
        if status >= 400 {
            return Err(error_from(status, &answer, "neon"));
        }
        read_answer(&answer, stmts.len())
    }
}

/// One value, as Neon wants it.
fn neon_param(value: &SqlValue) -> Value {
    match value {
        SqlValue::Null => Value::Null,
        SqlValue::Bool(flag) => Value::Bool(*flag),
        SqlValue::Int(number) => Value::Int(*number),
        SqlValue::Float(number) => Value::Float(*number),
        SqlValue::Text(text) => Value::str(text.clone()),
        SqlValue::Blob(bytes) => {
            Value::str(format!("\\x{}", crate::driver::encoding::hex_encode(bytes)))
        }
    }
}

/// Read Neon's answer.
///
/// With `Neon-Array-Mode` the answer is a list, one entry per statement.
fn read_answer(body: &str, wanted: usize) -> Result<Vec<QueryResult>, DbError> {
    let value = parse_json(body)?;
    let entries = match value.as_array() {
        Some(list) => list.clone(),
        None => vec![value],
    };
    let mut out = Vec::new();
    for entry in entries {
        // A statement that failed says so in the entry.
        if let Some(error) = entry.get("error") {
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
                code.unwrap_or_else(|| "PG_ERROR".to_string()),
                message,
                None,
            ));
        }
        out.push(read_result(&entry));
    }
    // A single statement answers with one entry; a batch answers with
    // one entry per statement.
    out.truncate(wanted.max(1));
    Ok(out)
}

/// One result, as Neon gives it.
fn read_result(entry: &Value) -> QueryResult {
    let mut out = QueryResult::empty();
    let mut types: Vec<u32> = Vec::new();
    if let Some(fields) = entry.get("fields").and_then(|v| v.as_array()) {
        for field in fields {
            out.columns.push(
                field
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            );
            types.push(
                field
                    .get("dataTypeID")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as u32,
            );
        }
    }
    if let Some(rows) = entry.get("rows").and_then(|v| v.as_array()) {
        for row in rows {
            let Some(cells) = row.as_array() else {
                continue;
            };
            let mut values = Vec::with_capacity(cells.len());
            for (index, cell) in cells.iter().enumerate() {
                let kind = types.get(index).copied().unwrap_or(0);
                values.push(neon_value(cell, kind));
            }
            out.rows.push(values);
        }
    }
    if let Some(count) = entry.get("rowCount").and_then(|v| v.as_f64()) {
        out.rows_affected = count.max(0.0) as u64;
    }
    out.last_insert_id = match entry.get("lastInsertId") {
        Some(Value::Int(id)) => Some(*id),
        Some(Value::Float(id)) => Some(*id as i64),
        _ => None,
    };
    out
}

/// One cell, as Neon gives it, read by its Postgres type.
///
/// The type is what tells a `1` from a `1.0` from a `true`, because
/// with `Neon-Raw-Text-Output` everything arrives as text.
fn neon_value(cell: &Value, kind: u32) -> SqlValue {
    match cell {
        Value::Null => SqlValue::Null,
        Value::Bool(flag) => SqlValue::Bool(*flag),
        Value::Int(number) => SqlValue::Int(*number),
        Value::Float(number) => SqlValue::Float(*number),
        Value::Str(text) => match kind {
            16 => match text.as_str() {
                "t" | "true" | "TRUE" | "1" => SqlValue::Bool(true),
                "f" | "false" | "FALSE" | "0" => SqlValue::Bool(false),
                _ => SqlValue::Text(text.clone()),
            },
            20 | 21 | 23 => match text.parse::<i64>() {
                Ok(number) => SqlValue::Int(number),
                Err(_) => SqlValue::Text(text.clone()),
            },
            700 | 701 => match text.parse::<f64>() {
                Ok(number) => SqlValue::Float(number),
                Err(_) => SqlValue::Text(text.clone()),
            },
            17 => {
                // A `bytea` in its `\x…` form.
                let hex = text.strip_prefix("\\x").unwrap_or(text);
                match hex_decode(hex) {
                    Ok(bytes) => SqlValue::Blob(bytes),
                    Err(_) => SqlValue::Text(text.clone()),
                }
            }
            _ => SqlValue::Text(text.clone()),
        },
        other => SqlValue::Text(to_json(other)),
    }
}

impl<T: HttpTransport> ServerlessDriver for NeonDriver<T> {
    fn dialect(&self) -> Dialect {
        Dialect::Postgres
    }

    async fn execute(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult, DbError> {
        // Postgres counts its parameters from one, and names them `$1`.
        let sql = rewrite_pg_placeholders(sql)?;
        let mut results = self.run(&[(&sql, params)]).await?;
        if results.is_empty() {
            return Ok(QueryResult::empty());
        }
        Ok(results.remove(0))
    }

    async fn batch(&self, stmts: &[(&str, &[SqlValue])]) -> Result<Vec<QueryResult>, DbError> {
        if stmts.is_empty() {
            return Ok(Vec::new());
        }
        let rewritten: Vec<(String, &[SqlValue])> = stmts
            .iter()
            .map(|(sql, params)| Ok((rewrite_pg_placeholders(sql)?, *params)))
            .collect::<Result<Vec<_>, DbError>>()?;
        let borrowed: Vec<(&str, &[SqlValue])> = rewritten
            .iter()
            .map(|(sql, params)| (sql.as_str(), *params))
            .collect();
        self.run(&borrowed).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::MockTransport;

    fn driver(
        responses: Vec<(u16, String)>,
    ) -> (
        NeonDriver<alloc::sync::Arc<MockTransport>>,
        alloc::sync::Arc<MockTransport>,
    ) {
        let config =
            DriverConfig::from_url("postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb")
                .unwrap();
        let transport = alloc::sync::Arc::new(MockTransport::with_responses(responses));
        (
            NeonDriver::new(config, alloc::sync::Arc::clone(&transport)),
            transport,
        )
    }

    #[test]
    fn the_endpoint_and_the_headers() {
        let (driver, transport) = driver(vec![(200, "[]".to_string())]);
        crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap();
        let request = transport.last_request().unwrap();
        assert_eq!(request.url, "https://ep-cool.us-east-2.aws.neon.tech/sql");
        assert_eq!(
            request.header("neon-connection-string"),
            Some("postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb")
        );
        assert_eq!(request.header("neon-raw-text-output"), Some("true"));
        assert_eq!(request.header("neon-array-mode"), Some("true"));
        assert_eq!(
            request.header("neon-batch"),
            None,
            "one statement is not a batch"
        );
    }

    #[test]
    fn the_placeholders_are_renumbered() {
        let (driver, transport) = driver(vec![(200, "[]".to_string())]);
        crate::driver::block_on(driver.execute("SELECT * FROM t WHERE a = ? AND b = ?", &[]))
            .unwrap();
        let request = transport.last_request().unwrap();
        let body = request.json().unwrap();
        assert_eq!(
            body.get("query").and_then(|v| v.as_str()),
            Some("SELECT * FROM t WHERE a = $1 AND b = $2")
        );
    }

    #[test]
    fn a_result_is_read_by_its_type() {
        let answer = r#"[{"fields":[
            {"name":"b","dataTypeID":16},
            {"name":"i","dataTypeID":23},
            {"name":"f","dataTypeID":701},
            {"name":"by","dataTypeID":17},
            {"name":"t","dataTypeID":25}],
            "rows":[["t","42","1.5","\\x000fff","hello"]],
            "rowCount":1}]"#;
        let (driver, _) = driver(vec![(200, answer.to_string())]);
        let result = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap();
        assert_eq!(result.columns, vec!["b", "i", "f", "by", "t"]);
        assert_eq!(result.rows[0][0], SqlValue::Bool(true));
        assert_eq!(result.rows[0][1], SqlValue::Int(42));
        assert_eq!(result.rows[0][2], SqlValue::Float(1.5));
        assert_eq!(result.rows[0][3], SqlValue::Blob(vec![0, 15, 255]));
        assert_eq!(result.rows[0][4], SqlValue::Text("hello".to_string()));
    }

    #[test]
    fn a_batch_asks_for_a_transaction() {
        let (driver, transport) = driver(vec![(200, "[]".to_string())]);
        crate::driver::block_on(driver.batch(&[("a", &[]), ("b", &[])])).unwrap();
        let request = transport.last_request().unwrap();
        assert_eq!(request.header("neon-batch"), Some("true"));
        let body = request.json().unwrap();
        let statements = body.get("statements").and_then(|v| v.as_array()).unwrap();
        assert_eq!(statements.len(), 2);
    }

    #[test]
    fn an_error_is_an_error() {
        let (driver, _) = driver(vec![(
            200,
            r#"[{"error":{"message":"relation does not exist","code":"42P01"}}]"#.to_string(),
        )]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.code.as_deref(), Some("42P01"));
        assert!(err.message.contains("does not exist"), "{}", err.message);
    }

    #[test]
    fn an_http_error_is_an_error() {
        let (driver, _) = driver(vec![(401, r#"{"message":"no"}"#.to_string())]);
        let err = crate::driver::block_on(driver.execute("SELECT 1", &[])).unwrap_err();
        assert_eq!(err.status, Some(401));
    }
}
