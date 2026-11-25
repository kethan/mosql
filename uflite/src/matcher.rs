//! A faithful port of [uqry](https://github.com/kethan/uqry)'s `filter` and
//! `expression` — the matcher `acl.js` evaluates rule `check`s with.
//!
//! It is tempting to reuse the mosql memory engine's `matches`, but the two
//! libraries differ at the edges (`$expr` argument handling, `$type`, how a
//! missing value compares), and ACL answers must be exactly uqry's. uqry is
//! ~180 lines, so it is ported whole instead of approximated.
//!
//! JavaScript `undefined` (a missing path) is not the same as `null` here, so
//! values are passed as `Option<&Value>`: `None` is `undefined`.
//!
//! ```
//! use mosql::{parse_json, Value};
//! use uflite::matcher::filter;
//!
//! let query = parse_json(r#"{"role": "admin", "post.status": "published"}"#).unwrap();
//! let doc = parse_json(r#"{"role": "admin", "post": {"status": "published"}}"#).unwrap();
//! assert!(filter(&query, Some(&doc)).unwrap());
//! // a missing path is `undefined`, which is only equal to `undefined`
//! let strict = parse_json(r#"{"age": null}"#).unwrap();
//! assert!(!filter(&strict, Some(&doc)).unwrap());
//! let _ = Value::Null;
//! ```

use mosql::memory::regex::Regex;
use mosql::Value;

use crate::error::{Error, Result};

type V<'a> = Option<&'a Value>;

// ---------------------------------------------------------------------------
// filter
// ---------------------------------------------------------------------------

