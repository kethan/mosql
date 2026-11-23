//! SQL literal escaping — a direct port of the JS `escape()` helper.

use crate::error::{Error, Result};
use crate::value::{to_iso_string, to_mysql_datetime, Value};
use crate::Db;
use alloc::string::{String, ToString};

pub fn escape(value: &Value, db: Db) -> Result<String> {
    match value {
        Value::Null => Ok("NULL".to_string()),
        Value::Int(i) => Ok(i.to_string()),
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(Error::new("Cannot escape non-finite number"));
            }
            Ok(crate::value::fmt_number(*f))
        }
        Value::Bool(true) => Ok(if db == Db::Pg {
            "TRUE".to_string()
        } else {
            "1".to_string()
        }),
        Value::Bool(false) => Ok(if db == Db::Pg {
            "FALSE".to_string()
        } else {
            "0".to_string()
        }),
        Value::Date(d) => {
            if db == Db::Mysql {
                return Ok(format!("'{}'", to_mysql_datetime(d)));
            }
            let iso = to_iso_string(d);
            Ok(if db == Db::Pg {
                format!("'{}'::timestamp", iso)
            } else {
                format!("'{}'", iso)
            })
        }
        Value::Array(_) | Value::Object(_) => {
            let j = crate::value::to_json(value).replace('\'', "''");
            Ok(if db == Db::Pg {
                format!("'{}'::jsonb", j)
            } else {
                format!("'{}'", j)
            })
        }
        Value::Str(s) => Ok(format!("'{}'", s.replace('\'', "''"))),
    }
}
