//! SQL literal escaping — a direct port of the JS `escape()` helper.

use crate::error::{Error, Result};
use crate::value::{to_iso_string, to_mysql_datetime, Value};
use crate::Db;
use alloc::string::String;

/// The same, written into a buffer the caller already owns.
///
/// The SQL builder writes into one string rather than making one per
/// fragment: a query with five conditions used to make a dozen
/// intermediate strings, and the allocations cost more than the parsing.
pub fn write_escaped(out: &mut String, value: &Value, db: Db) -> Result<()> {
    match value {
        Value::Null => out.push_str("NULL"),
        Value::Int(i) => {
            use core::fmt::Write;
            let _ = write!(out, "{}", i);
        }
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(Error::new("Cannot escape non-finite number"));
            }
            out.push_str(&crate::value::fmt_number(*f));
        }
        Value::Bool(true) => out.push_str(if db == Db::Pg { "TRUE" } else { "1" }),
        Value::Bool(false) => out.push_str(if db == Db::Pg { "FALSE" } else { "0" }),
        Value::Date(d) => {
            if db == Db::Mysql {
                out.push('\'');
                out.push_str(&to_mysql_datetime(d));
                out.push('\'');
                return Ok(());
            }
            let iso = to_iso_string(d);
            out.push('\'');
            out.push_str(&iso);
            out.push('\'');
            if db == Db::Pg {
                out.push_str("::timestamp");
            }
        }
        Value::Array(_) | Value::Object(_) => {
            let j = crate::value::to_json(value).replace('\'', "''");
            out.push('\'');
            out.push_str(&j);
            out.push('\'');
            if db == Db::Pg {
                out.push_str("::jsonb");
            }
        }
        Value::Str(s) => {
            out.push('\'');
            for ch in s.chars() {
                if ch == '\'' {
                    out.push('\'');
                }
                out.push(ch);
            }
            out.push('\'');
        }
    }
    Ok(())
}

pub fn escape(value: &Value, db: Db) -> Result<String> {
    let mut out = String::new();
    write_escaped(&mut out, value, db)?;
    Ok(out)
}