/// `filter(query)(value)` — true if `value` matches the Mongo-style `query`.
pub fn filter(query: &Value, value: V<'_>) -> Result<bool> {
    if let Value::Object(o) = query {
        for (key, sub) in o.iter() {
            if let Some(op) = key.strip_prefix('$') {
                if !filter_op(op, sub, value)? {
                    return Ok(false);
                }
            } else if !filter(sub, get_path(value, key))? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(eq(Some(query), value))
}

fn filter_op(op: &str, q: &Value, v: V<'_>) -> Result<bool> {
    match op {
        "eq" => Ok(eq(Some(q), v)),
        "ne" => Ok(!eq(Some(q), v)),
        // `value OP query` with JS semantics: any NaN makes it false.
        "gt" => Ok(rel(v, Some(q), |o| o == std::cmp::Ordering::Greater)),
        "gte" => Ok(rel(v, Some(q), |o| o != std::cmp::Ordering::Less)),
        "lt" => Ok(rel(v, Some(q), |o| o == std::cmp::Ordering::Less)),
        "lte" => Ok(rel(v, Some(q), |o| o != std::cmp::Ordering::Greater)),
        "in" => {
            let items: Vec<&Value> = match q {
                Value::Array(a) => a.iter().collect(),
                other => vec![other],
            };
            Ok(items.into_iter().any(|item| eq(Some(item), v)))
        }
        "nin" => {
            let items: Vec<&Value> = match q {
                Value::Array(a) => a.iter().collect(),
                other => vec![other],
            };
            Ok(!items.into_iter().any(|item| eq(Some(item), v)))
        }
        "and" => every_clause(q, v),
        "or" => some_clause(q, v),
        "not" => Ok(!filter(q, v)?),
        "regex" => {
            let re = Regex::new(&q.to_js_string(), false)
                .map_err(|e| Error::internal(format!("Invalid regular expression: {e}")))?;
            let text = match v {
                Some(x) => x.to_js_string(),
                None => "undefined".to_string(),
            };
            Ok(re.is_match(&text))
        }
        "expr" => Ok(expression(q, v)?.is_some_and(|x| x.is_truthy())),
        "exists" => Ok(if q.is_truthy() {
            v.is_some()
        } else {
            v.is_none()
        }),
        "type" => Ok(q
            .as_str()
            .is_some_and(|t| v.map_or("undefined", js_typeof) == t)),
        "mod" => {
            let (q0, q1) = match q {
                Value::Array(a) if a.len() >= 2 => (Some(&a[0]), Some(&a[1])),
                _ => (None, None),
            };
            let x = to_number(v);
            let y = to_number(q0);
            // `value % q0 === q1` — NaN never equals anything, including NaN.
            Ok(match (x, y, q1) {
                (Some(x), Some(y), Some(Value::Int(n))) => (x % y) == *n as f64,
                (Some(x), Some(y), Some(Value::Float(n))) => (x % y) == *n,
                _ => false,
            })
        }
        "elemMatch" => {
            let items: Vec<V<'_>> = match v {
                Some(Value::Array(a)) => a.iter().map(Some).collect(),
                other => vec![other],
            };
            for item in items {
                if filter(q, item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        "all" => {
            let Some(Value::Array(items)) = v else {
                return Ok(false);
            };
            let Value::Array(wanted) = q else {
                return Err(Error::internal("query.every is not a function"));
            };
            for w in wanted {
                let mut any = false;
                for item in items {
                    if filter(w, Some(item))? {
                        any = true;
                        break;
                    }
                }
                if !any {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        "size" => Ok(match (v, q) {
            (Some(Value::Array(a)), Value::Int(n)) => a.len() as i64 == *n,
            (Some(Value::Array(a)), Value::Float(n)) => a.len() as f64 == *n,
            _ => false,
        }),
        "where" => Err(Error::internal(
            "filterOps.$where requires a JavaScript function and cannot run in Rust",
        )),
        other => Err(Error::internal(format!(
            "filterOps.${other} is not a function"
        ))),
    }
}

fn every_clause(q: &Value, v: V<'_>) -> Result<bool> {
    let Value::Array(clauses) = q else {
        return Err(Error::internal("query.every is not a function"));
    };
    for c in clauses {
        if !filter(c, v)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn some_clause(q: &Value, v: V<'_>) -> Result<bool> {
    let Value::Array(clauses) = q else {
        return Err(Error::internal("query.some is not a function"));
    };
    for c in clauses {
        if filter(c, v)? {
            return Ok(true);
        }
    }
    Ok(false)
}

// ---------------------------------------------------------------------------
// expression
// ---------------------------------------------------------------------------

/// `expression(expr)(context)` — `$field` paths, operator objects and
/// literals, as uqry evaluates them for `$expr`.
pub fn expression(expr: &Value, ctx: V<'_>) -> Result<Option<Value>> {
    match expr {
        Value::Str(s) if s.starts_with('$') => Ok(get_path(ctx, &s[1..]).cloned()),
        Value::Object(o) => {
            if let Some((op, args)) = o.first() {
                if let Some(op) = op.strip_prefix('$') {
                    if is_expression_op(op) {
                        let owned;
                        let args: &[Value] = match args {
                            Value::Array(a) => a,
                            other => {
                                owned = [other.clone()];
                                &owned
                            }
                        };
                        return expr_op(op, args, ctx);
                    }
                }
            }
            Ok(Some(expr.clone()))
        }
        other => Ok(Some(other.clone())),
    }
}

fn is_expression_op(op: &str) -> bool {
    matches!(
        op,
        "add"
            | "subtract"
            | "multiply"
            | "divide"
            | "concat"
            | "min"
            | "max"
            | "avg"
            | "sum"
            | "cond"
            | "eq"
            | "ne"
            | "gt"
            | "gte"
            | "lt"
            | "lte"
            | "in"
            | "nin"
            | "and"
            | "or"
            | "not"
            | "switch"
    )
}

fn expr_op(op: &str, args: &[Value], ctx: V<'_>) -> Result<Option<Value>> {
    let ev = |i: usize| -> Result<Option<Value>> {
        args.get(i).map_or(Ok(None), |a| expression(a, ctx))
    };
    let truthy = |i: usize| -> Result<bool> { Ok(ev(i)?.is_some_and(|v| v.is_truthy())) };
    match op {
        // `+` is JS `+`: string concatenation when either side is a string.
        "add" => {
            let mut acc = Value::Int(0);
            for a in args {
                let b = expression(a, ctx)?;
                acc = js_add(&acc, b.as_ref());
            }
            Ok(Some(acc))
        }
        // `-`, `*`, `/` are numeric; `undefined` operands give NaN. Like
        // `reduce((a, b) => a - b)` with no initial value, the first operand
        // is the seed — and an empty list throws.
        "subtract" | "multiply" | "divide" => {
            let Some((first, rest)) = args.split_first() else {
                return Err(Error::internal(
                    "Reduce of empty array with no initial value",
                ));
            };
            let mut acc = to_number(expression(first, ctx)?.as_ref()).unwrap_or(f64::NAN);
            for a in rest {
                let b = to_number(expression(a, ctx)?.as_ref()).unwrap_or(f64::NAN);
                acc = match op {
                    "subtract" => acc - b,
                    "multiply" => acc * b,
                    _ => acc / b,
                };
            }
            Ok(Some(number_value(acc)))
        }
        "concat" => {
            let mut out = String::new();
            for a in args {
                // Array.prototype.join renders undefined and null as "".
                if let Some(v) = expression(a, ctx)? {
                    if !v.is_null() {
                        out.push_str(&v.to_js_string());
                    }
                }
            }
            Ok(Some(Value::Str(out)))
        }
        "min" | "max" | "sum" | "avg" => {
            let mut nums = Vec::new();
            for a in args {
                match expression(a, ctx)? {
                    Some(Value::Array(items)) => {
                        for i in &items {
                            nums.push(to_number(Some(i)).unwrap_or(f64::NAN));
                        }
                    }
                    other => nums.push(to_number(other.as_ref()).unwrap_or(f64::NAN)),
                }
            }
            let out = match op {
                "min" => nums.iter().copied().fold(f64::INFINITY, f64::min),
                "max" => nums.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                "sum" => nums.iter().copied().fold(0.0, |a, b| a + b),
                _ => {
                    // sum / args.length — the *argument* count, not the flattened one.
                    let sum = nums.iter().copied().fold(0.0, |a, b| a + b);
                    sum / args.len() as f64
                }
            };
            Ok(Some(number_value(out)))
        }
        "cond" => Ok(if truthy(0)? { ev(1)? } else { ev(2)? }),
        "eq" => {
            let a = ev(0)?;
            let b = ev(1)?;
            Ok(Some(Value::Bool(eq(a.as_ref(), b.as_ref()))))
        }
        "ne" => {
            let a = ev(0)?;
            let b = ev(1)?;
            Ok(Some(Value::Bool(!eq(a.as_ref(), b.as_ref()))))
        }
        "gt" | "gte" | "lt" | "lte" => {
            let a = ev(0)?;
            let b = ev(1)?;
            let r = compare(a.as_ref(), b.as_ref());
            let out = match op {
                "gt" => r == Some(std::cmp::Ordering::Greater),
                "gte" => {
                    r == Some(std::cmp::Ordering::Greater) || r == Some(std::cmp::Ordering::Equal)
                }
                "lt" => r == Some(std::cmp::Ordering::Less),
                _ => r == Some(std::cmp::Ordering::Less) || r == Some(std::cmp::Ordering::Equal),
            };
            Ok(Some(Value::Bool(out)))
        }
        "in" | "nin" => {
            // uqry quirk: `expression(b)(ctx)` on an array returns the array
            // as a literal, so the second argument's elements are compared
            // *unevaluated* against the evaluated first argument.
            let a = ev(0)?;
            let hit = match args.get(1) {
                Some(Value::Array(items)) => items.iter().any(|c| eq(a.as_ref(), Some(c))),
                Some(other) => eq(a.as_ref(), Some(other)),
                None => false,
            };
            Ok(Some(Value::Bool(if op == "in" { hit } else { !hit })))
        }
        "and" => {
            for i in 0..args.len() {
                if !truthy(i)? {
                    return Ok(Some(Value::Bool(false)));
                }
            }
            Ok(Some(Value::Bool(true)))
        }
        "or" => {
            for i in 0..args.len() {
                if truthy(i)? {
                    return Ok(Some(Value::Bool(true)));
                }
            }
            Ok(Some(Value::Bool(false)))
        }
        "not" => Ok(Some(Value::Bool(!truthy(0)?))),
        "switch" => {
            let Value::Object(cfg) = &args[0] else {
                return Err(Error::internal(
                    "Cannot destructure 'branches' of undefined",
                ));
            };
            if let Some(Value::Array(branches)) = cfg.get("branches") {
                for branch in branches {
                    let conds = expression(branch.get("case").unwrap_or(&Value::Null), ctx)?;
                    if conds.is_some_and(|c| c.is_truthy()) {
                        return expression(branch.get("then").unwrap_or(&Value::Null), ctx);
                    }
                }
            }
            expression(cfg.get("default").unwrap_or(&Value::Null), ctx)
        }
        _ => Err(Error::internal(format!(
            "expressionOps.{op} is not a function"
        ))),
    }
}

// ---------------------------------------------------------------------------
// JavaScript semantics
// ---------------------------------------------------------------------------

/// `dlv(obj, "a.b.c")`: walk a dotted path. A falsy intermediate *becomes* the
/// result (uqry's `a ? a[b] : a`), which is why `{a: 0}` with path `a.b`
/// compares against `0`, not `undefined`.
pub fn get_path<'a>(value: V<'a>, path: &str) -> V<'a> {
    let mut cur = value;
    for seg in path.split('.') {
        match cur {
            Some(c) if c.is_truthy() => {
                cur = match c {
                    Value::Object(o) => o.get(seg),
                    Value::Array(a) => seg.parse::<usize>().ok().and_then(|i| a.get(i)),
                    // JS would index strings (`"ab"[1]`) and expose `.length`;
                    // ACL contexts do neither, so these read as undefined.
                    Value::Str(_) => None,
                    _ => None,
                }
            }
            _ => break,
        }
    }
    cur
}

/// JS `===` on JSON values (`1 === 1.0` is true; `NaN !== NaN`).
fn strict_eq(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Null, Null) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Int(x), Float(y)) | (Float(y), Int(x)) => (*x as f64) == *y,
        (Float(x), Float(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Date(x), Date(y)) => x == y,
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(p, q)| strict_eq(p, q))
        }
        (Object(x), Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| strict_eq(v, w)))
        }
        _ => false,
    }
}

/// uqry's `eq`: `===`, then deep equality for same-shaped arrays/objects, then
/// `NaN === NaN`.
pub fn eq(a: V<'_>, b: V<'_>) -> bool {
    match (a, b) {
        (None, None) => true,
        (None, Some(_)) | (Some(_), None) => false,
        (Some(a), Some(b)) => strict_eq(a, b) || (is_nan(a) && is_nan(b)),
    }
}

fn is_nan(v: &Value) -> bool {
    matches!(v, Value::Float(f) if f.is_nan())
}

/// `typeof value`, as uqry's `$type` compares it.
pub fn js_typeof(v: &Value) -> &'static str {
    match v {
        Value::Null => "object",
        Value::Bool(_) => "boolean",
        Value::Int(_) | Value::Float(_) => "number",
        Value::Str(_) => "string",
        Value::Array(_) | Value::Object(_) | Value::Date(_) => "object",
    }
}

