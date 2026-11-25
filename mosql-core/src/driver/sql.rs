//! The SQL scanner: knowing which `?` are parameters and which are
//! punctuation.
//!
//! A parameter placeholder is a `?` that is *not* inside a string, not
//! inside a quoted identifier, and not inside a comment. Everything else
//! is text.
//!
//! ```
//! use mosql_core::driver::sql::count_parameters;
//!
//! assert_eq!(count_parameters("SELECT * FROM t WHERE a = ? AND b = ?").unwrap(), 2);
//! assert_eq!(count_parameters("SELECT '?' FROM t WHERE a = ?").unwrap(), 1);
//! assert_eq!(count_parameters("SELECT \"?\" FROM t WHERE a = ?").unwrap(), 1);
//! assert_eq!(count_parameters("SELECT `?` FROM t WHERE a = ?").unwrap(), 1);
//! assert_eq!(count_parameters("SELECT 1 -- ?\nWHERE a = ?").unwrap(), 1);
//! assert_eq!(count_parameters("SELECT 1 /* ? */ WHERE a = ?").unwrap(), 1);
//! ```
//!
//! The rules the scanner follows:
//!
//! * `'…'` ends at the next `'`, and `''` inside it is an escaped quote.
//!   A backslash before a quote escapes it too, because MySQL and
//!   SQLite both treat `\'` as a quote.
//! * `"…"` and `` `…` `` end at the next matching quote. MySQL lets a
//!   double quote inside a backtick-quoted identifier be a plain quote.
//! * `-- …` runs to the end of the line.
//! * `/* … */` runs to the closing `*/`.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::driver::DbError;

/// Where the parameters are in a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placeholders {
    /// The byte offsets of the `?` characters, in order.
    pub positions: Vec<usize>,
}

impl Placeholders {
    /// How many parameters the statement has.
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// Whether the statement has any parameters.
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }
}

/// Find the parameters in a statement.
pub fn scan(sql: &str) -> Result<Placeholders, DbError> {
    let bytes = sql.as_bytes();
    let mut positions = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                i = end_of_quoted(bytes, i, b'\'')?;
            }
            b'"' => {
                i = end_of_quoted(bytes, i, b'"')?;
            }
            b'`' => {
                i = end_of_quoted(bytes, i, b'`')?;
            }
            b'-' if i + 1 < bytes.len() && bytes[i + 1] == b'-' => {
                // A line comment: to the end of the line.
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                // A block comment: to the closing `*/`.
                i += 2;
                loop {
                    if i + 1 >= bytes.len() {
                        return Err(DbError::new("the statement has an unclosed /* comment"));
                    }
                    if bytes[i] == b'*' && bytes[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'?' => {
                positions.push(i);
                i += 1;
            }
            _ => i += 1,
        }
    }
    Ok(Placeholders { positions })
}

/// How many parameters a statement has.
pub fn count_parameters(sql: &str) -> Result<usize, DbError> {
    Ok(scan(sql)?.len())
}

/// The byte offset just past a quoted run.
///
/// `start` is the opening quote. The run ends at the next unescaped
/// quote of the same kind, or at the end of the statement — which is an
/// error, because a statement with an unterminated string is a
/// statement nobody should send.
fn end_of_quoted(bytes: &[u8], start: usize, quote: u8) -> Result<usize, DbError> {
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if quote == b'\'' || quote == b'`' => {
                // A backslash escapes the next character.
                i += 2;
            }
            b'\'' if quote == b'\'' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'\'' {
                    // `''` is an escaped quote, not the end.
                    i += 2;
                } else {
                    return Ok(i + 1);
                }
            }
            b'"' if quote == b'"' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'"' {
                    i += 2;
                } else {
                    return Ok(i + 1);
                }
            }
            b'`' if quote == b'`' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'`' {
                    i += 2;
                } else {
                    return Ok(i + 1);
                }
            }
            _ => i += 1,
        }
    }
    Err(DbError::new("the statement has an unterminated string"))
}

/// Rewrite `?` placeholders as Postgres's `$1`, `$2`, …
///
/// The numbers are the position of the parameter, counting from one, in
/// the order the parameters appear — which is the order they are given
/// in.
pub fn rewrite_pg_placeholders(sql: &str) -> Result<String, DbError> {
    let placeholders = scan(sql)?;
    let mut out = String::with_capacity(sql.len() + placeholders.len() * 2);
    let mut last = 0usize;
    for (index, position) in placeholders.positions.iter().enumerate() {
        out.push_str(&sql[last..*position]);
        out.push('$');
        out.push_str(&(index + 1).to_string());
        last = position + 1;
    }
    out.push_str(&sql[last..]);
    Ok(out)
}

