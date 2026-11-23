//! In-memory aggregation stages — a port of the JS `stageOps` table.

use crate::error::{Error, Result};
use crate::value::{to_json, Object, Value};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::eval::{deep_equals, eval_expr, get_path, gt, js_add, lt, matches, set_path, Ctx};

fn is_one(v: &Value) -> bool {
    matches!(v, Value::Int(1) | Value::Bool(true))
}

fn is_zero(v: &Value) -> bool {
    matches!(v, Value::Int(0) | Value::Bool(false))
}

fn is_dollar(v: &Value) -> bool {
    matches!(v, Value::Str(s) if s.starts_with('$'))
}

pub fn aggregate(pipeline: &Value, docs: &[Value]) -> Result<Vec<Value>> {
    let stages = pipeline
        .as_array()
        .ok_or_else(|| Error::new("Pipeline must be array"))?;

    // A leading `$match` selects on the borrowed input *before* anything is
    // cloned, so a selective first stage never pays for the documents it drops.
    // (A pipeline that mutates documents still needs owned values; a shared
    // `Rc` document type would remove the remaining clone entirely.)
    let mut ctx: Vec<Value>;
    let mut start = 0usize;

    if let Some(first) = stages.first() {
        if let Some(obj) = first.as_object() {
            if let Some((op, args)) = obj.first() {
                if op == "$match" {
                    let mut kept = Vec::new();
                    for doc in docs {
                        if matches(args, doc)? {
                            kept.push(doc.clone());
                        }
                    }
                    ctx = kept;
                    start = 1;
                } else {
                    ctx = docs.to_vec();
                }
            } else {
                ctx = docs.to_vec();
            }
        } else {
            ctx = docs.to_vec();
        }
    } else {
        ctx = Vec::new();
    }

    for stage in stages.iter().skip(start) {
        let obj = stage
            .as_object()
            .ok_or_else(|| Error::new("Stage must be an object"))?;
        let (op, args) = obj
            .first()
            .ok_or_else(|| Error::new("Stage must have one operator"))?;

        ctx = match op.as_str() {
            "$match" => {
                // Consume `ctx`: a matching document is moved, not cloned.
                let mut out = Vec::with_capacity(ctx.len());
                for doc in ctx {
                    if matches(args, &doc)? {
                        out.push(doc);
                    }
                }
                out
            }
            "$project" => {
                let mut out = Vec::with_capacity(ctx.len());
                for doc in &ctx {
                    out.push(project(doc, args)?);
                }
                out
            }
            "$addFields" | "$set" => {
                let mut out = Vec::with_capacity(ctx.len());
                for doc in &ctx {
                    let mut res = doc.clone();
                    let fields = args.as_object().unwrap().clone();
                    if let Value::Object(o) = &mut res {
                        for (k, v) in fields.iter() {
                            o.set(k.clone(), eval_expr(v, doc, &Vec::new())?);
                        }
                    }
                    out.push(res);
                }
                out
            }
            "$unset" => {
                let keys: Vec<String> = match args {
                    Value::Array(items) => items.iter().map(|v| v.to_js_string()).collect(),
                    other => vec![other.to_js_string()],
                };
                let mut out = ctx;
                for doc in &mut out {
                    for k in &keys {
                        super::eval::del_path(doc, k);
                    }
                }
                out
            }
            "$group" => group(args, &ctx)?,
            "$bucket" => bucket(args, &ctx)?,
            "$sortByCount" => sort_by_count(args, &ctx)?,
            "$sort" => {
                let mut out = ctx;
                sort_docs(&mut out, args);
                out
            }
            "$limit" => {
                let n = args.as_f64().unwrap_or(0.0).max(0.0) as usize;
                ctx.into_iter().take(n).collect()
            }
            "$skip" => {
                let n = args.as_f64().unwrap_or(0.0).max(0.0) as usize;
                ctx.into_iter().skip(n).collect()
            }
            "$count" => vec![{
                let mut o = Object::new();
                o.set(args.to_js_string(), Value::Int(ctx.len() as i64));
                Value::Object(o)
            }],
            "$unwind" => unwind(args, &ctx)?,
            other => return Err(Error::new(format!("Unknown stage: {}", other))),
        };
    }

    Ok(ctx)
}

