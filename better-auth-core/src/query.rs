//! The wire protocol between the engine and a remote store.
//!
//! A remote store — a serverless database behind an HTTP API, or a database
//! service on a TCP socket — is reached with one request per operation. The
//! request says what to do and which model; the reply says what came back.
//! The same JSON serves both transports, so a server can speak HTTP, TCP, or
//! both, and the engine does not care which.
//!
//! ```json
//! // request
//! { "op": "findOne", "model": "user",
//!   "where": [ { "field": "email", "op": "eq", "value": "ada@example.com" } ] }
//!
//! // reply
//! { "rows": [ { "id": "…", "email": "ada@example.com" } ], "count": 1 }
//! ```
//!
//! The protocol is deliberately small: eight operations, one filter list, one
//! record. Anything richer belongs in the database, not in the wire.

use mosql::{Object, Value};

use crate::error::{AuthError, AuthResult};
use crate::models::RecordMap;
use crate::store::{FilterOp, FindManyQuery, WhereClause};

/// What a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryOp {
    Create,
    FindOne,
    FindMany,
    Count,
    Update,
    UpdateMany,
    Delete,
    DeleteMany,
}

impl QueryOp {
    /// The name on the wire.
    pub fn name(&self) -> &'static str {
        match self {
            QueryOp::Create => "create",
            QueryOp::FindOne => "findOne",
            QueryOp::FindMany => "findMany",
            QueryOp::Count => "count",
            QueryOp::Update => "update",
            QueryOp::UpdateMany => "updateMany",
            QueryOp::Delete => "delete",
            QueryOp::DeleteMany => "deleteMany",
        }
    }

    /// The operation a name stands for.
    pub fn from_name(name: &str) -> Option<QueryOp> {
        Some(match name {
            "create" => QueryOp::Create,
            "findOne" => QueryOp::FindOne,
            "findMany" => QueryOp::FindMany,
            "count" => QueryOp::Count,
            "update" => QueryOp::Update,
            "updateMany" => QueryOp::UpdateMany,
            "delete" => QueryOp::Delete,
            "deleteMany" => QueryOp::DeleteMany,
            _ => return None,
        })
    }
}

/// One request: an operation, a model, and whatever it needs.
#[derive(Debug, Clone)]
pub struct QueryRequest {
    pub op: QueryOp,
    pub model: String,
    pub r#where: Vec<WhereClause>,
    pub data: RecordMap,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    /// (field, ascending)
    pub sort_by: Option<(String, bool)>,
}

impl QueryRequest {
    pub fn new(op: QueryOp, model: impl Into<String>) -> Self {
        QueryRequest {
            op,
            model: model.into(),
            r#where: Vec::new(),
            data: RecordMap::new(),
            limit: None,
            offset: None,
            sort_by: None,
        }
    }

    pub fn where_clause(mut self, clauses: Vec<WhereClause>) -> Self {
        self.r#where = clauses;
        self
    }

    pub fn data(mut self, data: RecordMap) -> Self {
        self.data = data;
        self
    }

    pub fn query(mut self, query: FindManyQuery) -> Self {
        self.limit = query.limit;
        self.offset = query.offset;
        self.sort_by = query.sort_by.clone();
        self.r#where = query.r#where;
        self
    }

    /// The request as JSON.
    pub fn to_value(&self) -> Value {
        let mut body = Object::new();
        body.set("op", Value::str(self.op.name()));
        body.set("model", Value::str(self.model.clone()));
        if !self.r#where.is_empty() {
            let clauses: Vec<Value> = self.r#where.iter().map(clause_to_value).collect();
            body.set("where", Value::Array(clauses));
        }
        if !self.data.is_empty() {
            body.set("data", Value::Object(self.data.clone()));
        }
        if let Some(limit) = self.limit {
            body.set("limit", Value::Int(limit as i64));
        }
        if let Some(offset) = self.offset {
            body.set("offset", Value::Int(offset as i64));
        }
        if let Some((field, ascending)) = &self.sort_by {
            let mut sort = Object::new();
            sort.set("field", Value::str(field.clone()));
            sort.set("ascending", Value::Bool(*ascending));
            body.set("sort", Value::Object(sort));
        }
        Value::Object(body)
    }

    /// The request from JSON.
    pub fn from_value(value: &Value) -> AuthResult<QueryRequest> {
        let op = value
            .get("op")
            .and_then(|v| v.as_str())
            .and_then(QueryOp::from_name)
            .ok_or_else(|| AuthError::invalid_request("the request has no known operation"))?;
        let model = value
            .get("model")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AuthError::invalid_request("the request has no model"))?
            .to_string();
        let mut request = QueryRequest::new(op, model);
        if let Some(clauses) = value.get("where").and_then(|v| v.as_array()) {
            for clause in clauses {
                request.r#where.push(clause_from_value(clause)?);
            }
        }
        if let Some(Value::Object(data)) = value.get("data") {
            request.data = data.clone();
        }
        request.limit = value
            .get("limit")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64);
        request.offset = value
            .get("offset")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64);
        if let Some(sort) = value.get("sort") {
            let field = sort
                .get("field")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let ascending = sort
                .get("ascending")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(field) = field {
                request.sort_by = Some((field, ascending));
            }
        }
        Ok(request)
    }
}

/// What a reply says.
#[derive(Debug, Clone, Default)]
pub struct QueryResponse {
    pub rows: Vec<RecordMap>,
    pub count: u64,
    /// What went wrong, when something did.
    pub error: Option<String>,
}

impl QueryResponse {
    /// A reply with no rows.
    pub fn empty() -> Self {
        QueryResponse::default()
    }

