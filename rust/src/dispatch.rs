//! The embedding-neutral request dispatcher.
//!
//! Both the wasm ABI ([`crate::wasm`]) and the Node-API addon
//! ([`crate::napi`]) speak the same request shape, so the logic lives here
//! once. A request is a JSON object:
//!
//! ```json
//! { "op": "find", "table": "users", "db": "pg", "query": { "age": { "$gte": 18 } },
//!   "sort": { "age": -1 }, "limit": 10 }
//! ```
//!
//! The result is a [`Value`]: a `Str` holding SQL for builder operations, an
//! `Array` of documents for memory operations.

use alloc::string::ToString;
use alloc::vec::Vec;

use crate::{Builder, CrudOptions, Db, Value};

#[cfg(feature = "memory")]
use crate::memory::Collection as MemCollection;

/// Execute one request. Builder operations return SQL; memory operations return
/// documents.
pub fn build_request(req: &Value) -> crate::Result<Value> {
    let db = Db::parse(req.get("db").and_then(|v| v.as_str()).unwrap_or("sqlite"));
    let op = req.get("op").and_then(|v| v.as_str()).unwrap_or("");
    let table = req.get("table").and_then(|v| v.as_str()).unwrap_or("");
    let b = match req.get("variant").and_then(|v| v.as_str()) {
        Some("lite") => Builder::lite(),
        Some("tiny") => Builder::tiny(),
        _ => Builder::full(),
    };

    let empty = Value::Object(Default::default());
    let query = req.get("query").cloned().unwrap_or_else(|| empty.clone());
    let returning = match req.get("returning") {
        Some(Value::Array(items)) => Some(
            items
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
        ),
        Some(Value::Bool(true)) | Some(Value::Int(1)) => Some(Vec::new()),
        _ => None,
    };
    let options = CrudOptions {
        returning,
        allow_delete_all: req
            .get("allowDeleteAll")
            .map(|v| v.is_truthy())
            .unwrap_or(false),
    };

    let sql = match op {
        "find" => {
            let q = b
                .collection(table, db)?
                .find(&query, req.get("projection"))?;
            chain(q, req)?.to_sql()?
        }
        "findOne" => {
            let q = b
                .collection(table, db)?
                .find_one(&query, req.get("projection"))?;
            chain(q, req)?.to_sql()?
        }
        "insertOne" => b
            .collection(table, db)?
            .insert_one(req.get("doc").unwrap_or(&empty), &options)?,
        "insertMany" => b
            .collection(table, db)?
            .insert_many(req.get("docs").unwrap_or(&empty), &options)?,
        "updateOne" => b.collection(table, db)?.update_one(
            &query,
            req.get("update").unwrap_or(&empty),
            &options,
        )?,
        "updateMany" => b.collection(table, db)?.update_many(
            &query,
            req.get("update").unwrap_or(&empty),
            &options,
        )?,
        "deleteOne" => b.collection(table, db)?.delete_one(&query, &options)?,
        "deleteMany" => b.collection(table, db)?.delete_many(&query, &options)?,
        "countDocuments" => b.collection(table, db)?.count_documents(&query)?,
        "distinct" => b.collection(table, db)?.distinct(
            req.get("field").and_then(|v| v.as_str()).unwrap_or(""),
            &query,
        )?,
        "aggregate" => b
            .collection(table, db)?
            .aggregate(req.get("pipeline").unwrap_or(&empty))?,
        "filter" => b.filter(&query, db)?,
        "expression" => b.expression(req.get("expr").unwrap_or(&empty), db)?,

        #[cfg(feature = "memory")]
        "memFind" => {
            let docs = req
                .get("docs")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let coll = MemCollection::new("mem", docs, Default::default());
            let out = find_docs(&coll, req, &query)?;
            return Ok(Value::Array(out));
        }
        #[cfg(feature = "memory")]
        "memAggregate" => {
            let docs = req
                .get("docs")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let out = crate::memory::aggregate(req.get("pipeline").unwrap_or(&empty), &docs)?;
            return Ok(Value::Array(out));
        }

        _ => return Err(crate::Error::new(format!("Unknown op: {}", op))),
    };

    Ok(Value::Str(sql))
}

/// Apply the fluent `sort`/`limit`/`skip`/`distinct`/`count` chain from a request.
pub fn chain(q: crate::FindQuery, req: &Value) -> crate::Result<crate::FindQuery> {
    let mut q = q;
    if let Some(sort) = req.get("sort") {
        if sort.is_truthy() {
            q = q.sort(sort)?;
        }
    }
    if let Some(limit) = req.get("limit") {
        if !limit.is_null() {
            q = q.limit(limit)?;
        }
    }
    if let Some(skip) = req.get("skip") {
        if !skip.is_null() {
            q = q.skip(skip)?;
        }
    }
    if req.get("distinct").map(|v| v.is_truthy()).unwrap_or(false) {
        q = q.distinct()?;
    }
    if req.get("count").map(|v| v.is_truthy()).unwrap_or(false) {
        q = q.count()?;
    }
    Ok(q)
}

/// Run a `find` against a memory collection, applying the request's chain.
#[cfg(feature = "memory")]
pub fn find_docs(coll: &MemCollection, req: &Value, query: &Value) -> crate::Result<Vec<Value>> {
    let mut q = coll.find(query);
    if let Some(proj) = req.get("projection") {
        if !proj.is_null() {
            q = q.select(proj);
        }
    }
    if let Some(sort) = req.get("sort") {
        if sort.is_truthy() {
            q = q.sort(sort);
        }
    }
    if let Some(skip) = req.get("skip") {
        if !skip.is_null() {
            q = q.skip(skip.to_number().unwrap_or(0.0) as i64);
        }
    }
    if let Some(limit) = req.get("limit") {
        if !limit.is_null() {
            q = q.limit(limit.to_number().unwrap_or(0.0) as i64);
        }
    }
    if req.get("distinct").map(|v| v.is_truthy()).unwrap_or(false) {
        q = q.distinct();
    }
    if req.get("count").map(|v| v.is_truthy()).unwrap_or(false) {
        q = q.count();
    }
    q.to_vec()
}
