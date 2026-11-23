//! In-memory filter and expression evaluation — a port of the JS `filterOps`,
//! `exprOps`, `deepEquals`, `getPath`, `setPath` and `delPath`.

use crate::error::{Error, Result};
use crate::float;
use crate::value::{parse_iso, to_iso_string, Value};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::regex::Regex;

// ---------------------------------------------------------------------------
// PATHS & EQUALITY
// ---------------------------------------------------------------------------

pub fn get_path<'a>(doc: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }
    let mut cur = doc;
    for key in path.split('.') {
        match cur {
            Value::Object(o) => cur = o.get(key)?,
            Value::Array(items) => {
                let idx: usize = key.parse().ok()?;
                cur = items.get(idx)?;
            }
            _ => return None,
        }
    }
    Some(cur)
}

pub fn set_path(doc: &mut Value, path: &str, value: Value) {
    let keys: Vec<&str> = path.split('.').collect();
    if keys.is_empty() {
        return;
    }
    let mut cur = doc;
    for i in 0..keys.len() - 1 {
        let k = keys[i];
        let numeric = keys[i + 1].bytes().all(|b| b.is_ascii_digit()) && !keys[i + 1].is_empty();
        if !cur.is_object() && !cur.is_array() {
            *cur = Value::object();
        }
        cur = match cur {
            Value::Object(o) => {
                if !o.get(k).is_some_and(|v| v.is_object() || v.is_array()) {
                    o.set(
                        k,
                        if numeric {
                            Value::Array(Vec::new())
                        } else {
                            Value::object()
                        },
                    );
                }
                o.get_mut(k).unwrap()
            }
            Value::Array(items) => {
                let idx: usize = k.parse().unwrap_or(0);
                while items.len() <= idx {
                    items.push(Value::Null);
                }
                &mut items[idx]
            }
            _ => unreachable!(),
        };
    }
    let last = keys[keys.len() - 1];
    match cur {
        Value::Object(o) => o.set(last, value),
        Value::Array(items) => {
            let idx: usize = last.parse().unwrap_or(0);
            while items.len() <= idx {
                items.push(Value::Null);
            }
            items[idx] = value;
        }
        _ => {}
    }
}

pub fn del_path(doc: &mut Value, path: &str) {
    let keys: Vec<&str> = path.split('.').collect();
    if keys.is_empty() {
        return;
    }
    let mut cur = doc;
    for k in &keys[..keys.len() - 1] {
        match cur {
            Value::Object(o) => match o.get_mut(k) {
                Some(next) => cur = next,
                None => return,
            },
            Value::Array(items) => {
                let idx: usize = k.parse().unwrap_or(usize::MAX);
                match items.get_mut(idx) {
                    Some(next) => cur = next,
                    None => return,
                }
            }
            _ => return,
        }
    }
    let last = keys[keys.len() - 1];
    match cur {
        Value::Object(o) => {
            if let Some(pos) = o.entries.iter().position(|(k, _)| k == last) {
                o.entries.remove(pos);
            }
        }
        Value::Array(items) => {
            if let Ok(idx) = last.parse::<usize>() {
                if idx < items.len() {
                    items.remove(idx);
                }
            }
        }
        _ => {}
    }
}

pub fn deep_equals(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => *x as f64 == *y,
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Date(x), Value::Date(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| deep_equals(a, b))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|other| deep_equals(v, other)))
        }
        _ => false,
    }
}