    /// A reply with one row.
    pub fn one(row: RecordMap) -> Self {
        QueryResponse {
            rows: vec![row],
            count: 1,
            error: None,
        }
    }

    /// A reply with a count and no rows.
    pub fn count(count: u64) -> Self {
        QueryResponse {
            rows: Vec::new(),
            count,
            error: None,
        }
    }

    /// A reply that carries an error instead of rows.
    pub fn failed(message: impl Into<String>) -> Self {
        QueryResponse {
            rows: Vec::new(),
            count: 0,
            error: Some(message.into()),
        }
    }

    /// The reply as JSON.
    pub fn to_value(&self) -> Value {
        let mut body = Object::new();
        body.set(
            "rows",
            Value::Array(self.rows.iter().cloned().map(Value::Object).collect()),
        );
        body.set("count", Value::Int(self.count as i64));
        if let Some(error) = &self.error {
            body.set("error", Value::str(error.clone()));
        }
        Value::Object(body)
    }

    /// The reply from JSON.
    pub fn from_value(value: &Value) -> AuthResult<QueryResponse> {
        let mut response = QueryResponse::default();
        if let Some(rows) = value.get("rows").and_then(|v| v.as_array()) {
            for row in rows {
                if let Value::Object(row) = row {
                    response.rows.push(row.clone());
                }
            }
        }
        response.count = value
            .get("count")
            .and_then(|v| v.as_f64())
            .map(|n| n as u64)
            .unwrap_or(response.rows.len() as u64);
        response.error = value
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        Ok(response)
    }
}

/// One filter clause, as JSON.
fn clause_to_value(clause: &WhereClause) -> Value {
    let mut body = Object::new();
    body.set("field", Value::str(clause.field.clone()));
    body.set("op", Value::str(op_name(clause.op)));
    body.set("value", clause.value.clone());
    body.set("or", Value::Bool(clause.connector_or));
    Value::Object(body)
}

/// One filter clause, from JSON.
fn clause_from_value(value: &Value) -> AuthResult<WhereClause> {
    let field = value
        .get("field")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AuthError::invalid_request("a filter clause has no field"))?
        .to_string();
    let op = value
        .get("op")
        .and_then(|v| v.as_str())
        .and_then(op_from_name)
        .ok_or_else(|| AuthError::invalid_request("a filter clause has no known operator"))?;
    // The compared value and the connector are two different fields.
    let compared = value.get("value").cloned().unwrap_or(Value::Null);
    let connector_or = value.get("or").and_then(|v| v.as_bool()).unwrap_or(false);
    Ok(WhereClause {
        field,
        op,
        value: compared,
        connector_or,
    })
}

/// The name of a comparison, on the wire.
pub fn op_name(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => "eq",
        FilterOp::Ne => "ne",
        FilterOp::Lt => "lt",
        FilterOp::Lte => "lte",
        FilterOp::Gt => "gt",
        FilterOp::Gte => "gte",
        FilterOp::In => "in",
        FilterOp::Contains => "contains",
        FilterOp::StartsWith => "startsWith",
        FilterOp::EndsWith => "endsWith",
    }
}

/// The comparison a name stands for.
pub fn op_from_name(name: &str) -> Option<FilterOp> {
    Some(match name {
        "eq" => FilterOp::Eq,
        "ne" => FilterOp::Ne,
        "lt" => FilterOp::Lt,
        "lte" => FilterOp::Lte,
        "gt" => FilterOp::Gt,
        "gte" => FilterOp::Gte,
        "in" => FilterOp::In,
        "contains" => FilterOp::Contains,
        "startsWith" => FilterOp::StartsWith,
        "endsWith" => FilterOp::EndsWith,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_survives_the_round_trip() {
        let request = QueryRequest::new(QueryOp::FindOne, "user")
            .where_clause(vec![
                WhereClause::eq("email", Value::str("ada@example.com")),
                WhereClause::or("role", FilterOp::Ne, Value::str("admin")),
            ])
            .data({
                let mut data = RecordMap::new();
                data.set("name", Value::str("Ada"));
                data
            });
        let value = request.to_value();
        let back = QueryRequest::from_value(&value).expect("parsed");
        assert_eq!(back.op, QueryOp::FindOne);
        assert_eq!(back.model, "user");
        assert_eq!(back.r#where.len(), 2);
        assert_eq!(back.r#where[0].field, "email");
        assert_eq!(back.r#where[0].op, FilterOp::Eq);
        assert!(!back.r#where[0].connector_or);
        assert!(back.r#where[1].connector_or);
        assert_eq!(back.data.get("name").and_then(|v| v.as_str()), Some("Ada"));
    }

    #[test]
    fn a_reply_survives_the_round_trip() {
        let mut row = RecordMap::new();
        row.set("id", Value::str("u1"));
        let response = QueryResponse {
            rows: vec![row],
            count: 1,
            error: None,
        };
        let value = response.to_value();
        let back = QueryResponse::from_value(&value).expect("parsed");
        assert_eq!(back.count, 1);
        assert_eq!(back.rows.len(), 1);
        assert_eq!(back.rows[0].get("id").and_then(|v| v.as_str()), Some("u1"));
    }

    #[test]
    fn every_operation_has_a_name() {
        for op in [
            QueryOp::Create,
            QueryOp::FindOne,
            QueryOp::FindMany,
            QueryOp::Count,
            QueryOp::Update,
            QueryOp::UpdateMany,
            QueryOp::Delete,
            QueryOp::DeleteMany,
        ] {
            assert_eq!(QueryOp::from_name(op.name()), Some(op));
        }
        assert_eq!(QueryOp::from_name("nope"), None);
    }
}