/// JS `Number(value)`; `None` for `undefined`. Returns `Some(NaN)` where JS
/// would produce NaN, so callers must check `is_finite`.
fn to_number(v: V<'_>) -> Option<f64> {
    v.map(|x| x.to_number().unwrap_or(f64::NAN))
}

enum Prim {
    Undefined,
    Str(String),
    Num(f64),
}

/// ToPrimitive for the values JSON can hold: objects and arrays become their
/// string forms, as `String(x)` would.
fn to_primitive(v: V<'_>) -> Prim {
    match v {
        None => Prim::Undefined,
        Some(Value::Str(s)) => Prim::Str(s.clone()),
        Some(Value::Array(_)) | Some(Value::Object(_)) | Some(Value::Date(_)) => {
            Prim::Str(v.unwrap().to_js_string())
        }
        Some(other) => Prim::Num(other.to_number().unwrap_or(f64::NAN)),
    }
}

/// Abstract Relational Comparison: `Some(Less)` for `a < b`, `Some(Equal)`,
/// `Some(Greater)`, `None` when either side is NaN (JS yields `undefined`,
/// which makes every comparison false).
pub fn compare(a: V<'_>, b: V<'_>) -> Option<std::cmp::Ordering> {
    let (pa, pb) = (to_primitive(a), to_primitive(b));
    match (&pa, &pb) {
        (Prim::Str(x), Prim::Str(y)) => Some(x.encode_utf16().cmp(y.encode_utf16())),
        _ => prim_to_number(&pa).partial_cmp(&prim_to_number(&pb)),
    }
}