/// `$project` stage semantics.
///
/// Mirrors `stageOps.$project` in the JS engine exactly: any `0`/`false` value
/// switches the whole stage to exclusion (copy every other key), otherwise keys
/// are emitted in projection order with `_id` appended unless it was excluded.
pub fn project(doc: &Value, proj: &Value) -> Result<Value> {
    let obj = match proj.as_object() {
        Some(o) => o,
        None => return Ok(doc.clone()),
    };

    let mut res = Object::new();
    let excl: Vec<&String> = obj
        .iter()
        .filter(|(_, v)| is_zero(v))
        .map(|(k, _)| k)
        .collect();

    if !excl.is_empty() {
        if let Value::Object(d) = doc {
            for (k, _) in d.iter() {
                if excl.contains(&k) {
                    continue;
                }
                if let Some(v) = get_path(doc, k) {
                    res.set(k.clone(), v.clone());
                }
            }
        }
        return Ok(Value::Object(res));
    }

    for (k, v) in obj.iter() {
        if is_one(v) {
            if let Some(val) = get_path(doc, k) {
                res.set(k.clone(), val.clone());
            }
        } else if v.is_object() || is_dollar(v) {
            res.set(k.clone(), eval_expr(v, doc, &Vec::new())?);
        } else {
            res.set(k.clone(), v.clone());
        }
    }

    match obj.get("_id") {
        None => {
            if let Some(id) = doc.get("_id") {
                res.set("_id", id.clone());
            }
        }
        Some(v) if is_zero(v) => {
            res.remove("_id");
        }
        _ => {}
    }

    Ok(Value::Object(res))
}

/// Projection used by `find`, which follows MongoDB inclusion semantics.
///
/// This is a *different* code path from [`project`] in the JS engine
/// (`project` vs `stageOps.$project`) and the two disagree, so both are ported.
pub fn find_projection(doc: &Value, proj: &Value) -> Result<Value> {
    let obj = match proj.as_object() {
        Some(o) => o,
        None => return Ok(doc.clone()),
    };

    let mut res = Object::new();
    let has_include = obj.iter().any(|(_, v)| is_one(v));

    if has_include {
        for (k, v) in obj.iter() {
            if is_one(v) {
                if let Some(val) = get_path(doc, k) {
                    let mut tmp = Value::Object(core::mem::take(&mut res));
                    set_path(&mut tmp, k, val.clone());
                    if let Value::Object(o) = tmp {
                        res = o;
                    }
                }
            } else if v.is_object() || is_dollar(v) {
                res.set(k.clone(), eval_expr(v, doc, &Vec::new())?);
            }
        }
        let id_zero = obj.get("_id").map(is_zero).unwrap_or(false);
        if !id_zero {
            if let Some(id) = doc.get("_id") {
                res.set("_id", id.clone());
            }
        }
    } else {
        if let Value::Object(d) = doc {
            for (k, v) in d.iter() {
                res.set(k.clone(), v.clone());
            }
        }
        for (k, v) in obj.iter() {
            if is_zero(v) {
                let mut tmp = Value::Object(core::mem::take(&mut res));
                super::eval::del_path(&mut tmp, k);
                if let Value::Object(o) = tmp {
                    res = o;
                }
            }
        }
    }

    Ok(Value::Object(res))
}

fn sort_docs(docs: &mut [Value], sort: &Value) {
    let obj = match sort.as_object() {
        Some(o) => o.clone(),
        None => return,
    };
    docs.sort_by(|a, b| {
        for (k, ord) in obj.iter() {
            let av = get_path(a, k);
            let bv = get_path(b, k);
            let equal = match (av, bv) {
                (Some(x), Some(y)) => deep_equals(x, y),
                (None, None) => true,
                _ => false,
            };
            if equal {
                continue;
            }
            if av.is_none_or(|v| v.is_null()) {
                return core::cmp::Ordering::Greater;
            }
            if bv.is_none_or(|v| v.is_null()) {
                return core::cmp::Ordering::Less;
            }
            let (x, y) = (av.unwrap(), bv.unwrap());
            let mut ord_cmp = if gt(x, y) {
                core::cmp::Ordering::Greater
            } else {
                core::cmp::Ordering::Less
            };
            if !is_asc(ord) {
                ord_cmp = ord_cmp.reverse();
            }
            return ord_cmp;
        }
        core::cmp::Ordering::Equal
    });
}

fn is_asc(ord: &Value) -> bool {
    matches!(ord, Value::Int(1)) || ord.as_str() == Some("asc")
}

struct Group {
    id: Value,
    /// Indices into the stage input, so grouping never clones documents.
    items: Vec<usize>,
}

/// JS objects order canonical integer keys ascending before string keys.
fn reorder_groups(groups: &mut Vec<Group>) {
    let is_index = |k: &str| {
        !k.is_empty() && (k == "0" || !k.starts_with('0')) && k.bytes().all(|b| b.is_ascii_digit())
    };
    let keys: Vec<String> = groups.iter().map(|g| to_json(&g.id)).collect();
    if groups.len() > 1 && keys.iter().all(|k| is_index(k)) {
        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by_key(|i| keys[*i].parse::<i64>().unwrap_or(0));
        let mut sorted: Vec<Option<Group>> = groups.drain(..).map(Some).collect();
        for src in order {
            groups.push(sorted[src].take().unwrap());
        }
    }
}

