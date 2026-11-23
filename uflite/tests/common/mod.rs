//! Shared helpers for the spec ports. Each `tests/*.rs` is its own crate, so
//! this is included with `mod common;` and may leave helpers unused.
#![allow(dead_code)]

use std::future::Future;

use mosql::{parse_json, Value};
use uflite::db::{block_on, FindResult, Page};
use uflite::storage::{LocalOptions, LocalStorage};
use uflite::{Error, Params, Result};

/// Parse JSON, panicking on a malformed literal — test data is hand-written.
pub fn json(s: &str) -> Value {
    parse_json(s).expect("invalid JSON in test")
}

/// A JSON array literal as a `Vec<Value>`.
pub fn docs(s: &str) -> Vec<Value> {
    match json(s) {
        Value::Array(v) => v,
        _ => panic!("expected a JSON array"),
    }
}

/// `Params` whose query is `query` (JSON), like `find({ query })`.
pub fn params(query: &str) -> Params {
    Params::new()
        .query(json(query))
        .expect("query must be an object")
}

/// Run a future on the current thread.
pub fn run<F: Future>(f: F) -> F::Output {
    block_on(f)
}

/// Unwrap the paginated `find` result.
pub fn page(r: FindResult) -> Page {
    match r {
        FindResult::Paginated(p) => p,
        FindResult::All(_) => panic!("expected a paginated result"),
    }
}

/// Unwrap the `paginate: false` result.
pub fn all(r: FindResult) -> Vec<Value> {
    match r {
        FindResult::All(v) => v,
        FindResult::Paginated(_) => panic!("expected an array result"),
    }
}

/// Read a JSON integer (`Value` has no `as_i64`; numbers are `Int`/`Float`).
pub fn int(v: &Value) -> i64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v:?}")) as i64
}

/// `expect(x).toEqual(JSON)`: structural equality, key order irrelevant
/// (like `toEqual`, unlike a JSON string comparison).
#[track_caller]
pub fn expect_json(actual: &Value, expected: &str) {
    let expected = json(expected);
    assert!(
        json_eq(actual, &expected),
        "value differs from expected JSON\n  actual:   {}\n  expected: {}",
        mosql::to_json(actual),
        mosql::to_json(&expected)
    )
}

#[track_caller]
pub fn expect_docs(actual: &[Value], expected: &str) {
    expect_json(&Value::Array(actual.to_vec()), expected)
}

/// `toEqual` on JSON values: same type and contents, objects unordered.
pub fn json_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(_), Int(_)) | (Float(_), Float(_)) | (Int(_), Float(_)) | (Float(_), Int(_)) => {
            a.as_f64() == b.as_f64()
        }
        (Str(x), Str(y)) | (Date(x), Date(y)) => x == y,
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_eq(p, q)),
        (Object(x), Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_eq(v, w)))
        }
        _ => false,
    }
}

/// `expect(x).toMatchObject(JSON)`: every key of `expected` is present and
/// equal in `actual` (recursively for objects); extra keys are fine.
pub fn matches_object(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => e
            .iter()
            .all(|(k, v)| a.get(k).is_some_and(|w| matches_object(w, v))),
        // Arrays match element-wise with the same length, as toMatchObject does.
        (Value::Array(a), Value::Array(e)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(x, y)| matches_object(x, y))
        }
        (a, e) => json_eq(a, e),
    }
}

#[track_caller]
pub fn expect_match(actual: &Value, expected: &str) {
    let expected = json(expected);
    assert!(
        matches_object(actual, &expected),
        "value does not match\n  actual:   {}\n  expected subset: {}",
        mosql::to_json(actual),
        mosql::to_json(&expected)
    )
}

/// `rejects.toThrow(msg)`: the error must carry `msg`, and is returned for
/// further assertions.
#[track_caller]
pub fn expect_err<T: std::fmt::Debug>(r: Result<T>, msg: &str) -> Error {
    let e = r.expect_err("expected an error");
    assert!(
        e.to_string().contains(msg),
        "expected error containing {msg:?}, got {e:?}"
    );
    e
}

/// `rejects.toMatchObject({ code: 404, status: 404, name: 'NotFound' })`.
#[track_caller]
pub fn expect_404<T: std::fmt::Debug>(r: Result<T>) -> Error {
    let e = expect_err(r, "No record found");
    assert_eq!(e.status(), 404);
    assert_eq!(e.name(), "NotFound");
    let body = Value::from(e.clone());
    expect_json(body.get("code").unwrap(), &e.status().to_string());
    e
}

/// A `LocalStorage` in a fresh temporary directory that is removed on drop.
pub struct TempStorage(LocalStorage);

impl TempStorage {
    pub fn new(options: LocalOptions) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "uflite-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let service = LocalStorage::new(options.base_path(dir.to_string_lossy().into_owned()))
            .expect("local storage");
        TempStorage(service)
    }

    pub fn get(&self) -> &LocalStorage {
        &self.0
    }
}

impl Drop for TempStorage {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.root());
    }
}