fn prim_to_number(p: &Prim) -> f64 {
    match p {
        Prim::Undefined => f64::NAN,
        // mosql's `Number(string)`: "" is 0, hex works, junk is NaN.
        Prim::Str(s) => Value::Str(s.clone()).to_number().unwrap_or(f64::NAN),
        Prim::Num(n) => *n,
    }
}

/// `compare(a, b)` restricted by `keep`; `None` (NaN) is always false.
fn rel(a: V<'_>, b: V<'_>, keep: impl Fn(std::cmp::Ordering) -> bool) -> bool {
    compare(a, b).is_some_and(keep)
}

/// `+`: string if either side is a string, numeric otherwise.
fn js_add(a: &Value, b: V<'_>) -> Value {
    let (pa, pb) = (to_primitive(Some(a)), to_primitive(b));
    match (&pa, &pb) {
        (Prim::Str(x), _) => Value::Str(format!("{x}{}", prim_to_string(&pb))),
        (_, Prim::Str(y)) => Value::Str(format!("{}{y}", prim_to_string(&pa))),
        _ => {
            let x = prim_to_number(&pa);
            let y = prim_to_number(&pb);
            number_value(x + y)
        }
    }
}

fn prim_to_string(p: &Prim) -> String {
    match p {
        Prim::Undefined => "undefined".to_string(),
        Prim::Str(s) => s.clone(),
        Prim::Num(n) => Value::Float(*n).to_js_string(),
    }
}

/// A JS number as a JSON value: integral values print without a fraction, so
/// they are `Int`, matching what `JSON.parse(JSON.stringify(x))` yields.
fn number_value(n: f64) -> Value {
    if n.is_finite() && n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        Value::Int(n as i64)
    } else {
        Value::Float(n)
    }
}
