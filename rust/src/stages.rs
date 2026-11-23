//! Aggregation pipeline stages — a direct port of `stageHandlers`.

use crate::error::{Error, Result};
use crate::escape::escape;
use crate::json_path::{json_path, Cast};
use crate::validate;
use crate::value::Value;
use crate::Db;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Default)]
pub struct AggState {
    pub sql: String,
    pub wheres: Vec<String>,
    pub having: Vec<String>,
    pub group_by: Option<String>,
    pub agg_exprs: Vec<(String, String)>,
    pub order: String,
    pub limit: String,
    pub offset: String,
    pub counter: u32,
}

impl AggState {
    pub fn agg_exprs_set(&mut self, key: String, value: String) {
        if let Some(slot) = self.agg_exprs.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.agg_exprs.push((key, value));
        }
    }

    pub fn agg_exprs_get(&self, key: &str) -> Option<&String> {
        self.agg_exprs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }
}

fn is_dollar(v: &Value) -> bool {
    matches!(v, Value::Str(s) if s.starts_with('$'))
}

fn is_one_or_true(v: &Value) -> bool {
    matches!(v, Value::Int(1) | Value::Bool(true))
}

fn is_zero_or_false(v: &Value) -> bool {
    matches!(v, Value::Int(0) | Value::Bool(false))
}

/// `str.replace(/\bkey\b/g, value)` — used to rewrite a `$match` filter against
/// the aliases produced by an earlier `$project`/`$group`.
pub fn replace_words(haystack: &str, map: &[(String, String)]) -> String {
    let mut entries: Vec<&(String, String)> = map.iter().collect();
    // Longest key first, so `$profile.score` is rewritten before `$profile`.
    entries.sort_by_key(|(k, _)| core::cmp::Reverse(k.len()));
    let mut out = haystack.to_string();
    for (k, v) in entries {
        out = replace_word(&out, k, v);
    }
    out
}

