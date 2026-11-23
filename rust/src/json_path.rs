//! JSON path extraction and in-place JSON updates — a direct port of
//! `jsonPath`, `jsonUpdate` and `batchJSONUpdate` from the JS source.

use crate::error::Result;
use crate::escape::escape;
use crate::validate;
use crate::value::{to_json, Object, Value};
use crate::Db;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cast {
    None,
    Numeric,
    Int,
    Boolean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonOp {
    Set,
    Inc,
    Mul,
    Remove,
}

/// `a.0.b` -> `a[0].b` (JS `PATTERN.NUM = /\ (\d+)/g`).
fn bracket_numeric(path: &str) -> String {
    path.split('.')
        .map(|seg| {
            if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
                format!("[{}]", seg)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub fn json_path(path: &str, db: Db, cast: Cast) -> Result<String> {
    if path.is_empty() {
        return Ok(String::new());
    }
    let mut it = path.split('.');
    let col = it.next().unwrap_or("");
    let rest: Vec<&str> = it.collect();
    if rest.is_empty() {
        return validate::col(col, db);
    }

    let a = rest.join(",");
    let b = format!("$.{}", bracket_numeric(&rest.join(".")));

    match db {
        Db::Pg => {
            let x = format!("({}::jsonb #>> '{{{}}}')", validate::col(col, db)?, a);
            Ok(match cast {
                Cast::Numeric => format!("({})::numeric", x),
                Cast::Int => format!("({})::int", x),
                Cast::Boolean => format!("({})::boolean", x),
                Cast::None => x,
            })
        }
        Db::Mysql => {
            let x = format!(
                "JSON_UNQUOTE(JSON_EXTRACT({}, '{}'))",
                validate::col(col, db)?,
                b
            );
            Ok(match cast {
                Cast::Numeric => format!("CAST({} AS DECIMAL(20,6))", x),
                Cast::Int => format!("CAST({} AS SIGNED)", x),
                _ => x,
            })
        }
        Db::Sqlite => Ok(format!(
            "json_extract({}, '{}')",
            validate::col(col, db)?,
            b
        )),
    }
}

fn build_val(v: &Value, db: Db) -> Result<String> {
    if v.is_null() {
        return Ok("NULL".to_string());
    }
    if v.is_object() || v.is_array() {
        let j = to_json(v).replace('\'', "''");
        return Ok(match db {
            Db::Pg => format!("'{}'::jsonb", j),
            Db::Mysql => format!("CAST('{}' AS JSON)", j),
            Db::Sqlite => format!("json('{}')", j),
        });
    }
    Ok(match db {
        Db::Pg => format!("to_jsonb({})", escape(v, db)?),
        _ => escape(v, db)?,
    })
}

pub fn json_update(path: &str, value: &Value, db: Db, op: JsonOp) -> Result<Option<String>> {
    let mut it = path.split('.');
    let col = it.next().unwrap_or("");
    let rest: Vec<&str> = it.collect();
    if rest.is_empty() {
        return Ok(None);
    }

    let c = validate::col(col, db)?;
    let a = rest.join(",");
    let b = format!("$.{}", rest.join("."));

    let extract = match db {
        Db::Pg => format!(
            "COALESCE(({}::jsonb #>> '{{{}}}')::numeric, {})",
            c,
            a,
            if op == JsonOp::Mul { 1 } else { 0 }
        ),
        Db::Mysql => format!(
            "COALESCE(JSON_EXTRACT({}, '{}'), {})",
            c,
            b,
            if op == JsonOp::Mul { 1 } else { 0 }
        ),
        Db::Sqlite => format!(
            "COALESCE(json_extract({}, '{}'), {})",
            c,
            b,
            if op == JsonOp::Mul { 1 } else { 0 }
        ),
    };

    if op == JsonOp::Remove {
        return Ok(Some(match db {
            Db::Pg => format!("{} = {}::jsonb #- '{{{}}}'", c, c, a),
            Db::Mysql => format!("{} = JSON_REMOVE({}, '{}')", c, c, b),
            Db::Sqlite => format!("{} = json_remove({}, '{}')", c, c, b),
        }));
    }

    if matches!(op, JsonOp::Inc | JsonOp::Mul) {
        let sign = if op == JsonOp::Inc { "+" } else { "*" };
        let x = format!("{} {} {}", extract, sign, escape(value, db)?);
        return Ok(Some(match db {
            Db::Pg => format!(
                "{} = jsonb_set({}::jsonb, '{{{}}}', to_jsonb({}), true)",
                c, c, a, x
            ),
            Db::Mysql => format!("{} = JSON_SET({}, '{}', {})", c, c, b, x),
            Db::Sqlite => format!("{} = json_set({}, '{}', {})", c, c, b, x),
        }));
    }

    let jv = build_val(value, db)?;
    Ok(Some(match db {
        Db::Pg => format!(
            "{} = jsonb_set(COALESCE({}::jsonb, '{{}}'::jsonb), '{{{}}}', {}, true)",
            c, c, a, jv
        ),
        Db::Mysql => format!("{} = JSON_SET(COALESCE({}, '{{}}'), '{}', {})", c, c, b, jv),
        Db::Sqlite => format!("{} = json_set(COALESCE({}, '{{}}'), '{}', {})", c, c, b, jv),
    }))
}

/// Group dotted `$set` fields per column and emit one JSON_SET per column.
pub fn batch_json_update(fields: &Object, db: Db) -> Result<Vec<String>> {
    let mut by_col: Vec<(String, Vec<(String, Value)>)> = Vec::new();
    let mut regular: Vec<(String, Value)> = Vec::new();

    for (path, value) in fields.iter() {
        if path.contains('.') {
            let col = path.split('.').next().unwrap_or("").to_string();
            match by_col.iter_mut().find(|(c, _)| *c == col) {
                Some((_, entries)) => entries.push((path.clone(), value.clone())),
                None => by_col.push((col, vec![(path.clone(), value.clone())])),
            }
        } else {
            regular.push((path.clone(), value.clone()));
        }
    }

    let mut out: Vec<String> = Vec::new();

    for (k, v) in &regular {
        out.push(format!("{} = {}", validate::col(k, db)?, escape(v, db)?));
    }

    for (col, paths) in &by_col {
        if paths.len() == 1 {
            if let Some(sql) = json_update(&paths[0].0, &paths[0].1, db, JsonOp::Set)? {
                out.push(sql);
            }
        } else if db == Db::Sqlite || db == Db::Mysql {
            let func = if db == Db::Mysql {
                "JSON_SET"
            } else {
                "json_set"
            };
            let mut pairs = Vec::new();
            for (path, value) in paths {
                let jp = format!(
                    "$.{}",
                    path.split('.').skip(1).collect::<Vec<_>>().join(".")
                );
                let val = if value.is_object_like() {
                    match db {
                        Db::Mysql => format!("CAST('{}' AS JSON)", to_json(value)),
                        Db::Sqlite => format!("json('{}')", to_json(value)),
                        Db::Pg => format!("'{}'::jsonb", to_json(value)),
                    }
                } else {
                    escape(value, db)?
                };
                pairs.push(format!("'{}', {}", jp, val));
            }
            out.push(format!(
                "{} = {}(COALESCE({}, '{{}}'), {})",
                validate::col(col, db)?,
                func,
                validate::col(col, db)?,
                pairs.join(", ")
            ));
        } else {
            let mut x = format!(
                "COALESCE({}::jsonb, '{{}}'::jsonb)",
                validate::col(col, db)?
            );
            for (path, value) in paths {
                let jp = path.split('.').skip(1).collect::<Vec<_>>().join(",");
                let val = if value.is_object_like() {
                    format!("'{}'::jsonb", to_json(value))
                } else {
                    format!("to_jsonb({})", escape(value, db)?)
                };
                x = format!("jsonb_set({}, '{{{}}}', {}, true)", x, jp, val);
            }
            out.push(format!("{} = {}", validate::col(col, db)?, x));
        }
    }

    Ok(out)
}