/// Put the parameters into the statement, as text, the way MySQL's
/// clients do when there is no prepared statement.
///
/// The values are escaped, so a parameter cannot change the statement.
/// The escapes are MySQL's:
///
/// | value | becomes |
/// | --- | --- |
/// | `NULL` | `NULL` |
/// | `TRUE` / `FALSE` | `1` / `0` |
/// | an integer | its digits |
/// | a float | a finite decimal — `NaN` and the infinities are refused |
/// | text | `'…'`, with `\` `'` `"` newline carriage-return null and Ctrl-Z escaped |
/// | a blob | `X'…'`, the bytes in hex |
pub fn inline_mysql_params(sql: &str, params: &[SqlValue]) -> Result<String, DbError> {
    let placeholders = scan(sql)?;
    if placeholders.len() != params.len() {
        return Err(DbError::new(format!(
            "the statement has {} placeholder(s) but {} value(s) were given",
            placeholders.len(),
            params.len()
        )));
    }
    let mut out = String::with_capacity(sql.len() + params.len() * 8);
    let mut last = 0usize;
    for (index, position) in placeholders.positions.iter().enumerate() {
        out.push_str(&sql[last..*position]);
        out.push_str(&mysql_literal(&params[index])?);
        last = position + 1;
    }
    out.push_str(&sql[last..]);
    Ok(out)
}

/// One value, as MySQL text.
fn mysql_literal(value: &SqlValue) -> Result<String, DbError> {
    Ok(match value {
        SqlValue::Null => "NULL".to_string(),
        SqlValue::Bool(true) => "1".to_string(),
        SqlValue::Bool(false) => "0".to_string(),
        SqlValue::Int(number) => number.to_string(),
        SqlValue::Float(number) => {
            if !number.is_finite() {
                return Err(DbError::new(
                    "a floating point value must be finite: NaN and the infinities have no SQL spelling",
                ));
            }
            // `{:?}` gives `1.0` for 1.0, which MySQL reads as a decimal
            // — that is right. But it gives `1e20` for a large number,
            // which is also right, and `inf` for an infinity, which the
            // check above has already refused.
            let mut text = format!("{number:?}");
            if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                text.push_str(".0");
            }
            text
        }
        SqlValue::Text(text) => {
            let mut out = String::with_capacity(text.len() + 2);
            out.push('\'');
            for ch in text.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\'' => out.push_str("\\'"),
                    '"' => out.push_str("\\\""),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\0' => out.push_str("\\0"),
                    '\u{1a}' => out.push_str("\\Z"),
                    other => out.push(other),
                }
            }
            out.push('\'');
            out
        }
        SqlValue::Blob(bytes) => {
            let mut out = String::with_capacity(bytes.len() * 2 + 3);
            out.push_str("X'");
            out.push_str(&crate::driver::encoding::hex_encode(bytes));
            out.push('\'');
            out
        }
    })
}

