//! In-memory update operators — a port of the JS `updateOps` table.

use crate::error::{Error, Result};
use crate::value::{Object, Value};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::eval::{deep_equals, del_path, get_path, gt, js_add, lt, set_path, to_number};

fn truthy_or_zero(v: &Value) -> Value {
    if v.is_truthy() {
        v.clone()
    } else {
        Value::Int(0)
    }
}

fn numeric(v: &Value) -> f64 {
    to_number(v)
}

fn num_value(x: f64) -> Value {
    if x.is_finite() && x % 1.0 == 0.0 && x.abs() < 9.0e15 {
        Value::Int(x as i64)
    } else {
        Value::Float(x)
    }
}

/// Apply one update operator to `doc`.
pub fn apply_update(op: &str, fields: &Object, doc: &mut Value, is_insert: bool) -> Result<()> {
    match op {
        "$set" => {
            for (k, v) in fields.iter() {
                set_path(doc, k, v.clone());
            }
        }
        "$unset" => {
            for (k, _) in fields.iter() {
                del_path(doc, k);
            }
        }
        "$inc" => {
            for (k, v) in fields.iter() {
                let cur = get_path(doc, k).cloned().unwrap_or(Value::Null);
                let cur = truthy_or_zero(&cur);
                set_path(doc, k, js_add(&cur, v));
            }
        }
        "$mul" => {
            for (k, v) in fields.iter() {
                let cur = get_path(doc, k).cloned().unwrap_or(Value::Null);
                let cur = truthy_or_zero(&cur);
                set_path(doc, k, num_value(numeric(&cur) * numeric(v)));
            }
        }
        "$min" => {
            for (k, v) in fields.iter() {
                let cur = get_path(doc, k);
                let replace = match cur {
                    None => true,
                    Some(cur) => lt(v, cur),
                };
                if replace {
                    set_path(doc, k, v.clone());
                }
            }
        }
        "$max" => {
            for (k, v) in fields.iter() {
                let cur = get_path(doc, k);
                let replace = match cur {
                    None => true,
                    Some(cur) => gt(v, cur),
                };
                if replace {
                    set_path(doc, k, v.clone());
                }
            }
        }
        "$push" => {
            for (k, v) in fields.iter() {
                let cur = get_path(doc, k).cloned();
                if let Some(Value::Array(mut arr)) = cur {
                    if let Some(each) = v.get("$each").and_then(|e| e.as_array()).cloned() {
                        let position = v
                            .get("$position")
                            .and_then(|p| p.as_f64())
                            .map(|n| n as i64);
                        match position {
                            Some(pos) if pos >= 0 => {
                                let pos = (pos as usize).min(arr.len());
                                for (i, item) in each.into_iter().enumerate() {
                                    arr.insert(pos + i, item);
                                }
                            }
                            Some(pos) => {
                                let pos = ((arr.len() as i64 + pos).max(0)) as usize;
                                for (i, item) in each.into_iter().enumerate() {
                                    arr.insert(pos + i, item);
                                }
                            }
                            None => arr.extend(each),
                        }

                        if let Some(sort) = v.get("$sort") {
                            sort_values(&mut arr, sort)?;
                        }
                        if let Some(slice) = v.get("$slice") {
                            let n = slice.as_f64().unwrap_or(0.0) as i64;
                            let len = arr.len() as i64;
                            let keep = if n < 0 {
                                (len + n).max(0) as usize..len as usize
                            } else {
                                0..(n.min(len)) as usize
                            };
                            arr = arr[keep].to_vec();
                        }
                    } else {
                        arr.push(v.clone());
                    }
                    set_path(doc, k, Value::Array(arr));
                } else {
                    set_path(doc, k, Value::Array(vec![v.clone()]));
                }
            }
        }
        "$pull" => {
            for (k, cond) in fields.iter() {
                if let Some(Value::Array(arr)) = get_path(doc, k).cloned() {
                    let mut kept = Vec::new();
                    for item in arr {
                        let remove = if cond.is_object() {
                            super::eval::matches(cond, &item)?
                        } else {
                            deep_equals(cond, &item)
                        };
                        if !remove {
                            kept.push(item);
                        }
                    }
                    set_path(doc, k, Value::Array(kept));
                }
            }
        }
        "$pullAll" => {
            for (k, vals) in fields.iter() {
                if let (Some(Value::Array(arr)), Some(remove)) =
                    (get_path(doc, k).cloned(), vals.as_array())
                {
                    let kept: Vec<Value> = arr
                        .into_iter()
                        .filter(|item| !remove.iter().any(|r| deep_equals(r, item)))
                        .collect();
                    set_path(doc, k, Value::Array(kept));
                }
            }
        }
        "$addToSet" => {
            for (k, v) in fields.iter() {
                if let Some(Value::Array(mut arr)) = get_path(doc, k).cloned() {
                    let items: Vec<Value> = match v.get("$each").and_then(|e| e.as_array()) {
                        Some(each) => each.clone(),
                        None => vec![v.clone()],
                    };
                    for item in items {
                        if !arr.iter().any(|e| deep_equals(e, &item)) {
                            arr.push(item);
                        }
                    }
                    set_path(doc, k, Value::Array(arr));
                } else {
                    set_path(doc, k, Value::Array(vec![v.clone()]));
                }
            }
        }
        "$pop" => {
            for (k, v) in fields.iter() {
                if let Some(Value::Array(mut arr)) = get_path(doc, k).cloned() {
                    if v.as_f64() == Some(1.0) {
                        arr.pop();
                    } else if v.as_f64() == Some(-1.0) && !arr.is_empty() {
                        arr.remove(0);
                    }
                    set_path(doc, k, Value::Array(arr));
                }
            }
        }
        "$rename" => {
            for (old, new) in fields.iter() {
                let value = get_path(doc, old).cloned();
                if let Some(value) = value {
                    let new = new.as_str().unwrap_or("").to_string();
                    set_path(doc, &new, value);
                    del_path(doc, old);
                }
            }
        }
        "$currentDate" => {
            for (k, _) in fields.iter() {
                set_path(doc, k, Value::Date(now_iso()));
            }
        }
        "$setOnInsert" => {
            if is_insert {
                for (k, v) in fields.iter() {
                    set_path(doc, k, v.clone());
                }
            }
        }
        _ => return Err(Error::new(format!("Unknown update operator: {}", op))),
    }
    Ok(())
}

