//! Type inference, widening and literal escaping — `schemaless.js`'s helpers.

use mosql::{to_json, Db, Value};

/// `inferSQLType(value, database)` — the column type a JavaScript value maps to.
///
/// Arrays and objects become the dialect's JSON type, dates its timestamp type,
/// and a string longer than 255 characters its large-text type.
pub fn infer_sql_type(value: &Value, database: Db) -> &'static str {
    match value {
        Value::Date(_) => date_type(database),
        Value::Array(_) | Value::Object(_) => json_type(database),
        Value::Str(s) => string_type(s, database),
        Value::Int(_) => int_type(database),
        Value::Float(_) => float_type(database),
        Value::Bool(_) => bool_type(database),
        // `typeof undefined === 'undefined'` falls through to the string branch.
        Value::Null => string_type("", database),
    }
}

/// `widenType(current, next)` — the wider of two types, ties going to `next`.
pub fn widen_type(current: &str, next: &str) -> String {
    if rank(current) > rank(next) {
        current.to_string()
    } else {
        next.to_string()
    }
}

/// `baseType(type)` — the type without its `(length)`, upper-cased, so
/// `VARCHAR` from `information_schema` matches `VARCHAR(255)`.
pub fn base_type(t: &str) -> String {
    let cut = t.find('(').unwrap_or(t.len());
    t[..cut].trim().to_uppercase()
}

/// `escapeValue(value, database)` — a SQL literal, escaped by doubling quotes.
pub fn escape_value(value: &Value, database: Db) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => match (database, b) {
            (Db::Pg, true) => "TRUE".to_string(),
            (Db::Pg, false) => "FALSE".to_string(),
            (_, true) => "1".to_string(),
            (_, false) => "0".to_string(),
        },
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Date(iso) => format!("'{}'", sql_timestamp(iso)),
        Value::Array(_) | Value::Object(_) => quote(&to_json(value)),
        Value::Str(s) => quote(s),
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// `Date.toISOString()` trimmed to `YYYY-MM-DD HH:MM:SS`, as the adapter does.
fn sql_timestamp(iso: &str) -> String {
    let bytes = iso.as_bytes();
    if bytes.len() >= 19 {
        let mut s: String = iso[..19].replace('T', " ");
        s.retain(|c| c != '\u{feff}');
        s
    } else {
        iso.replace('T', " ")
    }
}

fn string_type(s: &str, database: Db) -> &'static str {
    match database {
        Db::Pg if s.chars().count() > 255 => "TEXT",
        Db::Pg => "VARCHAR(255)",
        Db::Mysql if s.chars().count() > 255 => "TEXT",
        Db::Mysql => "VARCHAR(255)",
        Db::Sqlite => "TEXT",
    }
}

fn int_type(database: Db) -> &'static str {
    match database {
        Db::Pg => "INTEGER",
        Db::Mysql => "INT",
        Db::Sqlite => "INTEGER",
    }
}

fn float_type(database: Db) -> &'static str {
    match database {
        Db::Sqlite => "REAL",
        _ => "DECIMAL(20,6)",
    }
}

fn bool_type(database: Db) -> &'static str {
    match database {
        Db::Pg => "BOOLEAN",
        Db::Mysql => "TINYINT(1)",
        Db::Sqlite => "INTEGER",
    }
}

/// The timestamp column type, also used for `Date` values.
pub fn date_type(database: Db) -> &'static str {
    match database {
        Db::Pg => "TIMESTAMP",
        Db::Mysql => "DATETIME",
        Db::Sqlite => "TEXT",
    }
}

/// The JSON column type, also used for arrays and objects.
pub fn json_type(database: Db) -> &'static str {
    match database {
        Db::Pg => "JSONB",
        Db::Mysql => "JSON",
        Db::Sqlite => "TEXT",
    }
}

fn rank(t: &str) -> u8 {
    match base_type(t).as_str() {
        "INTEGER" | "INT" | "TINYINT" => 1,
        "BIGINT" => 2,
        "DECIMAL" | "REAL" => 3,
        "VARCHAR" => 4,
        "TEXT" => 5,
        _ => 0,
    }
}