fn group(args: &Value, docs: &[Value]) -> Result<Vec<Value>> {
    let obj = args
        .as_object()
        .ok_or_else(|| Error::new("$group requires an object"))?;
    let id_expr = obj.get("_id").cloned().unwrap_or(Value::Null);
    let id_is_null = id_expr.is_null();

    let mut groups: Vec<Group> = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let id = if id_is_null {
            Value::Null
        } else {
            eval_expr(&id_expr, doc, &Vec::new())?
        };
        match groups.iter_mut().find(|g| deep_equals(&g.id, &id)) {
            Some(g) => g.items.push(i),
            None => groups.push(Group { id, items: vec![i] }),
        }
    }

    reorder_groups(&mut groups);
    let mut out = Vec::with_capacity(groups.len());
    for g in groups {
        let mut res = Object::new();
        res.set("_id", g.id.clone());
        for (k, acc) in obj.iter() {
            if k == "_id" {
                continue;
            }
            let acc_obj = acc
                .as_object()
                .ok_or_else(|| Error::new(format!("Accumulator must be object: {}", k)))?;
            let (op, arg) = acc_obj
                .first()
                .ok_or_else(|| Error::new(format!("Accumulator must be object: {}", k)))?;
            let value = accumulate(op, arg, docs, &g.items)?;
            res.set(k, value);
        }
        out.push(Value::Object(res));
    }
    Ok(out)
}

fn accumulate(op: &str, arg: &Value, docs: &[Value], items: &[usize]) -> Result<Value> {
    match op {
        "$sum" => {
            if matches!(arg, Value::Int(1)) {
                return Ok(Value::Int(items.len() as i64));
            }
            let mut acc = Value::Int(0);
            for item in items {
                let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                let v = if v.is_truthy() { v } else { Value::Int(0) };
                acc = js_add(&acc, &v);
            }
            Ok(acc)
        }
        "$avg" => {
            let mut sum = 0.0;
            for item in items {
                let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                sum += if v.is_truthy() {
                    v.to_number().unwrap_or(0.0)
                } else {
                    0.0
                };
            }
            if items.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Float(sum / items.len() as f64))
            }
        }
        "$min" | "$max" => {
            let mut values = Vec::new();
            for item in items {
                let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                if !v.is_null() {
                    values.push(v);
                }
            }
            if values.is_empty() {
                return Ok(Value::Float(if op == "$min" {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                }));
            }
            let mut best = values[0].clone();
            for v in &values[1..] {
                let take = if op == "$min" {
                    lt(v, &best)
                } else {
                    gt(v, &best)
                };
                if take {
                    best = v.clone();
                }
            }
            Ok(best)
        }
        "$push" => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(eval_expr(arg, &docs[*item], &Vec::new())?);
            }
            Ok(Value::Array(out))
        }
        "$addToSet" => {
            let mut out: Vec<Value> = Vec::new();
            let mut seen: Vec<String> = Vec::new();
            for item in items {
                let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                let key = to_json(&v);
                if !seen.contains(&key) {
                    seen.push(key);
                    out.push(v);
                }
            }
            Ok(Value::Array(out))
        }
        "$first" => Ok(if items.is_empty() {
            Value::Null
        } else {
            eval_expr(arg, &docs[items[0]], &Vec::new())?
        }),
        "$last" => {
            let last = items.len().checked_sub(1);
            match last {
                None => Ok(Value::Null),
                Some(i) => eval_expr(arg, &docs[items[i]], &Vec::new()),
            }
        }
        _ => Err(Error::new(format!("Unknown accumulator: {}", op))),
    }
}