fn deep_equals_opt(a: &Value, b: Option<&Value>) -> bool {
    match b {
        Some(b) => deep_equals(a, b),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// JS VALUE COERCIONS
// ---------------------------------------------------------------------------

pub fn to_string(v: &Value) -> String {
    v.to_js_string()
}

pub fn to_number(v: &Value) -> f64 {
    v.to_number().unwrap_or(f64::NAN)
}

fn num_value(x: f64) -> Value {
    if x.is_finite() && float::fract(x) == 0.0 && x.abs() < 9.0e15 {
        Value::Int(x as i64)
    } else {
        Value::Float(x)
    }
}

pub(crate) fn js_add(a: &Value, b: &Value) -> Value {
    if a.is_object_like() || b.is_object_like() {
        // JS: object + number -> string concatenation
        return Value::Str(format!("{}{}", a.to_js_string(), b.to_js_string()));
    }
    if let (Value::Str(_), _) | (_, Value::Str(_)) = (a, b) {
        return Value::Str(format!("{}{}", a.to_js_string(), b.to_js_string()));
    }
    num_value(to_number(a) + to_number(b))
}

/// JS abstract relational comparison (`<`); `None` when either side is NaN.
fn js_lt(a: &Value, b: &Value) -> Option<core::cmp::Ordering> {
    if let (Value::Str(x), Value::Str(y)) = (a, b) {
        return Some(x.cmp(y));
    }
    let x = to_number(a);
    let y = to_number(b);
    if x.is_nan() || y.is_nan() {
        return None;
    }
    x.partial_cmp(&y)
}

pub(crate) fn lt(a: &Value, b: &Value) -> bool {
    matches!(js_lt(a, b), Some(core::cmp::Ordering::Less))
}
pub(crate) fn gt(a: &Value, b: &Value) -> bool {
    matches!(js_lt(a, b), Some(core::cmp::Ordering::Greater))
}
pub(crate) fn lte(a: &Value, b: &Value) -> bool {
    !gt(a, b)
}
pub(crate) fn gte(a: &Value, b: &Value) -> bool {
    !lt(a, b)
}

// ---------------------------------------------------------------------------
// FILTER
// ---------------------------------------------------------------------------

pub fn matches(query: &Value, doc: &Value) -> Result<bool> {
    let obj = match query {
        Value::Object(o) => o,
        _ => return Err(Error::new("Query must be object")),
    };
    for (k, v) in obj.iter() {
        if let Some(op) = k.strip_prefix('$') {
            let ok = match op {
                "and" => {
                    let arr = v
                        .as_array()
                        .ok_or_else(|| Error::new("$and requires array"))?;
                    let mut all = true;
                    for item in arr {
                        if !matches(item, doc)? {
                            all = false;
                            break;
                        }
                    }
                    all
                }
                "or" => {
                    let arr = v
                        .as_array()
                        .ok_or_else(|| Error::new("$or requires array"))?;
                    let mut any = false;
                    for item in arr {
                        if matches(item, doc)? {
                            any = true;
                            break;
                        }
                    }
                    any
                }
                "nor" => {
                    let arr = v
                        .as_array()
                        .ok_or_else(|| Error::new("$nor requires array"))?;
                    let mut any = false;
                    for item in arr {
                        if matches(item, doc)? {
                            any = true;
                            break;
                        }
                    }
                    !any
                }
                "not" => !matches(v, doc)?,
                "expr" => eval_expr(v, doc, &Vec::new())?.is_truthy(),
                // A bare operator key is legal when the "document" is a scalar
                // (e.g. `{age: {$not: {$gt: 30}}}` re-enters filter with 36).
                _ => filter_op(&format!("${}", op), v, Some(doc))?,
            };
            if !ok {
                return Ok(false);
            }
        } else {
            let value = get_path(doc, k);
            if !eval_condition(v, value)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

fn eval_condition(cond: &Value, value: Option<&Value>) -> Result<bool> {
    if let Some(obj) = cond.as_object() {
        for (op, arg) in obj.iter() {
            if op.starts_with('$') {
                if !filter_op(op, arg, value)? {
                    return Ok(false);
                }
            } else if !deep_equals_opt(cond, value) {
                // JS quirk: a non-operator key compares the whole condition object
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(deep_equals_opt(cond, value))
}

fn like_regex(pattern: &str, case_insensitive: bool) -> Result<Regex> {
    let mut p = String::from("^");
    for c in pattern.chars() {
        match c {
            '%' => p.push_str(".*"),
            '_' => p.push('.'),
            '.' | '+' | '*' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' | '\\' => {
                p.push('\\');
                p.push(c);
            }
            c => p.push(c),
        }
    }
    p.push('$');
    Regex::new(&p, case_insensitive).map_err(Error::new)
}

pub fn filter_op(op: &str, q: &Value, v: Option<&Value>) -> Result<bool> {
    let present = |v: Option<&Value>| v.cloned().unwrap_or(Value::Null);
    match op {
        "$eq" => Ok(deep_equals_opt(q, v)),
        "$ne" => Ok(!deep_equals_opt(q, v)),
        "$gt" => Ok(v.is_some_and(|v| gt(v, q))),
        "$gte" => Ok(v.is_some_and(|v| gte(v, q))),
        "$lt" => Ok(v.is_some_and(|v| lt(v, q))),
        "$lte" => Ok(v.is_some_and(|v| lte(v, q))),

        "$in" => {
            let items = q.as_array().cloned().unwrap_or_else(|| vec![q.clone()]);
            let value = present(v);
            Ok(items.iter().any(|x| deep_equals(x, &value)))
        }
        "$nin" => {
            let items = q.as_array().cloned().unwrap_or_else(|| vec![q.clone()]);
            let value = present(v);
            Ok(!items.iter().any(|x| deep_equals(x, &value)))
        }

        "$and" => {
            let arr = q
                .as_array()
                .ok_or_else(|| Error::new("$and requires array"))?;
            let value = present(v);
            let mut all = true;
            for item in arr {
                if !matches(item, &value)? {
                    all = false;
                    break;
                }
            }
            Ok(all)
        }
        "$or" => {
            let arr = q
                .as_array()
                .ok_or_else(|| Error::new("$or requires array"))?;
            let value = present(v);
            let mut any = false;
            for item in arr {
                if matches(item, &value)? {
                    any = true;
                    break;
                }
            }
            Ok(any)
        }
        "$not" => {
            let value = present(v);
            Ok(!matches(q, &value)?)
        }
        "$nor" => {
            let arr = q
                .as_array()
                .ok_or_else(|| Error::new("$nor requires array"))?;
            let value = present(v);
            let mut any = false;
            for item in arr {
                if matches(item, &value)? {
                    any = true;
                    break;
                }
            }
            Ok(!any)
        }

        "$regex" => {
            let pattern = q.to_js_string();
            let re = Regex::new(&pattern, false).map_err(Error::new)?;
            let text = v.map(to_string).unwrap_or_else(|| "undefined".to_string());
            Ok(re.is_match(&text))
        }
        "$like" => {
            let re = like_regex(&q.to_js_string(), false)?;
            Ok(re.is_match(&v.map(to_string).unwrap_or_else(|| "undefined".to_string())))
        }
        "$ilike" => {
            let re = like_regex(&q.to_js_string(), true)?;
            Ok(re.is_match(&v.map(to_string).unwrap_or_else(|| "undefined".to_string())))
        }
        "$nlike" => {
            let re = like_regex(&q.to_js_string(), false)?;
            Ok(!re.is_match(&v.map(to_string).unwrap_or_else(|| "undefined".to_string())))
        }
        "$nilike" => {
            let re = like_regex(&q.to_js_string(), true)?;
            Ok(!re.is_match(&v.map(to_string).unwrap_or_else(|| "undefined".to_string())))
        }

        "$exists" => Ok(if q.is_truthy() {
            v.is_some_and(|v| !v.is_null())
        } else {
            v.map_or(true, |v| v.is_null())
        }),

        "$type" => {
            let name = q.as_str().unwrap_or("");
            let value = present(v);
            Ok(match name {
                "string" => matches!(value, Value::Str(_)),
                "number" => matches!(value, Value::Int(_) | Value::Float(_)),
                "boolean" => matches!(value, Value::Bool(_)),
                "array" => matches!(value, Value::Array(_)),
                "object" => matches!(value, Value::Object(_)),
                "null" => matches!(value, Value::Null),
                "date" => matches!(value, Value::Date(_)),
                _ => false,
            })
        }

        "$mod" => {
            let arr = q
                .as_array()
                .ok_or_else(|| Error::new("$mod requires [divisor, remainder]"))?;
            if arr.len() != 2 {
                return Err(Error::new("$mod requires [divisor, remainder]"));
            }
            let value = present(v);
            if !matches!(value, Value::Int(_) | Value::Float(_)) {
                return Ok(false);
            }
            let divisor = to_number(&arr[0]);
            let remainder = to_number(&arr[1]);
            Ok(to_number(&value) % divisor == remainder)
        }

        "$elemMatch" => {
            let value = present(v);
            match &value {
                Value::Array(items) => {
                    for item in items {
                        if matches(q, item)? {
                            return Ok(true);
                        }
                    }
                    Ok(false)
                }
                _ => Ok(false),
            }
        }

        "$all" => {
            let value = present(v);
            let wanted = q
                .as_array()
                .ok_or_else(|| Error::new("$all requires array"))?;
            match &value {
                Value::Array(items) => Ok(wanted
                    .iter()
                    .all(|w| items.iter().any(|i| deep_equals(i, w)))),
                _ => Ok(false),
            }
        }

        "$size" => {
            let value = present(v);
            Ok(match &value {
                Value::Array(items) => Some(items.len() as f64) == q.as_f64(),
                _ => false,
            })
        }

        "$between" => {
            let arr = q
                .as_array()
                .ok_or_else(|| Error::new("$between requires [min, max]"))?;
            if arr.len() != 2 {
                return Ok(false);
            }
            let value = present(v);
            Ok(gte(&value, &arr[0]) && lte(&value, &arr[1]))
        }

        "$expr" => {
            let value = present(v);
            Ok(eval_expr(q, &value, &Vec::new())?.is_truthy())
        }

        "$where" => Err(Error::new(
            "$where is not supported in the Rust port (it takes a JS function)",
        )),

        _ => Err(Error::new(format!("Unknown filter operator: {}", op))),
    }
}

// ---------------------------------------------------------------------------
// EXPRESSION
// ---------------------------------------------------------------------------

pub type Ctx = Vec<(String, Value)>;

fn ctx_get<'a>(ctx: &'a Ctx, key: &str) -> Option<&'a Value> {
    ctx.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v)
}

fn ctx_with(ctx: &Ctx, key: &str, value: Value) -> Ctx {
    let mut next = ctx.clone();
    next.push((key.to_string(), value));
    next
}

pub fn eval_expr(e: &Value, doc: &Value, ctx: &Ctx) -> Result<Value> {
    if let Some(obj) = e.as_object() {
        let (op, args) = obj
            .first()
            .ok_or_else(|| Error::new("Unknown expression operator: (empty object)"))?;
        let list: Vec<Value> = match args {
            Value::Array(items) => items.clone(),
            other => vec![other.clone()],
        };
        return expr_op(op, &list, doc, ctx);
    }
    if let Value::Str(s) = e {
        if let Some(rest) = s.strip_prefix('$') {
            let key = rest.strip_prefix('$').unwrap_or(rest);
            if let Some(v) = ctx_get(ctx, key) {
                return Ok(v.clone());
            }
            return Ok(get_path(doc, key).cloned().unwrap_or(Value::Null));
        }
    }
    Ok(e.clone())
}

fn ev(arg: &Value, doc: &Value, ctx: &Ctx) -> Result<Value> {
    eval_expr(arg, doc, ctx)
}

fn js_slice(arr: &[Value], start: i64, end: Option<i64>) -> Value {
    let len = arr.len() as i64;
    let norm = |i: i64| -> i64 {
        if i < 0 {
            (len + i).max(0)
        } else {
            i.min(len)
        }
    };
    let s = norm(start);
    let e = match end {
        Some(e) => norm(e),
        None => len,
    };
    let (s, e) = if s > e { (e, e) } else { (s, e) };
    Value::Array(arr[s as usize..e as usize].to_vec())
}

fn parse_date(v: &Value) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    match v {
        Value::Date(d) => parse_iso(d),
        Value::Str(s) => parse_iso(s),
        Value::Int(i) => ms_to_parts(*i as f64),
        Value::Float(f) => ms_to_parts(*f),
        _ => None,
    }
}

fn ms_to_parts(ms: f64) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    if !ms.is_finite() {
        return None;
    }
    let days = float::floor(ms / 86_400_000.0) as i64;
    let rem = float::rem_euclid(ms, 86_400_000.0);
    let (y, mo, d) = civil_from_days(days);
    let h = float::floor(rem / 3_600_000.0) as u32;
    let mi = float::floor((rem % 3_600_000.0) / 60_000.0) as u32;
    let s = float::floor((rem % 60_000.0) / 1_000.0) as u32;
    let msec = float::floor(rem % 1_000.0) as u32;
    Some((y, mo, d, h, mi, s, msec))
}

pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y } as i32, m, d)
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn weekday(y: i32, mo: u32, d: u32) -> u32 {
    ((days_from_civil(y, mo, d) + 4).rem_euclid(7)) as u32
}

