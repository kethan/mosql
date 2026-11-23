//! Filter, expression and update operators — a direct port of the JS operator
//! tables. Every generated fragment is byte-identical to the JS implementation.

use crate::error::{Error, Result};
use crate::escape::escape;
use crate::json_path::{batch_json_update, json_path, json_update, Cast, JsonOp};
use crate::validate;
use crate::value::Value;
use crate::Db;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

fn cmp(v: &Value, db: Db, op: &str) -> Result<String> {
    Ok(format!("{} {}", op, escape(v, db)?))
}

pub fn filter_op(op: &str, v: &Value, db: Db, _f: &str) -> Result<String> {
    match op {
        "$eq" => cmp(v, db, "="),
        "$ne" => cmp(v, db, "!="),
        "$gt" => cmp(v, db, ">"),
        "$gte" => cmp(v, db, ">="),
        "$lt" => cmp(v, db, "<"),
        "$lte" => cmp(v, db, "<="),

        "$in" => {
            let arr = validate::arr(v, "$in")?;
            if arr.is_empty() {
                return Ok("= 1 AND 1 = 0".to_string());
            }
            let mut parts = Vec::with_capacity(arr.len());
            for item in arr {
                parts.push(escape(item, db)?);
            }
            Ok(format!("IN ({})", parts.join(", ")))
        }
        "$nin" => {
            let arr = validate::arr(v, "$nin")?;
            if arr.is_empty() {
                return Ok("= 1 OR 1 = 1".to_string());
            }
            let mut parts = Vec::with_capacity(arr.len());
            for item in arr {
                parts.push(escape(item, db)?);
            }
            Ok(format!("NOT IN ({})", parts.join(", ")))
        }

        "$like" => Ok(format!("LIKE {}", escape(v, db)?)),
        "$ilike" => {
            if db == Db::Pg {
                Ok(format!("ILIKE {}", escape(v, db)?))
            } else {
                Ok(format!("LIKE LOWER({})", escape(v, db)?))
            }
        }
        "$nilike" => {
            if db == Db::Pg {
                Ok(format!("NOT ILIKE {}", escape(v, db)?))
            } else {
                Ok(format!("NOT LIKE LOWER({})", escape(v, db)?))
            }
        }
        "$nlike" => Ok(format!("NOT LIKE {}", escape(v, db)?)),

        "$regex" => {
            let p = v.to_js_string();
            if p.encode_utf16().count() > 1000 {
                return Err(Error::new("Regex pattern too long (max 1000 chars)"));
            }
            if db == Db::Sqlite {
                let like = if p.starts_with('^') && p.ends_with('$') {
                    p[1..p.len() - 1].to_string()
                } else if let Some(rest) = p.strip_prefix('^') {
                    format!("{}%", rest)
                } else if let Some(rest) = p.strip_suffix('$') {
                    format!("%{}", rest)
                } else {
                    format!("%{}%", p.replace(".*", ""))
                };
                return Ok(format!("LIKE {}", escape(&Value::Str(like), db)?));
            }
            Ok(format!(
                "{} {}",
                if db == Db::Pg { "~" } else { "REGEXP" },
                escape(&Value::Str(p), db)?
            ))
        }

        "$exists" => Ok(if v.is_truthy() {
            "IS NOT NULL".to_string()
        } else {
            "IS NULL".to_string()
        }),

        "$between" => {
            let arr = v
                .as_array()
                .ok_or_else(|| Error::new("$between requires [min, max]"))?;
            if arr.len() != 2 {
                return Err(Error::new("$between requires [min, max]"));
            }
            Ok(format!(
                "BETWEEN {} AND {}",
                escape(&arr[0], db)?,
                escape(&arr[1], db)?
            ))
        }

        "$mod" => {
            let arr = v
                .as_array()
                .ok_or_else(|| Error::new("$mod requires [divisor, remainder]"))?;
            if arr.len() != 2 {
                return Err(Error::new("$mod requires [divisor, remainder]"));
            }
            Ok(format!(
                "% {} = {}",
                arr[0].to_js_string(),
                arr[1].to_js_string()
            ))
        }

        _ => Err(Error::new(format!("Unknown operator: {}", op))),
    }
}

fn join_args(
    args: &[Value],
    expr: &mut dyn FnMut(&Value) -> Result<String>,
) -> Result<Vec<String>> {
    let mut out = Vec::with_capacity(args.len());
    for a in args {
        out.push(expr(a)?);
    }
    Ok(out)
}