fn sort_values(arr: &mut [Value], sort: &Value) -> Result<()> {
    match sort {
        Value::Int(1) | Value::Float(_) if sort.as_f64() == Some(1.0) => {
            arr.sort_by(cmp_plain);
        }
        Value::Int(-1) => {
            arr.sort_by(|a, b| cmp_plain(b, a));
        }
        Value::Object(o) => {
            if let Some((key, ord)) = o.first() {
                let key = key.clone();
                let asc = ord.as_f64() != Some(-1.0);
                arr.sort_by(|a, b| {
                    let av = get_path(a, &key);
                    let bv = get_path(b, &key);
                    let ord = match (av, bv) {
                        (Some(x), Some(y)) => {
                            if gt(x, y) {
                                core::cmp::Ordering::Greater
                            } else if lt(x, y) {
                                core::cmp::Ordering::Less
                            } else {
                                core::cmp::Ordering::Equal
                            }
                        }
                        (None, None) => core::cmp::Ordering::Equal,
                        (None, _) => core::cmp::Ordering::Greater,
                        (_, None) => core::cmp::Ordering::Less,
                    };
                    if asc {
                        ord
                    } else {
                        ord.reverse()
                    }
                });
            }
        }
        _ => {}
    }
    Ok(())
}

fn cmp_plain(a: &Value, b: &Value) -> core::cmp::Ordering {
    if gt(a, b) {
        core::cmp::Ordering::Greater
    } else if lt(a, b) {
        core::cmp::Ordering::Less
    } else {
        core::cmp::Ordering::Equal
    }
}

/// The memory engine has no clock in `no_std`; ids are monotonic instead.
pub fn now_iso() -> String {
    super::tick_iso()
}
