//! Identifier validation — a direct port of the JS `Validate` helper.

use crate::error::{Error, Result};
use crate::value::Value;
use crate::Db;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// MySQL 8.0 reserved words that must be backtick-quoted when used as identifiers.
/// https://dev.mysql.com/doc/refman/8.0/en/keywords.html
const MYSQL_RESERVED: &[&str] = &[
    "accessible",
    "add",
    "all",
    "alter",
    "analyze",
    "and",
    "as",
    "asensitive",
    "before",
    "between",
    "bigint",
    "binary",
    "blob",
    "both",
    "by",
    "call",
    "cascade",
    "case",
    "change",
    "char",
    "character",
    "check",
    "collate",
    "column",
    "condition",
    "constraint",
    "continue",
    "convert",
    "create",
    "cross",
    "cube",
    "cume_dist",
    "current_date",
    "current_time",
    "current_timestamp",
    "current_user",
    "cursor",
    "database",
    "databases",
    "day_hour",
    "day_microsecond",
    "day_minute",
    "day_second",
    "dec",
    "decimal",
    "declare",
    "default",
    "delayed",
    "delete",
    "dense_rank",
    "desc",
    "describe",
    "deterministic",
    "distinct",
    "distinctrow",
    "div",
    "double",
    "drop",
    "dual",
    "each",
    "else",
    "elseif",
    "empty",
    "enclosed",
    "escaped",
    "except",
    "exists",
    "exit",
    "explain",
    "false",
    "fetch",
    "first_value",
    "float",
    "float4",
    "float8",
    "for",
    "force",
    "foreign",
    "from",
    "fulltext",
    "function",
    "generated",
    "get",
    "grant",
    "group",
    "grouping",
    "groups",
    "having",
    "high_priority",
    "hour_microsecond",
    "hour_minute",
    "hour_second",
    "if",
    "ignore",
    "in",
    "index",
    "infile",
    "inner",
    "inout",
    "insensitive",
    "insert",
    "int",
    "int1",
    "int2",
    "int3",
    "int4",
    "int8",
    "integer",
    "intersect",
    "interval",
    "into",
    "io_after_gtids",
    "io_before_gtids",
    "is",
    "iterate",
    "join",
    "json_table",
    "key",
    "keys",
    "kill",
    "lag",
    "last_value",
    "lateral",
    "lead",
    "leading",
    "leave",
    "left",
    "like",
    "limit",
    "linear",
    "lines",
    "load",
    "localtime",
    "localtimestamp",
    "lock",
    "long",
    "longblob",
    "longtext",
    "loop",
    "low_priority",
    "master_bind",
    "master_ssl_verify_server_cert",
    "match",
    "maxvalue",
    "mediumblob",
    "mediumint",
    "mediumtext",
    "middleint",
    "minute_microsecond",
    "minute_second",
    "mod",
    "modifies",
    "natural",
    "not",
    "no_write_to_binlog",
    "nth_value",
    "ntile",
    "null",
    "numeric",
    "of",
    "on",
    "optimize",
    "optimizer_costs",
    "option",
    "optionally",
    "or",
    "order",
    "out",
    "outer",
    "outfile",
    "over",
    "partition",
    "percent_rank",
    "precision",
    "primary",
    "procedure",
    "purge",
    "range",
    "rank",
    "read",
    "reads",
    "read_write",
    "real",
    "recursive",
    "references",
    "regexp",
    "release",
    "rename",
    "repeat",
    "replace",
    "require",
    "resignal",
    "restrict",
    "return",
    "revoke",
    "right",
    "rlike",
    "row",
    "rows",
    "row_number",
    "schema",
    "schemas",
    "second_microsecond",
    "select",
    "sensitive",
    "separator",
    "set",
    "show",
    "signal",
    "smallint",
    "spatial",
    "specific",
    "sql",
    "sqlexception",
    "sqlstate",
    "sqlwarning",
    "sql_big_result",
    "sql_calc_found_rows",
    "sql_small_result",
    "ssl",
    "starting",
    "stored",
    "straight_join",
    "system",
    "table",
    "terminated",
    "then",
    "tinyblob",
    "tinyint",
    "tinytext",
    "to",
    "trailing",
    "trigger",
    "true",
    "undo",
    "union",
    "unique",
    "unlock",
    "unsigned",
    "update",
    "usage",
    "use",
    "using",
    "utc_date",
    "utc_time",
    "utc_timestamp",
    "values",
    "varbinary",
    "varchar",
    "varcharacter",
    "varying",
    "virtual",
    "when",
    "where",
    "while",
    "window",
    "with",
    "write",
    "xor",
    "year_month",
    "zerofill",
    "_filename",
];

fn strip_quotes(s: &str) -> &str {
    if s.len() >= 2
        && ((s.starts_with('`') && s.ends_with('`')) || (s.starts_with('"') && s.ends_with('"')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Validate and quote an identifier (or dotted path) for the target database.
pub fn col(name: &str, db: Db) -> Result<String> {
    if name.is_empty() {
        return Err(Error::new(format!("Invalid column: {}", name)));
    }
    let parts: Vec<&str> = name.split('.').map(strip_quotes).collect();
    for p in &parts {
        if p.is_empty() || !p.chars().all(is_word_char) {
            return Err(Error::new(format!("Invalid column: {}", name)));
        }
    }
    if db == Db::Mysql {
        let quoted: Vec<String> = parts
            .iter()
            .map(|p| {
                let lower = p.to_ascii_lowercase();
                if MYSQL_RESERVED.contains(&lower.as_str()) {
                    format!("`{}`", p)
                } else {
                    (*p).to_string()
                }
            })
            .collect();
        return Ok(quoted.join("."));
    }
    Ok(parts.join("."))
}

/// The same, written into a buffer the caller already owns.
///
/// The SQL builder writes into one string rather than making one per
/// fragment: a query with five conditions used to make a dozen
/// intermediate strings, and the allocations cost more than the parsing.
pub fn write_col(out: &mut String, name: &str, db: Db) -> Result<()> {
    if name.is_empty() {
        return Err(Error::new(format!("Invalid column: {}", name)));
    }
    let mut first = true;
    for part in name.split('.').map(strip_quotes) {
        if part.is_empty() || !part.chars().all(is_word_char) {
            return Err(Error::new(format!("Invalid column: {}", name)));
        }
        if !first {
            out.push('.');
        }
        first = false;
        if db == Db::Mysql {
            let lower = part.to_ascii_lowercase();
            if MYSQL_RESERVED.contains(&lower.as_str()) {
                out.push('`');
                out.push_str(part);
                out.push('`');
            } else {
                out.push_str(part);
            }
        } else {
            out.push_str(part);
        }
    }
    Ok(())
}

/// Replace every non-word character with `_` (used for derived aliases).
pub fn alias(s: &str) -> String {
    s.chars()
        .map(|c| if is_word_char(c) { c } else { '_' })
        .collect()
}

pub fn arr<'a>(value: &'a Value, op: &str) -> Result<&'a Vec<Value>> {
    match value {
        Value::Array(items) => Ok(items),
        _ => Err(Error::new(format!("{} requires array", op))),
    }
}

/// JS `Number(v)` + `Number.isInteger(v) && v >= 0`.
pub fn int(value: &Value, op: &str) -> Result<i64> {
    let n = value.to_number().unwrap_or(f64::NAN);
    if !n.is_finite() || n % 1.0 != 0.0 || n < 0.0 {
        return Err(Error::new(format!("{} requires non-negative integer", op)));
    }
    Ok(n as i64)
}