fn replace_word(s: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if s[i..].starts_with(from) {
            let before_ok = i == 0 || !is_word_byte(bytes[i - 1]);
            let end = i + from.len();
            let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
            if before_ok && after_ok {
                out.push_str(to);
                i = end;
                continue;
            }
        }
        let ch = s[i..].chars().next().unwrap_or('\u{FFFD}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

pub fn stage(
    builder: &crate::Builder,
    op: &str,
    args: &Value,
    s: &mut AggState,
    db: Db,
) -> Result<()> {
    match op {
        "$match" => {
            let frag = builder.filter(args, db)?;
            if s.group_by.is_some() {
                let having_frag = replace_words(&frag, &s.agg_exprs);
                s.having.push(having_frag);
            } else {
                s.wheres.push(frag);
            }
        }

        "$project" => {
            builder.apply_where(s);
            let obj = args
                .as_object()
                .ok_or_else(|| Error::new("$project requires an object"))?;
            let mut cols = Vec::new();
            for (k, v) in obj.iter() {
                let alias = validate::alias(k);
                if is_one_or_true(v) {
                    let f = if k.contains('.') {
                        json_path(k, db, Cast::None)?
                    } else {
                        validate::col(k, db)?
                    };
                    cols.push(format!("{} AS {}", f, alias));
                } else if is_zero_or_false(v) {
                    // excluded
                } else if v.is_object() || is_dollar(v) {
                    let x = builder.expr(v, db)?;
                    s.agg_exprs_set(alias.clone(), x.clone());
                    cols.push(format!("{} AS {}", x, alias));
                } else {
                    cols.push(format!("{} AS {}", escape(v, db)?, alias));
                }
            }
            s.sql = format!("SELECT {} FROM {}", cols.join(", "), builder.wrap(s));
        }

        "$addFields" | "$set" => {
            builder.apply_where(s);
            let obj = args
                .as_object()
                .ok_or_else(|| Error::new("$addFields requires an object"))?;
            let existing: Vec<String> = if s.group_by.is_some() {
                s.agg_exprs
                    .iter()
                    .map(|(k, _)| format!("{} AS {}", k, k))
                    .collect()
            } else {
                vec!["*".to_string()]
            };
            let mut new_fields = Vec::new();
            for (k, v) in obj.iter() {
                let x = builder.expr(v, db)?;
                let alias = validate::alias(k);
                s.agg_exprs_set(alias.clone(), x.clone());
                new_fields.push(format!("{} AS {}", x, alias));
            }
            let all: Vec<String> = if existing.first().map(|s| s.as_str()) == Some("*") {
                let mut v = vec!["*".to_string()];
                v.extend(new_fields);
                v
            } else {
                let mut v = existing;
                v.extend(new_fields);
                v
            };
            s.sql = format!("SELECT {} FROM {}", all.join(", "), builder.wrap(s));
        }

        "$group" => {
            builder.apply_where(s);
            let obj = args
                .as_object()
                .ok_or_else(|| Error::new("$group requires an object"))?;

            let mut id: Option<String> = None;
            let mut grp: Option<String> = None;

            match obj.get("_id") {
                Some(Value::Null) => {}
                Some(v) if v.is_object() && !is_dollar(v) => {
                    let mut parts = Vec::new();
                    let mut grps = Vec::new();
                    for (k, sub) in v.as_object().unwrap().iter() {
                        let x = builder.expr(sub, db)?;
                        parts.push(format!("{} AS {}", x, validate::alias(k)));
                        grps.push(x);
                    }
                    id = Some(parts.join(", "));
                    grp = Some(grps.join(", "));
                }
                Some(v) => {
                    let x = builder.expr(v, db)?;
                    id = Some(format!("{} AS _id", x));
                    grp = Some(x);
                }
                None => return Err(Error::new("Cannot escape undefined")),
            }

            s.agg_exprs.clear();
            let mut aggs = Vec::new();
            for (k, v) in obj.iter() {
                if k == "_id" {
                    continue;
                }
                let x = builder.expr(v, db)?;
                let alias = validate::alias(k);
                s.agg_exprs_set(alias.clone(), x.clone());
                aggs.push(format!("{} AS {}", x, alias));
            }

            let mut parts = Vec::new();
            if let Some(id) = id {
                parts.push(id);
            }
            parts.extend(aggs);

            s.sql = format!("SELECT {} FROM {}", parts.join(", "), builder.wrap(s));
            s.group_by = grp;
        }

        "$sort" => {
            let obj = args
                .as_object()
                .ok_or_else(|| Error::new("$sort requires an object"))?;
            let mut clauses = Vec::new();
            for (k, ord) in obj.iter() {
                let f = if s.agg_exprs_get(k).is_some() {
                    validate::alias(k)
                } else if k.contains('.') {
                    json_path(k, db, Cast::None)?
                } else {
                    validate::col(k, db)?
                };
                let asc = matches!(ord, Value::Int(1)) || ord.as_str() == Some("asc");
                clauses.push(format!("{} {}", f, if asc { "ASC" } else { "DESC" }));
            }
            s.order = format!(" ORDER BY {}", clauses.join(", "));
        }

        "$limit" => {
            s.limit = format!(" LIMIT {}", validate::int(args, "$limit")?);
        }

        "$skip" => {
            s.offset = format!(" OFFSET {}", validate::int(args, "$skip")?);
        }

        "$count" => {
            builder.apply_where(s);
            s.sql = format!(
                "SELECT COUNT(*) AS {} FROM {}",
                validate::alias(&args.to_js_string()),
                builder.wrap(s)
            );
            s.order = String::new();
            s.limit = String::new();
            s.offset = String::new();
            s.agg_exprs.clear();
        }

        "$sample" => {
            builder.apply_where(s);
            let size = validate::int(args.get("size").unwrap_or(&Value::Null), "$sample")?;
            let rand_fn = match db {
                Db::Mysql => "RAND()",
                _ => "RANDOM()",
            };
            s.sql = format!(
                "SELECT * FROM {} ORDER BY {} LIMIT {}",
                builder.wrap(s),
                rand_fn,
                size
            );
            s.order = String::new();
            s.limit = String::new();
            s.offset = String::new();
        }

        "$sortByCount" => {
            builder.apply_where(s);
            let x = builder.expr(args, db)?;
            s.sql = format!(
                "SELECT {} AS _id, COUNT(*) AS count FROM {}",
                x,
                builder.wrap(s)
            );
            s.group_by = Some(x);
            s.order = " ORDER BY count DESC".to_string();
            s.agg_exprs = vec![("count".to_string(), "COUNT(*)".to_string())];
        }

        "$bucket" => {
            let boundaries = args
                .get("boundaries")
                .and_then(|b| b.as_array())
                .ok_or_else(|| Error::new("$bucket requires boundaries array"))?;

            builder.apply_where(s);

            let group_by = args.get("groupBy").cloned().unwrap_or(Value::Null);
            let x = builder.expr(&group_by, db)?;

            let mut case_expr = String::from("CASE ");
            if !boundaries.is_empty() {
                for i in 0..boundaries.len() - 1 {
                    case_expr.push_str(&format!(
                        "WHEN {} >= {} AND {} < {} THEN {} ",
                        x,
                        boundaries[i].to_js_string(),
                        x,
                        boundaries[i + 1].to_js_string(),
                        boundaries[i].to_js_string()
                    ));
                }
            }
            if let Some(def) = args.get("default") {
                if def.is_truthy() {
                    case_expr.push_str(&format!("ELSE {} ", escape(def, db)?));
                }
            }
            case_expr.push_str("END");

            let out_fields = match args.get("output") {
                Some(o) if o.is_object() => {
                    let mut parts = Vec::new();
                    for (k, v) in o.as_object().unwrap().iter() {
                        parts.push(format!(
                            "{} AS {}",
                            builder.expr(v, db)?,
                            validate::alias(k)
                        ));
                    }
                    parts.join(", ")
                }
                _ => String::new(),
            };

            s.sql = format!(
                "SELECT {} AS _id, COUNT(*) AS count{} FROM {}",
                case_expr,
                if out_fields.is_empty() {
                    String::new()
                } else {
                    format!(", {}", out_fields)
                },
                builder.wrap(s)
            );
            s.group_by = Some(case_expr);
        }

        _ => return Err(Error::new(format!("Unknown pipeline operator: {}", op))),
    }
    Ok(())
}
