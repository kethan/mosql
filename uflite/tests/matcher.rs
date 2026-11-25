//! Port of uqry's `test.js` — the `filter` and `expression` suites that back
//! `acl.js` rule checks (aggregate and the `add()` operator registry are not
//! used by uflite and are not ported) — plus the edge cases uqry's source
//! defines that its own suite leaves implicit.

mod common;

use common::{docs, expect_err, expect_json, json};
use mosql::Value;
use uflite::matcher::{eq, expression, filter};

const DATA: &str = r#"[
    {"name": "Alice", "age": 25, "city": "New York", "scores": [12, 13, 14],
     "friends": [{"name": "Bob", "age": 26}, {"name": "John", "age": 34}]},
    {"name": "Bob", "age": 30, "city": "San Francisco", "scores": [12],
     "friends": [{"name": "Jane", "age": 30}]},
    {"name": "Charlie", "age": 35, "city": "New York", "scores": [13, 14],
     "friends": [{"name": "Jack", "age": 31}, {"name": "John", "age": 34}, {"name": "Paul", "age": 42}]}
]"#;

/// `data.filter(filter(query))` → the indices that matched.
fn matched(data: &[Value], query: &str) -> Vec<usize> {
    let q = json(query);
    data.iter()
        .enumerate()
        .filter(|(_, d)| filter(&q, Some(d)).expect("filter"))
        .map(|(i, _)| i)
        .collect()
}

fn expr(expr: &str, ctx: &str) -> Value {
    expression(&json(expr), Some(&json(ctx)))
        .expect("expression")
        .unwrap_or(Value::Null)
}

// -- Filter Test -----------------------------------------------------------------