/// The values a statement's parameters stand for.
use crate::driver::SqlValue;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_question_mark_is_a_parameter() {
        assert_eq!(count_parameters("?").unwrap(), 1);
        assert_eq!(count_parameters("SELECT ?").unwrap(), 1);
        assert_eq!(count_parameters("a ? b ? c").unwrap(), 2);
    }

    #[test]
    fn a_question_mark_in_a_string_is_not_a_parameter() {
        assert_eq!(count_parameters("SELECT '?'").unwrap(), 0);
        assert_eq!(count_parameters("SELECT 'a?b'").unwrap(), 0);
        assert_eq!(count_parameters("SELECT 'it''s a ?'").unwrap(), 0);
        assert_eq!(count_parameters("SELECT 'it\\'s a ?'").unwrap(), 0);
    }

    #[test]
    fn a_question_mark_in_an_identifier_is_not_a_parameter() {
        assert_eq!(count_parameters("SELECT \"?\"").unwrap(), 0);
        assert_eq!(count_parameters("SELECT `?`").unwrap(), 0);
        assert_eq!(count_parameters("SELECT \"a\"\"?b\"").unwrap(), 0);
    }

    #[test]
    fn a_question_mark_in_a_comment_is_not_a_parameter() {
        assert_eq!(count_parameters("SELECT 1 -- ?").unwrap(), 0);
        assert_eq!(count_parameters("SELECT 1 -- ?\n, ?").unwrap(), 1);
        assert_eq!(count_parameters("SELECT 1 /* ? */ , ?").unwrap(), 1);
        assert_eq!(count_parameters("/* ? */").unwrap(), 0);
    }

    #[test]
    fn the_mix_is_counted_right() {
        assert_eq!(
            count_parameters("SELECT '?', \"?\", `?`, -- ?\n /* ? */ ?").unwrap(),
            1
        );
    }

    #[test]
    fn an_unterminated_string_is_an_error() {
        assert!(scan("SELECT 'unterminated").is_err());
        assert!(scan("SELECT `unterminated").is_err());
        assert!(scan("SELECT /* unterminated").is_err());
    }

    #[test]
    fn postgres_placeholders_are_numbered() {
        assert_eq!(
            rewrite_pg_placeholders("SELECT * FROM t WHERE a = ? AND b = ?").unwrap(),
            "SELECT * FROM t WHERE a = $1 AND b = $2"
        );
        assert_eq!(
            rewrite_pg_placeholders("SELECT '?', ?").unwrap(),
            "SELECT '?', $1"
        );
        assert_eq!(rewrite_pg_placeholders("SELECT 1").unwrap(), "SELECT 1");
    }

    #[test]
    fn the_number_of_parameters_must_match() {
        let err = inline_mysql_params("SELECT ?", &[]).unwrap_err();
        assert!(err.message.contains("1 placeholder"), "{}", err.message);
        let err = inline_mysql_params("SELECT ?, ?", &[SqlValue::Int(1)]).unwrap_err();
        assert!(err.message.contains("2 placeholder"), "{}", err.message);
    }

    #[test]
    fn values_are_escaped() {
        assert_eq!(
            inline_mysql_params("SELECT ?", &[SqlValue::Null]).unwrap(),
            "SELECT NULL"
        );
        assert_eq!(
            inline_mysql_params("SELECT ?", &[SqlValue::Bool(true)]).unwrap(),
            "SELECT 1"
        );
        assert_eq!(
            inline_mysql_params("SELECT ?", &[SqlValue::Int(42)]).unwrap(),
            "SELECT 42"
        );
        assert_eq!(
            inline_mysql_params("SELECT ?", &[SqlValue::Float(1.5)]).unwrap(),
            "SELECT 1.5"
        );
        assert_eq!(
            inline_mysql_params("SELECT ?", &[SqlValue::Text("it's".into())]).unwrap(),
            "SELECT 'it\\'s'"
        );
    }

    #[test]
    fn an_injection_payload_stays_a_string() {
        let payload = "'; DROP TABLE users; --";
        let statement = inline_mysql_params(
            "SELECT * FROM t WHERE name = ?",
            &[SqlValue::Text(payload.into())],
        )
        .unwrap();
        // The quote inside the payload is escaped, so the payload stays
        // one string and the statement stays one statement.
        assert_eq!(
            statement,
            "SELECT * FROM t WHERE name = '\\'; DROP TABLE users; --'"
        );
        // The dangerous words are inside the quotes, so the statement is
        // still one statement.
        assert_eq!(count_parameters(&statement).unwrap(), 0);
    }

    #[test]
    fn a_null_byte_in_text_is_escaped() {
        let statement = inline_mysql_params("SELECT ?", &[SqlValue::Text("a\0b".into())]).unwrap();
        assert_eq!(statement, "SELECT 'a\\0b'");
    }

    #[test]
    fn a_backslash_in_text_is_escaped() {
        let statement = inline_mysql_params("SELECT ?", &[SqlValue::Text("a\\b".into())]).unwrap();
        assert_eq!(statement, "SELECT 'a\\\\b'");
    }

    #[test]
    fn a_blob_becomes_hex() {
        let statement =
            inline_mysql_params("SELECT ?", &[SqlValue::Blob(vec![0x00, 0x0f, 0xff])]).unwrap();
        assert_eq!(statement, "SELECT X'000fff'");
    }

    #[test]
    fn a_not_a_number_is_refused() {
        let err = inline_mysql_params("SELECT ?", &[SqlValue::Float(f64::NAN)]).unwrap_err();
        assert!(err.message.contains("finite"), "{}", err.message);
        let err = inline_mysql_params("SELECT ?", &[SqlValue::Float(f64::INFINITY)]).unwrap_err();
        assert!(err.message.contains("finite"), "{}", err.message);
        let err =
            inline_mysql_params("SELECT ?", &[SqlValue::Float(f64::NEG_INFINITY)]).unwrap_err();
        assert!(err.message.contains("finite"), "{}", err.message);
    }
}