fn bucket(args: &Value, docs: &[Value]) -> Result<Vec<Value>> {
    let obj = args
        .as_object()
        .ok_or_else(|| Error::new("$bucket requires an object"))?;
    let boundaries = obj
        .get("boundaries")
        .and_then(|b| b.as_array())
        .ok_or_else(|| Error::new("$bucket requires boundaries"))?;
    if boundaries.len() < 2 {
        return Err(Error::new("$bucket requires boundaries"));
    }
    let group_by = obj.get("groupBy").cloned().unwrap_or(Value::Null);
    let default = obj.get("default").cloned();
    let output = obj.get("output").cloned();

    let get_bucket = |v: &Value| -> Value {
        for i in 0..boundaries.len() - 1 {
            let ge = !lt(v, &boundaries[i]);
            let lt_next = lt(v, &boundaries[i + 1]);
            if ge && lt_next {
                return boundaries[i].clone();
            }
        }
        default.clone().unwrap_or(Value::Null)
    };

    let mut groups: Vec<Group> = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let v = eval_expr(&group_by, doc, &Vec::new())?;
        let b = get_bucket(&v);
        match groups.iter_mut().find(|g| deep_equals(&g.id, &b)) {
            Some(g) => g.items.push(i),
            None => groups.push(Group {
                id: b,
                items: vec![i],
            }),
        }
    }

    reorder_groups(&mut groups);
    let mut out = Vec::with_capacity(groups.len());
    for g in groups {
        let mut res = Object::new();
        res.set("_id", g.id.clone());
        res.set("count", Value::Int(g.items.len() as i64));
        if let Some(Value::Object(output)) = output.clone() {
            for (k, acc) in output.iter() {
                let acc_obj = acc
                    .as_object()
                    .ok_or_else(|| Error::new("Output accumulator must be object"))?;
                let (op, arg) = acc_obj
                    .first()
                    .ok_or_else(|| Error::new("Output accumulator must be object"))?;
                let value = match op.as_str() {
                    "$sum" => {
                        let mut acc = Value::Int(0);
                        for item in &g.items {
                            let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                            let v = if v.is_truthy() { v } else { Value::Int(0) };
                            acc = js_add(&acc, &v);
                        }
                        acc
                    }
                    "$avg" => {
                        let mut sum = 0.0;
                        for item in &g.items {
                            let v = eval_expr(arg, &docs[*item], &Vec::new())?;
                            sum += if v.is_truthy() {
                                v.to_number().unwrap_or(0.0)
                            } else {
                                0.0
                            };
                        }
                        if g.items.is_empty() {
                            Value::Null
                        } else {
                            Value::Float(sum / g.items.len() as f64)
                        }
                    }
                    "$count" => Value::Int(g.items.len() as i64),
                    other => accumulate(other, arg, docs, &g.items)?,
                };
                res.set(k, value);
            }
        }
        out.push(Value::Object(res));
    }
    Ok(out)
}

fn sort_by_count(args: &Value, docs: &[Value]) -> Result<Vec<Value>> {
    let key_of = |doc: &Value| -> Result<Value> {
        match args {
            Value::Str(s) if s.starts_with('$') => {
                Ok(get_path(doc, &s[1..]).cloned().unwrap_or(Value::Null))
            }
            other => eval_expr(other, doc, &Vec::new()),
        }
    };

    let mut counts: Vec<(String, Value, i64)> = Vec::new();
    for doc in docs {
        let key = key_of(doc)?;
        let json = to_json(&key);
        match counts.iter_mut().find(|(k, _, _)| *k == json) {
            Some((_, _, n)) => *n += 1,
            None => counts.push((json, key, 1)),
        }
    }

    let mut out: Vec<Value> = counts
        .into_iter()
        .map(|(_, id, n)| {
            let mut o = Object::new();
            o.set("_id", id);
            o.set("count", Value::Int(n));
            Value::Object(o)
        })
        .collect();
    out.sort_by(|a, b| {
        let av = a.get("count").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bv = b.get("count").and_then(|v| v.as_f64()).unwrap_or(0.0);
        bv.partial_cmp(&av).unwrap_or(core::cmp::Ordering::Equal)
    });
    Ok(out)
}

fn unwind(args: &Value, docs: &[Value]) -> Result<Vec<Value>> {
    let (path, preserve, include_index) = match args {
        Value::Str(s) => (s.trim_start_matches('$').to_string(), false, None),
        Value::Object(o) => (
            o.get("path")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .trim_start_matches('$')
                .to_string(),
            o.get("preserveNullAndEmptyArrays")
                .map(|v| v.is_truthy())
                .unwrap_or(false),
            o.get("includeArrayIndex").cloned(),
        ),
        _ => return Err(Error::new("$unwind requires a path")),
    };

    let mut out = Vec::new();
    for doc in docs {
        match get_path(doc, &path).cloned() {
            Some(Value::Array(items)) if !items.is_empty() => {
                for (i, item) in items.into_iter().enumerate() {
                    let mut n = doc.clone();
                    set_path(&mut n, &path, item);
                    if let Some(idx) = &include_index {
                        set_path(&mut n, idx.as_str().unwrap_or(""), Value::Int(i as i64));
                    }
                    out.push(n);
                }
            }
            _ if preserve => {
                let mut n = doc.clone();
                set_path(&mut n, &path, Value::Null);
                out.push(n);
            }
            _ => {}
        }
    }
    Ok(out)
}

// used by the collection's sort implementation
pub fn sort_values(docs: &mut [Value], sort: &Value) {
    sort_docs(docs, sort);
}

#[allow(dead_code)]
fn _ctx(_c: &Ctx) {}