#[test]
fn uqry_filter_suite() {
    let data = docs(DATA);
    let cases: &[(&str, &str, &[usize])] = &[
        ("$eq", r#"{"age": {"$eq": 25}}"#, &[0]),
        ("$ne", r#"{"age": {"$ne": 25}}"#, &[1, 2]),
        ("$gt", r#"{"age": {"$gt": 25}}"#, &[1, 2]),
        ("$gte", r#"{"age": {"$gte": 25}}"#, &[0, 1, 2]),
        ("$lt", r#"{"age": {"$lt": 30}}"#, &[0]),
        ("$lte", r#"{"age": {"$lte": 30}}"#, &[0, 1]),
        (
            "$in",
            r#"{"city": {"$in": ["New York", "San Francisco"]}}"#,
            &[0, 1, 2],
        ),
        (
            "$nin",
            r#"{"city": {"$nin": ["New York", "San Francisco"]}}"#,
            &[],
        ),
        (
            "$and",
            r#"{"$and": [{"city": "New York"}, {"age": {"$gt": 30}}]}"#,
            &[2],
        ),
        (
            "$or",
            r#"{"$or": [{"city": "New York"}, {"age": {"$lt": 30}}]}"#,
            &[0, 2],
        ),
        ("$not", r#"{"$not": {"city": "New York"}}"#, &[1]),
        ("$regex", r#"{"city": {"$regex": "S.*"}}"#, &[1]),
        ("$exists", r#"{"city": {"$exists": true}}"#, &[0, 1, 2]),
        ("$expr", r#"{"$expr": {"$eq": ["$age", 25]}}"#, &[0]),
        ("$type", r#"{"age": {"$type": "number"}}"#, &[0, 1, 2]),
        ("$mod", r#"{"age": {"$mod": [10, 5]}}"#, &[0, 2]),
        ("$size", r#"{"scores": {"$size": 2}}"#, &[2]),
        (
            "$elemMatch",
            r#"{"friends": {"$elemMatch": {"age": {"$gt": 40}}}}"#,
            &[2],
        ),
        ("$all", r#"{"scores": {"$all": [12, 13, 14]}}"#, &[0]),
        (
            "$all of $elemMatch",
            r#"{"friends": {"$all": [{"$elemMatch": {"age": {"$gte": 34}, "name": "John"}}]}}"#,
            &[0, 2],
        ),
    ];
    for (title, query, expected) in cases {
        assert_eq!(&matched(&data, query), expected, "{title}: {query}");
    }
    // `$where` takes a JavaScript function; it cannot run here.
    assert!(filter(
        &json(r#"{"$where": "function () { return this.age > 25 }"}"#),
        Some(&data[0])
    )
    .is_err());
}

// -- Expression Test -------------------------------------------------------------

#[test]
fn uqry_expression_suite() {
    let ctx = r#"{"a": 5, "b": 10, "c": 20}"#;
    expect_json(&expr(r#"{"$add": ["$a", "$b"]}"#, ctx), "15");
    expect_json(&expr(r#"{"$subtract": ["$b", "$a"]}"#, ctx), "5");
    expect_json(&expr(r#"{"$multiply": ["$a", "$c"]}"#, ctx), "100");
    expect_json(&expr(r#"{"$divide": ["$c", "$a"]}"#, ctx), "4");
    assert_eq!(expr(r#"{"$eq": ["$a", 5]}"#, ctx), Value::Bool(true));
    assert_eq!(expr(r#"{"$ne": ["$a", 10]}"#, ctx), Value::Bool(true));
    assert_eq!(expr(r#"{"$gt": ["$c", "$b"]}"#, ctx), Value::Bool(true));
    assert_eq!(expr(r#"{"$lt": ["$a", "$b"]}"#, ctx), Value::Bool(true));
    assert_eq!(expr(r#"{"$gte": ["$b", 10]}"#, ctx), Value::Bool(true));
    assert_eq!(expr(r#"{"$lte": ["$a", 5]}"#, ctx), Value::Bool(true));
    // `$in: ['$a', 5]`: the non-array second argument is `[5]`, so 5 ∈ [5].
    assert_eq!(expr(r#"{"$in": ["$a", 5]}"#, ctx), Value::Bool(true));
    // `$nin: ['$a', [4, 6]]` — despite the title, the operator is `$nin`.
    assert_eq!(expr(r#"{"$nin": ["$a", [4, 6]]}"#, ctx), Value::Bool(true));
}

// -- Remaining expression operators ------------------------------------------------

#[test]
fn expression_cond_switch_concat_and_accumulators() {
    let ctx = r#"{"a": 5, "name": "Ada", "scores": [1, 2, 3], "nil": null}"#;
    assert_eq!(
        expr(r#"{"$cond": [{"$gt": ["$a", 3]}, "big", "small"]}"#, ctx),
        Value::str("big")
    );
    assert_eq!(
        expr(
            r#"{"$switch": {"branches": [{"case": {"$lt": ["$a", 3]}, "then": "low"}], "default": "high"}}"#,
            ctx
        ),
        Value::str("high")
    );
    // Array.prototype.join: undefined and null render as "".
    assert_eq!(
        expr(r#"{"$concat": ["$name", "$nil", "!"]}"#, ctx),
        Value::str("Ada!")
    );
    // Math.min/max/sum flatten array-valued arguments.
    assert_eq!(expr(r#"{"$min": ["$scores", 4]}"#, ctx), Value::Int(1));
    assert_eq!(expr(r#"{"$max": ["$scores", 4]}"#, ctx), Value::Int(4));
    expect_json(&expr(r#"{"$sum": ["$scores"]}"#, ctx), "6");
    // `$avg` divides by the *argument* count (1), not the flattened length — uqry's behaviour.
    expect_json(&expr(r#"{"$avg": ["$scores"]}"#, ctx), "6");
    expect_json(&expr(r#"{"$avg": [1, 2, 3]}"#, ctx), "2");
    // reduce with no initial value: the first operand is the seed.
    expect_json(&expr(r#"{"$subtract": [100, "$a"]}"#, ctx), "95");
    // `100 - [1, 2, 3]`: the array stringifies to "1,2,3", which is NaN.
    assert!(
        matches!(expr(r#"{"$subtract": [100, "$scores"]}"#, ctx), Value::Float(f) if f.is_nan())
    );
    expect_json(&expr(r#"{"$divide": [100, "$a", 2]}"#, ctx), "10");
    assert!(expression(&json(r#"{"$subtract": []}"#), Some(&json(ctx))).is_err());
    assert_eq!(expr(r#"{"$and": [true, "$a"]}"#, ctx), Value::Bool(true));
    assert_eq!(
        expr(r#"{"$or": [false, "$missing"]}"#, ctx),
        Value::Bool(false)
    );
    assert_eq!(expr(r#"{"$not": ["$missing"]}"#, ctx), Value::Bool(true));
    // Literals: unknown operators, arrays and scalars come back unchanged.
    assert_eq!(expr(r#"{"$unknown": 1}"#, ctx), json(r#"{"$unknown": 1}"#));
    assert_eq!(expr(r#"[1, "$a"]"#, ctx), json(r#"[1, "$a"]"#));
    assert_eq!(
        expr(r#"{"nested": {"$add": [1, 1]}}"#, ctx),
        json(r#"{"nested": {"$add": [1, 1]}}"#)
    );
    assert_eq!(expr(r#""plain""#, ctx), Value::str("plain"));
    // A non-array argument is wrapped: `{$not: "$a"}` is `{$not: ["$a"]}`.
    assert_eq!(expr(r#"{"$not": "$a"}"#, ctx), Value::Bool(false));
    // JS `+`: a string operand makes the whole sum a string, starting from 0.
    assert_eq!(expr(r#"{"$add": ["$name", 1]}"#, ctx), Value::str("0Ada1"));
    assert_eq!(expr(r#"{"$add": []}"#, ctx), Value::Int(0));
    // `undefined - 1` is NaN.
    assert!(
        matches!(expr(r#"{"$subtract": ["$missing", 1]}"#, ctx), Value::Float(f) if f.is_nan())
    );
}

// -- `eq` and dotted paths -----------------------------------------------------------

#[test]
fn eq_is_strict_then_deep_then_nan() {
    assert!(eq(Some(&Value::Int(1)), Some(&Value::Float(1.0))));
    assert!(!eq(Some(&Value::Int(1)), Some(&Value::str("1"))));
    assert!(eq(
        Some(&json(r#"{"a": [1, {"b": 2}]}"#)),
        Some(&json(r#"{"a": [1, {"b": 2}]}"#))
    ));
    assert!(!eq(
        Some(&json(r#"{"a": 1}"#)),
        Some(&json(r#"{"a": 1, "b": 2}"#))
    ));
    assert!(!eq(Some(&json(r#"{"a": 1}"#)), Some(&json(r#"{"b": 1}"#))));
    assert!(eq(None, None));
    assert!(!eq(None, Some(&Value::Null)));
    assert!(!eq(Some(&Value::Null), None));
    assert!(eq(
        Some(&Value::Float(f64::NAN)),
        Some(&Value::Float(f64::NAN))
    ));
}

#[test]
fn dotted_paths_follow_dlv() {
    let doc = json(
        r#"{"user": {"id": 10, "name": "Ada"}, "post": {"authorId": 10, "tags": ["a", "b"]},
            "zero": 0, "empty": "", "no": false, "nul": null, "arr": [{"x": 1}]}"#,
    );
    assert!(filter(&json(r#"{"user.id": 10, "post.authorId": 10}"#), Some(&doc)).unwrap());
    // Numeric segments index arrays.
    assert!(filter(&json(r#"{"arr.0.x": 1}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"post.tags.1": "b"}"#), Some(&doc)).unwrap());
    // A falsy intermediate is returned as-is: `zero.x` is `0`, not undefined.
    assert!(filter(&json(r#"{"zero.x": 0}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"empty.x": ""}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"no.x": false}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"nul.x": null}"#), Some(&doc)).unwrap());
    // `$exists` distinguishes a missing path from a null value.
    assert!(filter(&json(r#"{"nul": {"$exists": true}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"missing": {"$exists": false}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"user.missing": {"$exists": false}}"#), Some(&doc)).unwrap());
    assert!(!filter(&json(r#"{"nul": {"$exists": false}}"#), Some(&doc)).unwrap());
    // `$type` is `typeof`: null is "object", a missing path is "undefined".
    assert!(filter(&json(r#"{"nul": {"$type": "object"}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"missing": {"$type": "undefined"}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"post.tags": {"$type": "object"}}"#), Some(&doc)).unwrap());
}

#[test]
fn relational_operators_use_javascript_semantics() {
    let doc = json(r#"{"age": 30, "name": "Ada", "tags": ["a"], "nul": null}"#);
    // `value > query`; strings compare as strings.
    assert!(filter(&json(r#"{"age": {"$gt": 29}}"#), Some(&doc)).unwrap());
    assert!(!filter(&json(r#"{"age": {"$gt": 30}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"age": {"$gte": 30}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"age": {"$lte": 30}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"age": {"$lt": 31}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"name": {"$gt": "A"}}"#), Some(&doc)).unwrap());
    // A missing value is undefined → NaN → every relational comparison is false.
    assert!(!filter(&json(r#"{"missing": {"$gt": -1e9}}"#), Some(&doc)).unwrap());
    assert!(!filter(&json(r#"{"missing": {"$lte": 1e9}}"#), Some(&doc)).unwrap());
    // null coerces to 0.
    assert!(filter(&json(r#"{"nul": {"$lt": 1}}"#), Some(&doc)).unwrap());
    // Arrays compare by their string form ("a" → NaN... ["a"] → "a" → NaN).
    assert!(!filter(&json(r#"{"tags": {"$gt": 0}}"#), Some(&doc)).unwrap());
    // `$in` with a scalar query is `[query]`.
    assert!(filter(&json(r#"{"name": {"$in": "Ada"}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"name": {"$nin": "Bob"}}"#), Some(&doc)).unwrap());
    // NaN is in a list that contains NaN.
    assert!(!filter(&json(r#"{"missing": {"$in": [null]}}"#), Some(&doc)).unwrap());
}

#[test]
fn malformed_operators_error_like_javascript_throws() {
    let doc = json(r#"{"a": 1, "arr": [1]}"#);
    expect_err(
        filter(&json(r#"{"$nope": 1}"#), Some(&doc)),
        "filterOps.$nope is not a function",
    );
    expect_err(
        filter(&json(r#"{"$and": {"a": 1}}"#), Some(&doc)),
        "query.every is not a function",
    );
    expect_err(
        filter(&json(r#"{"$or": {"a": 1}}"#), Some(&doc)),
        "query.some is not a function",
    );
    expect_err(
        filter(&json(r#"{"arr": {"$all": 1}}"#), Some(&doc)),
        "query.every is not a function",
    );
    expect_err(
        filter(&json(r#"{"a": {"$regex": "("}}"#), Some(&doc)),
        "Invalid regular expression",
    );
    // `$mod` with a short query is `undefined === …` → false, not an error.
    assert!(!filter(&json(r#"{"a": {"$mod": [2]}}"#), Some(&doc)).unwrap());
    assert!(filter(&json(r#"{"a": {"$mod": [2, 1]}}"#), Some(&doc)).unwrap());
}

#[test]
fn filter_with_a_non_object_query_is_equality() {
    let doc = json(r#"{"a": {"b": 1}, "list": [1, 2]}"#);
    assert!(filter(&json(r#"{"b": 1}"#), Some(doc.get("a").unwrap())).unwrap());
    assert!(filter(&json(r#"[1, 2]"#), Some(doc.get("list").unwrap())).unwrap());
    assert!(!filter(&json(r#"[2, 1]"#), Some(doc.get("list").unwrap())).unwrap());
    // `eq(null, undefined)` is false — null is a value, not a missing one.
    assert!(!filter(&Value::Null, None).unwrap());
    // An empty query matches everything, including undefined.
    assert!(filter(&json("{}"), None).unwrap());
}