pub fn expr_op(
    op: &str,
    args: &[Value],
    db: Db,
    expr: &mut dyn FnMut(&Value) -> Result<String>,
) -> Result<String> {
    match op {
        // Arithmetic
        "$add" => {
            let parts = join_args(args, expr)?;
            Ok(format!("({})", parts.join(" + ")))
        }
        "$subtract" => {
            let parts = join_args(args, expr)?;
            Ok(format!("({})", parts.join(" - ")))
        }
        "$multiply" => {
            let parts = join_args(args, expr)?;
            Ok(format!("({})", parts.join(" * ")))
        }
        "$divide" => Ok(format!(
            "({} * 1.0 / NULLIF({}, 0))",
            expr(&args[0])?,
            expr(&args[1])?
        )),
        "$mod" => Ok(format!("({} % {})", expr(&args[0])?, expr(&args[1])?)),
        "$abs" => Ok(format!("ABS({})", expr(&args[0])?)),
        "$ceil" => Ok(format!("CEIL({})", expr(&args[0])?)),
        "$floor" => Ok(format!("FLOOR({})", expr(&args[0])?)),
        "$round" => {
            let n = expr(&args[0])?;
            let p = match args.get(1) {
                Some(v) => expr(v)?,
                None => "0".to_string(),
            };
            Ok(format!("ROUND({}, {})", n, p))
        }
        "$pow" => Ok(format!("POWER({}, {})", expr(&args[0])?, expr(&args[1])?)),
        "$sqrt" => Ok(format!("SQRT({})", expr(&args[0])?)),

        // String
        "$concat" => {
            let parts = join_args(args, expr)?;
            Ok(if db == Db::Pg || db == Db::Mysql {
                format!("CONCAT({})", parts.join(", "))
            } else {
                format!("({})", parts.join(" || "))
            })
        }
        "$upper" => Ok(format!("UPPER({})", expr(&args[0])?)),
        "$lower" => Ok(format!("LOWER({})", expr(&args[0])?)),
        "$substr" => {
            let s = expr(&args[0])?;
            let st = format!("({} + 1)", expr(&args[1])?);
            match args.get(2) {
                Some(l) => Ok(format!("SUBSTRING({}, {}, {})", s, st, expr(l)?)),
                None => Ok(format!("SUBSTRING({}, {})", s, st)),
            }
        }
        "$trim" => Ok(format!("TRIM({})", expr(&args[0])?)),
        "$ltrim" => Ok(format!("LTRIM({})", expr(&args[0])?)),
        "$rtrim" => Ok(format!("RTRIM({})", expr(&args[0])?)),
        "$strLen" => Ok(format!("LENGTH({})", expr(&args[0])?)),
        "$replace" => Ok(format!(
            "REPLACE({}, {}, {})",
            expr(&args[0])?,
            expr(&args[1])?,
            expr(&args[2])?
        )),

        // Aggregates
        "$sum" => {
            if args.first().and_then(|v| v.as_f64()) == Some(1.0) {
                Ok("COUNT(*)".to_string())
            } else {
                Ok(format!("SUM({})", expr(&args[0])?))
            }
        }
        "$avg" => {
            let x = format!("AVG({})", expr(&args[0])?);
            Ok(if db == Db::Mysql {
                format!("CAST({} AS DOUBLE)", x)
            } else {
                x
            })
        }
        "$min" => {
            if args.len() == 1 {
                Ok(format!("MIN({})", expr(&args[0])?))
            } else {
                let parts = join_args(args, expr)?;
                Ok(if db == Db::Sqlite {
                    format!("MIN({})", parts.join(", "))
                } else {
                    format!("LEAST({})", parts.join(", "))
                })
            }
        }
        "$max" => {
            if args.len() == 1 {
                Ok(format!("MAX({})", expr(&args[0])?))
            } else {
                let parts = join_args(args, expr)?;
                Ok(if db == Db::Sqlite {
                    format!("MAX({})", parts.join(", "))
                } else {
                    format!("GREATEST({})", parts.join(", "))
                })
            }
        }
        "$count" => Ok("COUNT(*)".to_string()),
        "$stdDevPop" => Ok(format!("STDDEV_POP({})", expr(&args[0])?)),
        "$stdDevSamp" => Ok(format!("STDDEV_SAMP({})", expr(&args[0])?)),

        // Comparison
        "$eq" => Ok(format!("({} = {})", expr(&args[0])?, expr(&args[1])?)),
        "$ne" => Ok(format!("({} <> {})", expr(&args[0])?, expr(&args[1])?)),
        "$gt" => Ok(format!("({} > {})", expr(&args[0])?, expr(&args[1])?)),
        "$gte" => Ok(format!("({} >= {})", expr(&args[0])?, expr(&args[1])?)),
        "$lt" => Ok(format!("({} < {})", expr(&args[0])?, expr(&args[1])?)),
        "$lte" => Ok(format!("({} <= {})", expr(&args[0])?, expr(&args[1])?)),
        "$cmp" => {
            let x = expr(&args[0])?;
            let y = expr(&args[1])?;
            Ok(format!(
                "CASE WHEN {x} < {y} THEN -1 WHEN {x} > {y} THEN 1 ELSE 0 END"
            ))
        }

        // Array
        "$in" => {
            if args.len() != 2 || !args[1].is_array() {
                return Err(Error::new("$in requires [value, array]"));
            }
            let items = args[1].as_array().unwrap();
            let parts = join_args(items, expr)?;
            Ok(format!("({} IN ({}))", expr(&args[0])?, parts.join(", ")))
        }
        "$nin" => {
            if args.len() != 2 || !args[1].is_array() {
                return Err(Error::new("$nin requires [value, array]"));
            }
            let items = args[1].as_array().unwrap();
            let parts = join_args(items, expr)?;
            Ok(format!(
                "({} NOT IN ({}))",
                expr(&args[0])?,
                parts.join(", ")
            ))
        }
        "$size" => {
            let f = expr(&args[0])?;
            Ok(match db {
                Db::Pg => format!("jsonb_array_length({}::jsonb)", f),
                Db::Mysql => format!("JSON_LENGTH({})", f),
                Db::Sqlite => format!("json_array_length({})", f),
            })
        }

        // Logical
        "$and" => {
            let parts = join_args(args, expr)?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        "$or" => {
            let parts = join_args(args, expr)?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        "$not" => Ok(format!("(NOT {})", expr(&args[0])?)),

        // Conditional
        "$cond" => {
            if args.len() != 3 {
                return Err(Error::new("$cond requires [condition, then, else]"));
            }
            Ok(format!(
                "CASE WHEN {} THEN {} ELSE {} END",
                expr(&args[0])?,
                expr(&args[1])?,
                expr(&args[2])?
            ))
        }
        "$ifNull" => Ok(format!(
            "COALESCE({}, {})",
            expr(&args[0])?,
            expr(&args[1])?
        )),
        "$switch" => {
            let cfg = &args[0];
            let branches = match cfg.get("branches") {
                Some(b) if b.is_array() => b.as_array().unwrap().clone(),
                _ => {
                    return Err(Error::new(
                        "$switch requires {branches: [...], default: ...}",
                    ))
                }
            };
            let mut parts = Vec::with_capacity(branches.len());
            for b in &branches {
                let case = match b.get("case") {
                    Some(c) if c.is_truthy() => c.clone(),
                    _ => return Err(Error::new("Branch needs {case, then}")),
                };
                let then = match b.get("then") {
                    Some(t) if t.is_truthy() => t.clone(),
                    _ => return Err(Error::new("Branch needs {case, then}")),
                };
                parts.push(format!("WHEN {} THEN {}", expr(&case)?, expr(&then)?));
            }
            let def = match cfg.get("default") {
                Some(d) => expr(d)?,
                None => "NULL".to_string(),
            };
            Ok(format!("CASE {} ELSE {} END", parts.join(" "), def))
        }
        "$exists" => {
            if args.len() != 2 {
                return Err(Error::new("$exists requires [field, boolean]"));
            }
            Ok(format!(
                "({} {})",
                expr(&args[0])?,
                if args[1].is_truthy() {
                    "IS NOT NULL"
                } else {
                    "IS NULL"
                }
            ))
        }

        // Date / time
        "$year" => date_extract(db, &args[0], expr, "YEAR", "%Y"),
        "$month" => date_extract(db, &args[0], expr, "MONTH", "%m"),
        "$dayOfMonth" => date_extract(db, &args[0], expr, "DAY", "%d"),
        "$dayOfWeek" => {
            let d = expr(&args[0])?;
            Ok(match db {
                Db::Pg => format!("EXTRACT(DOW FROM {}) + 1", d),
                Db::Mysql => format!("DAYOFWEEK({})", d),
                Db::Sqlite => format!("CAST(strftime('%w', {}) AS INTEGER) + 1", d),
            })
        }
        "$hour" => date_extract(db, &args[0], expr, "HOUR", "%H"),
        "$minute" => date_extract(db, &args[0], expr, "MINUTE", "%M"),
        "$second" => date_extract(db, &args[0], expr, "SECOND", "%S"),
        "$week" => date_extract(db, &args[0], expr, "WEEK", "%W"),

        // Type conversion
        "$toString" => Ok(match db {
            Db::Mysql => format!("CAST({} AS CHAR)", expr(&args[0])?),
            _ => format!("CAST({} AS TEXT)", expr(&args[0])?),
        }),
        "$toInt" => Ok(match db {
            Db::Mysql => format!("CAST({} AS SIGNED)", expr(&args[0])?),
            _ => format!("CAST({} AS INTEGER)", expr(&args[0])?),
        }),
        "$toDouble" => {
            let x = expr(&args[0])?;
            Ok(match db {
                Db::Pg => format!("CAST({} AS DOUBLE PRECISION)", x),
                Db::Mysql => format!("CAST({} AS DECIMAL(20,6))", x),
                Db::Sqlite => format!("CAST({} AS REAL)", x),
            })
        }
        "$toBool" => Ok(match db {
            Db::Mysql => format!("IF({}, 1, 0)", expr(&args[0])?),
            _ => format!("CAST({} AS BOOLEAN)", expr(&args[0])?),
        }),
        "$toDate" => Ok(match db {
            Db::Sqlite => format!("datetime({})", expr(&args[0])?),
            Db::Mysql => format!("CAST({} AS DATETIME)", expr(&args[0])?),
            Db::Pg => format!("CAST({} AS TIMESTAMP)", expr(&args[0])?),
        }),

        // Literal
        "$literal" => escape(&args[0], Db::Sqlite),

        _ => Err(Error::new(format!("Unknown expression operator: {}", op))),
    }
}

fn date_extract(
    db: Db,
    arg: &Value,
    expr: &mut dyn FnMut(&Value) -> Result<String>,
    unit: &str,
    fmt: &str,
) -> Result<String> {
    let d = expr(arg)?;
    Ok(match db {
        Db::Pg => format!("EXTRACT({} FROM {})", unit, d),
        Db::Mysql => format!("{}({})", unit, d),
        Db::Sqlite => format!("CAST(strftime('{}', {}) AS INTEGER)", fmt, d),
    })
}

pub fn update_op(op: &str, fields: &crate::value::Object, db: Db) -> Result<Vec<String>> {
    match op {
        "$set" => batch_json_update(fields, db),

        "$inc" | "$mul" => {
            let kind = if op == "$inc" {
                JsonOp::Inc
            } else {
                JsonOp::Mul
            };
            let sign = if op == "$inc" { "+" } else { "*" };
            let mut out = Vec::new();
            for (k, v) in fields.iter() {
                if k.contains('.') {
                    if let Some(sql) = json_update(k, v, db, kind)? {
                        out.push(sql);
                    }
                } else {
                    let c = validate::col(k, db)?;
                    out.push(format!("{} = {} {} {}", c, c, sign, escape(v, db)?));
                }
            }
            Ok(out)
        }

        "$min" | "$max" => {
            let func = if db == Db::Sqlite {
                if op == "$min" {
                    "MIN"
                } else {
                    "MAX"
                }
            } else if op == "$min" {
                "LEAST"
            } else {
                "GREATEST"
            };
            let mut out = Vec::new();
            for (k, v) in fields.iter() {
                let dotted = k.contains('.');
                let field = if dotted {
                    json_path(k, db, Cast::Numeric)?
                } else {
                    validate::col(k, db)?
                };
                if dotted {
                    if let Some(sql) = json_update(k, v, db, JsonOp::Set)? {
                        out.push(sql);
                    }
                } else {
                    out.push(format!(
                        "{} = {}({}, {})",
                        field,
                        func,
                        field,
                        escape(v, db)?
                    ));
                }
            }
            Ok(out)
        }

        "$unset" => {
            let mut out = Vec::new();
            for (k, _) in fields.iter() {
                if k.contains('.') {
                    if let Some(sql) = json_update(k, &Value::Null, db, JsonOp::Remove)? {
                        out.push(sql);
                    }
                } else {
                    out.push(format!("{} = NULL", validate::col(k, db)?));
                }
            }
            Ok(out)
        }

        "$currentDate" => {
            let now = match db {
                Db::Pg => "CURRENT_TIMESTAMP",
                Db::Mysql => "NOW()",
                Db::Sqlite => "datetime('now')",
            };
            let mut out = Vec::new();
            for (k, _) in fields.iter() {
                if k.contains('.') {
                    if let Some(sql) =
                        json_update(k, &Value::Str(now.to_string()), db, JsonOp::Set)?
                    {
                        out.push(sql);
                    }
                } else {
                    out.push(format!("{} = {}", validate::col(k, db)?, now));
                }
            }
            Ok(out)
        }

        "$rename" => {
            let mut out = Vec::new();
            for (old, new) in fields.iter() {
                let new = new.as_str().unwrap_or_default().to_string();
                if old.contains('.') || new.contains('.') {
                    return Err(Error::new("$rename for JSON fields not supported"));
                }
                let o = validate::col(old, db)?;
                let n = validate::col(&new, db)?;
                out.push(format!("{} = {}", n, o));
                out.push(format!("{} = NULL", o));
            }
            Ok(out)
        }

        _ => Err(Error::new(format!("Unknown update operator: {}", op))),
    }
}
