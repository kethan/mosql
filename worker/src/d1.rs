//! The store, over D1.
//!
//! D1 is the durable store: a SQLite database the platform keeps, reached
//! over its own API rather than over a file. The eight operations of
//! [`AuthStore`] become eight statements, and the statements are the ones
//! the native SQLite store already uses — they are built by
//! `better_auth_core::sql`, so both stores speak exactly the same SQL and
//! read rows back exactly the same way.
//!
//! What is different is how a statement is run. There is no connection to
//! hold and no call to make: the engine asks the host to run the statement
//! ([`Bus::call`]), stops there, and is continued when the answer comes
//! back.

use std::sync::Arc;

use better_auth_core::error::{AuthError, AuthResult};
use better_auth_core::models::{RecordMap, MODEL_SESSION};
use better_auth_core::sql::{
    count_sql, delete_sql, insert_sql, row_to_record, select_one_sql, select_sql, update_sql,
};
use better_auth_core::store::{AuthStore, BoxFuture, FilterOp, FindManyQuery, WhereClause};

use crate::bus::Bus;

/// A row of a result set: the columns, in order, and their values.
type Row = (Vec<String>, Vec<mosql::Value>);

/// What a statement produced.
#[derive(Debug, Default)]
struct Answer {
    rows: Vec<Row>,
    changes: u64,
}

/// The store, over the platform's database.
pub struct D1Store {
    bus: Bus,
}

impl D1Store {
    /// A store that asks the host to run its statements.
    pub fn new(bus: Bus) -> D1Store {
        D1Store { bus }
    }

    /// Run a statement, and read back what it produced.
    async fn run(
        &self,
        sql: &str,
        params: &[mosql::Value],
        wants_rows: bool,
    ) -> Result<Answer, AuthError> {
        let mut op = mosql::Object::new();
        op.set("kind", mosql::Value::str("query"));
        op.set("sql", mosql::Value::str(sql));
        op.set("params", mosql::Value::Array(params.to_vec()));
        op.set("rows", mosql::Value::Bool(wants_rows));
        let answer = self
            .bus
            .call(mosql::Value::Object(op))
            .await
            .map_err(AuthError::internal)?;
        decode(&answer)
    }

    /// The rows a statement produced, as records.
    async fn records(
        &self,
        model: &str,
        sql: &str,
        params: &[mosql::Value],
    ) -> AuthResult<Vec<RecordMap>> {
        let answer = self.run(sql, params, true).await?;
        Ok(answer
            .rows
            .iter()
            .map(|(columns, values)| row_to_record(model, columns, values))
            .collect())
    }

    /// How many rows a statement changed.
    async fn changes(&self, sql: &str, params: &[mosql::Value]) -> AuthResult<u64> {
        Ok(self.run(sql, params, false).await?.changes)
    }
}

/// Read the host's answer.
fn decode(answer: &mosql::Value) -> Result<Answer, AuthError> {
    if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
        return Err(AuthError::internal(message.to_string()));
    }
    let mut rows = Vec::new();
    if let Some(items) = answer.get("rows").and_then(|v| v.as_array()) {
        for item in items {
            let columns = item
                .get("columns")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let values = item
                .get("values")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            rows.push((columns, values));
        }
    }
    let changes = answer
        .get("changes")
        .and_then(|v| v.as_f64())
        .map(|n| if n < 0.0 { 0 } else { n as u64 })
        .unwrap_or(0);
    Ok(Answer { rows, changes })
}

/// The tokens a clause names, when it names exactly one.
///
/// The cache is keyed by token, so a clause that names one can be
/// invalidated precisely. A clause that does not names none, and the
/// caller falls back to invalidating everything.
pub fn named_tokens(r#where: &[WhereClause]) -> Vec<String> {
    r#where
        .iter()
        .filter(|clause| clause.field == "token" && clause.op == FilterOp::Eq)
        .filter_map(|clause| clause.value.as_str().map(str::to_string))
        .collect()
}

/// The store, over the platform's database.
impl AuthStore for D1Store {
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>> {
        Box::pin(async move {
            let record = better_auth_core::store::prepare_create(model, data)?;
            let (sql, params) = insert_sql(model, &record)?;
            self.changes(&sql, &params).await?;
            Ok(record)
        })
    }

    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        Box::pin(async move {
            let (sql, params) = select_one_sql(model, r#where)?;
            Ok(self.records(model, &sql, &params).await?.into_iter().next())
        })
    }

    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>> {
        Box::pin(async move {
            let (sql, params) = select_sql(model, &query)?;
            self.records(model, &sql, &params).await
        })
    }

    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        Box::pin(async move {
            let (sql, params) = count_sql(model, r#where)?;
            let answer = self.run(&sql, &params, true).await?;
            Ok(answer
                .rows
                .first()
                .and_then(|(_, values)| values.first())
                .and_then(|v| v.as_f64())
                .map(|n| if n < 0.0 { 0 } else { n as u64 })
                .unwrap_or(0))
        })
    }

    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        Box::pin(async move {
            let changed = self.update_many(model, r#where, update).await?;
            if changed == 0 {
                return Ok(None);
            }
            let (sql, params) = select_one_sql(model, r#where)?;
            Ok(self.records(model, &sql, &params).await?.into_iter().next())
        })
    }

    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>> {
        Box::pin(async move {
            let Some((sql, params)) = update_sql(model, r#where, &update)? else {
                return Ok(0);
            };
            self.changes(&sql, &params).await
        })
    }

    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            self.delete_many(model, r#where).await?;
            Ok(())
        })
    }

    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        Box::pin(async move {
            let (sql, params) = delete_sql(model, r#where)?;
            self.changes(&sql, &params).await
        })
    }
}

/// A statement the store would run for a count, without running it.
///
/// The cache uses this to decide whether a lookup is worth making at all:
/// a clause that cannot match anything is answered without a round trip.
pub fn is_empty_clause(r#where: &[WhereClause]) -> bool {
    r#where.iter().any(|clause| {
        clause.op == FilterOp::In
            && clause
                .value
                .as_array()
                .map(|items| items.is_empty())
                .unwrap_or(false)
    })
}

/// The session model's name, for the cache.
pub const SESSION_MODEL: &str = MODEL_SESSION;

/// A store with a cache in front of it.
pub type CachedStore = Arc<D1Store>;

#[cfg(test)]
mod tests {
    use super::*;
    use mosql::Value;

    #[test]
    fn an_answer_with_rows_is_read() {
        let answer = mosql::parse_json(
            r#"{"rows":[{"columns":["n","name"],"values":[1,"ada"]}],"changes":0}"#,
        )
        .expect("json");
        let answer = decode(&answer).expect("decode");
        assert_eq!(answer.rows.len(), 1);
        assert_eq!(answer.rows[0].0, vec!["n".to_string(), "name".to_string()]);
        assert_eq!(answer.rows[0].1[0], Value::Int(1));
        assert_eq!(answer.rows[0].1[1], Value::str("ada"));
    }

    #[test]
    fn an_answer_with_an_error_is_an_error() {
        let answer = mosql::parse_json(r#"{"error":"no such table: user"}"#).expect("json");
        let error = decode(&answer).expect_err("decode");
        assert!(error.to_string().contains("no such table"), "{error}");
    }

    #[test]
    fn the_changes_are_read() {
        let answer = mosql::parse_json(r#"{"rows":[],"changes":3}"#).expect("json");
        assert_eq!(decode(&answer).expect("decode").changes, 3);
    }
}