fn js_substr(s: &str, start: i64, len: Option<i64>) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len() as i64;
    let start = if start < 0 {
        (n + start).max(0)
    } else {
        start.min(n)
    };
    let end = match len {
        Some(l) if l > 0 => (start + l).min(n),
        Some(_) => start,
        None => n,
    };
    if start >= end {
        return String::new();
    }
    chars[start as usize..end as usize].iter().collect()
}

pub fn expr_op(op: &str, a: &[Value], doc: &Value, ctx: &Ctx) -> Result<Value> {
    match op {
        "$add" => {
            let mut acc = Value::Int(0);
            for x in a {
                let v = ev(x, doc, ctx)?;
                let v = if v.is_truthy() { v } else { Value::Int(0) };
                acc = js_add(&acc, &v);
            }
            Ok(acc)
        }
        "$subtract" => {
            if a.is_empty() {
                return Err(Error::new("Reduce of empty array with no initial value"));
            }
            let mut acc = ev(&a[0], doc, ctx)?;
            if !acc.is_truthy() {
                acc = Value::Int(0);
            }
            for x in &a[1..] {
                let v = ev(x, doc, ctx)?;
                let v = if v.is_truthy() { v } else { Value::Int(0) };
                acc = num_value(to_number(&acc) - to_number(&v));
            }
            Ok(acc)
        }
        "$multiply" => {
            let mut acc = Value::Int(1);
            for x in a {
                let v = ev(x, doc, ctx)?;
                let v = if v.is_truthy() { v } else { Value::Int(0) };
                acc = num_value(to_number(&acc) * to_number(&v));
            }
            Ok(acc)
        }
        "$divide" => {
            let x = ev(&a[0], doc, ctx)?;
            let y = ev(&a[1], doc, ctx)?;
            let x = if x.is_truthy() { x } else { Value::Int(0) };
            let y = if y.is_truthy() { y } else { Value::Int(0) };
            let d = to_number(&y);
            if d == 0.0 {
                return Ok(Value::Null);
            }
            Ok(num_value(to_number(&x) / d))
        }
        "$mod" => {
            let x = ev(&a[0], doc, ctx)?;
            let y = ev(&a[1], doc, ctx)?;
            let d = to_number(&y);
            if d == 0.0 {
                return Ok(Value::Null);
            }
            Ok(num_value(to_number(&x) % d))
        }
        "$abs" => Ok(num_value(to_number(&ev(&a[0], doc, ctx)?).abs())),
        "$ceil" => Ok(num_value(float::ceil(to_number(&ev(&a[0], doc, ctx)?)))),
        "$floor" => Ok(num_value(float::floor(to_number(&ev(&a[0], doc, ctx)?)))),
        "$round" => {
            let n = to_number(&ev(&a[0], doc, ctx)?);
            let p = match a.get(1) {
                Some(v) => to_number(&ev(v, doc, ctx)?),
                None => 0.0,
            };
            let f = float::powf(10.0, p);
            Ok(num_value(float::round(n * f) / f))
        }
        "$pow" => Ok(num_value(float::powf(
            to_number(&ev(&a[0], doc, ctx)?),
            to_number(&ev(&a[1], doc, ctx)?),
        ))),
        "$sqrt" => Ok(num_value(float::sqrt(to_number(&ev(&a[0], doc, ctx)?)))),

        "$concat" => {
            let mut out = String::new();
            for x in a {
                let v = ev(x, doc, ctx)?;
                if v.is_truthy() {
                    out.push_str(&to_string(&v));
                }
            }
            Ok(Value::Str(out))
        }
        "$upper" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(if v.is_truthy() {
                to_string(&v).to_uppercase()
            } else {
                String::new()
            }))
        }
        "$lower" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(if v.is_truthy() {
                to_string(&v).to_lowercase()
            } else {
                String::new()
            }))
        }
        "$substr" => {
            let s = ev(&a[0], doc, ctx)?;
            let s = if s.is_truthy() {
                to_string(&s)
            } else {
                String::new()
            };
            let start = to_number(&ev(&a[1], doc, ctx)?) as i64;
            let len = match a.get(2) {
                Some(v) => Some(to_number(&ev(v, doc, ctx)?) as i64),
                None => Some(s.chars().count() as i64),
            };
            Ok(Value::Str(js_substr(&s, start, len)))
        }
        "$strLen" => {
            let v = ev(&a[0], doc, ctx)?;
            let s = if v.is_truthy() {
                to_string(&v)
            } else {
                String::new()
            };
            Ok(Value::Int(s.encode_utf16().count() as i64))
        }
        "$trim" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(if v.is_truthy() {
                to_string(&v).trim().to_string()
            } else {
                String::new()
            }))
        }
        "$ltrim" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(if v.is_truthy() {
                to_string(&v).trim_start().to_string()
            } else {
                String::new()
            }))
        }
        "$rtrim" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(if v.is_truthy() {
                to_string(&v).trim_end().to_string()
            } else {
                String::new()
            }))
        }
        "$replace" => {
            let s = to_string(&ev(&a[0], doc, ctx)?);
            let srch = to_string(&ev(&a[1], doc, ctx)?);
            let rep = to_string(&ev(&a[2], doc, ctx)?);
            let re = Regex::new(&srch, false).map_err(Error::new)?;
            Ok(Value::Str(re.replace_all(&s, &rep)))
        }
        "$split" => {
            let s = ev(&a[0], doc, ctx)?;
            let s = if s.is_truthy() {
                to_string(&s)
            } else {
                String::new()
            };
            let sep = to_string(&ev(&a[1], doc, ctx)?);
            let parts: Vec<Value> = if sep.is_empty() {
                s.chars().map(|c| Value::Str(c.to_string())).collect()
            } else {
                s.split(&sep).map(|p| Value::Str(p.to_string())).collect()
            };
            Ok(Value::Array(parts))
        }

        "$min" | "$max" => {
            let mut nums: Vec<f64> = Vec::with_capacity(a.len());
            for x in a {
                nums.push(to_number(&ev(x, doc, ctx)?));
            }
            if nums.iter().any(|n| n.is_nan()) {
                return Ok(Value::Float(f64::NAN));
            }
            let v = if op == "$min" {
                nums.iter().cloned().fold(f64::INFINITY, f64::min)
            } else {
                nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            };
            Ok(num_value(v))
        }
        "$avg" => {
            let mut sum = 0.0;
            for x in a {
                let v = ev(x, doc, ctx)?;
                sum += if v.is_truthy() { to_number(&v) } else { 0.0 };
            }
            Ok(Value::Float(sum / a.len() as f64))
        }
        "$sum" => {
            let mut acc = Value::Int(0);
            for x in a {
                let v = ev(x, doc, ctx)?;
                let v = if v.is_truthy() { v } else { Value::Int(0) };
                acc = js_add(&acc, &v);
            }
            Ok(acc)
        }

        "$eq" => Ok(Value::Bool(deep_equals(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$ne" => Ok(Value::Bool(!deep_equals(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$gt" => Ok(Value::Bool(gt(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$gte" => Ok(Value::Bool(gte(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$lt" => Ok(Value::Bool(lt(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$lte" => Ok(Value::Bool(lte(
            &ev(&a[0], doc, ctx)?,
            &ev(&a[1], doc, ctx)?,
        ))),
        "$cmp" => {
            let x = ev(&a[0], doc, ctx)?;
            let y = ev(&a[1], doc, ctx)?;
            Ok(Value::Int(match js_lt(&x, &y) {
                Some(core::cmp::Ordering::Less) => -1,
                Some(core::cmp::Ordering::Greater) => 1,
                _ => 0,
            }))
        }

        "$in" => {
            let value = ev(&a[0], doc, ctx)?;
            let arr = ev(&a[1], doc, ctx)?;
            Ok(Value::Bool(match &arr {
                Value::Array(items) => items.iter().any(|i| deep_equals(i, &value)),
                _ => false,
            }))
        }
        "$nin" => {
            let value = ev(&a[0], doc, ctx)?;
            let arr = ev(&a[1], doc, ctx)?;
            Ok(Value::Bool(match &arr {
                Value::Array(items) => !items.iter().any(|i| deep_equals(i, &value)),
                _ => true,
            }))
        }
        "$size" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(match v {
                Value::Array(items) => Value::Int(items.len() as i64),
                _ => Value::Null,
            })
        }
        "$arrayElemAt" => {
            let arr = ev(&a[0], doc, ctx)?;
            let idx = to_number(&ev(&a[1], doc, ctx)?) as i64;
            Ok(match &arr {
                Value::Array(items) if idx >= 0 => {
                    items.get(idx as usize).cloned().unwrap_or(Value::Null)
                }
                Value::Array(items) => {
                    let i = items.len() as i64 + idx;
                    if i >= 0 {
                        items.get(i as usize).cloned().unwrap_or(Value::Null)
                    } else {
                        Value::Null
                    }
                }
                _ => Value::Null,
            })
        }
        "$slice" => {
            let arr = ev(&a[0], doc, ctx)?;
            let n = to_number(&ev(&a[1], doc, ctx)?) as i64;
            let items = match &arr {
                Value::Array(items) => items.clone(),
                _ => return Ok(Value::Null),
            };
            match a.get(2) {
                Some(l) => {
                    let lim = to_number(&ev(l, doc, ctx)?) as i64;
                    Ok(js_slice(&items, n, Some(n + lim)))
                }
                None if n >= 0 => Ok(js_slice(&items, 0, Some(n))),
                None => Ok(js_slice(&items, n, None)),
            }
        }
        "$map" => {
            let arr = ev(&a[0], doc, ctx)?;
            let var = a[1].as_str().unwrap_or("").to_string();
            let body = &a[2];
            match &arr {
                Value::Array(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for item in items {
                        let c = ctx_with(ctx, &var, item.clone());
                        out.push(eval_expr(body, doc, &c)?);
                    }
                    Ok(Value::Array(out))
                }
                _ => Ok(Value::Null),
            }
        }
        "$filter" => {
            let arr = ev(&a[0], doc, ctx)?;
            let var = a[1].as_str().unwrap_or("").to_string();
            let body = &a[2];
            match &arr {
                Value::Array(items) => {
                    let mut out = Vec::new();
                    for item in items {
                        let c = ctx_with(ctx, &var, item.clone());
                        if eval_expr(body, doc, &c)?.is_truthy() {
                            out.push(item.clone());
                        }
                    }
                    Ok(Value::Array(out))
                }
                _ => Ok(Value::Null),
            }
        }
        "$reduce" => {
            let arr = ev(&a[0], doc, ctx)?;
            let init = ev(&a[1], doc, ctx)?;
            let body = &a[2];
            match &arr {
                Value::Array(items) => {
                    let mut acc = init;
                    for item in items {
                        let mut c = ctx.clone();
                        c.push(("value".to_string(), acc.clone()));
                        c.push(("this".to_string(), item.clone()));
                        acc = eval_expr(body, doc, &c)?;
                    }
                    Ok(acc)
                }
                _ => Ok(init),
            }
        }

        "$and" => {
            for x in a {
                if !ev(x, doc, ctx)?.is_truthy() {
                    return Ok(Value::Bool(false));
                }
            }
            Ok(Value::Bool(true))
        }
        "$or" => {
            for x in a {
                if ev(x, doc, ctx)?.is_truthy() {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        "$not" => Ok(Value::Bool(!ev(&a[0], doc, ctx)?.is_truthy())),

        "$cond" => {
            if a.len() != 3 {
                return Err(Error::new("$cond requires [condition, then, else]"));
            }
            if ev(&a[0], doc, ctx)?.is_truthy() {
                ev(&a[1], doc, ctx)
            } else {
                ev(&a[2], doc, ctx)
            }
        }
        "$ifNull" => {
            let v = ev(&a[0], doc, ctx)?;
            if v.is_null() {
                ev(&a[1], doc, ctx)
            } else {
                Ok(v)
            }
        }
        "$switch" => {
            let cfg = &a[0];
            let branches = match cfg.get("branches") {
                Some(b) if b.is_array() => b.as_array().unwrap().clone(),
                _ => {
                    return Err(Error::new(
                        "$switch requires {branches: [...], default: ...}",
                    ))
                }
            };
            for b in &branches {
                let case = match b.get("case") {
                    Some(c) if c.is_truthy() => c.clone(),
                    _ => return Err(Error::new("Branch needs {case, then}")),
                };
                let then = match b.get("then") {
                    Some(t) if t.is_truthy() => t.clone(),
                    _ => return Err(Error::new("Branch needs {case, then}")),
                };
                if eval_expr(&case, doc, ctx)?.is_truthy() {
                    return eval_expr(&then, doc, ctx);
                }
            }
            match cfg.get("default") {
                Some(d) => eval_expr(d, doc, ctx),
                None => Ok(Value::Null),
            }
        }

        "$type" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(
                match &v {
                    Value::Null => "null",
                    Value::Bool(_) => "boolean",
                    Value::Int(_) | Value::Float(_) => "number",
                    Value::Str(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                    Value::Date(_) => "date",
                }
                .to_string(),
            ))
        }

        "$year" | "$month" | "$dayOfMonth" | "$hour" | "$minute" | "$second" | "$millisecond"
        | "$dayOfWeek" | "$week" => {
            let v = ev(&a[0], doc, ctx)?;
            let parts = parse_date(&v);
            Ok(match parts {
                None => Value::Null,
                Some((y, mo, d, h, mi, s, ms)) => match op {
                    "$year" => Value::Int(y as i64),
                    "$month" => Value::Int(mo as i64),
                    "$dayOfMonth" => Value::Int(d as i64),
                    "$hour" => Value::Int(h as i64),
                    "$minute" => Value::Int(mi as i64),
                    "$second" => Value::Int(s as i64),
                    "$millisecond" => Value::Int(ms as i64),
                    "$dayOfWeek" => Value::Int(weekday(y, mo, d) as i64 + 1),
                    "$week" => {
                        let yday = days_from_civil(y, mo, d) - days_from_civil(y, 1, 1);
                        let jan1 = weekday(y, 1, 1) as i64;
                        let before = (jan1 + 6) % 7;
                        Value::Int((yday + 7 - before) / 7)
                    }
                    _ => Value::Null,
                },
            })
        }

        "$toString" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Str(to_string(&v)))
        }
        "$toInt" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Int(parse_int(&to_string(&v))))
        }
        "$toDouble" => {
            let v = ev(&a[0], doc, ctx)?;
            Ok(Value::Float(parse_float(&to_string(&v))))
        }
        "$toBool" => Ok(Value::Bool(ev(&a[0], doc, ctx)?.is_truthy())),
        "$toDate" => {
            let v = ev(&a[0], doc, ctx)?;
            match parse_date(&v) {
                Some((y, mo, d, h, mi, s, ms)) => Ok(Value::Date(format!(
                    "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
                    y, mo, d, h, mi, s, ms
                ))),
                None => Ok(Value::Null),
            }
        }
        "$literal" => Ok(a[0].clone()),

        _ => Err(Error::new(format!("Unknown expression operator: {}", op))),
    }
}

fn parse_int(s: &str) -> i64 {
    let t = s.trim();
    let mut end = 0;
    let bytes = t.as_bytes();
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return 0;
    }
    t[..end].parse().unwrap_or(0)
}

fn parse_float(s: &str) -> f64 {
    let t = s.trim();
    let bytes = t.as_bytes();
    let mut end = 0;
    if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
        end += 1;
    }
    while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
        end += 1;
    }
    if end < bytes.len() && (bytes[end] == b'e' || bytes[end] == b'E') {
        let mut j = end + 1;
        if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b'+') {
            j += 1;
        }
        let ds = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        if j > ds {
            end = j;
        }
    }
    t[..end].parse().unwrap_or(f64::NAN)
}

// keep the ISO helpers referenced (used by `$toDate`)
#[allow(dead_code)]
fn _unused(v: &Value) -> String {
    to_iso_string(v.as_str().unwrap_or(""))
}
