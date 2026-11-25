//! The SQL the store speaks, and the rows it reads back.
//!
//! Nothing in this module knows how a statement is run. It turns a query
//! description into a parameterised statement and its parameters, and turns
//! a row of columns and values back into a record. That split is what lets
//! the same engine sit on a local SQLite file and on a platform database:
//! the local store runs the statement, the platform store hands it to a
//! remote service, and both read the row the same way.
//!
//! Two details are worth stating, because they are easy to get wrong:
//!
//! * **Identifiers are quoted.** A column name is part of the statement, so
//!   it is written with `{:?}`, which quotes it the way SQLite does. A
//!   field name is checked against the model's declared columns first, so
//!   nothing that is not a column can reach the statement.
//! * **The extra column holds what the table does not declare.** A record
//!   may carry fields the model has no column for; they travel as JSON in
//!   one extra column and are spread back over the record on the way out.

use mosql::Value;

use crate::error::{AuthError, AuthResult};
use crate::models::{self, RecordMap, EXTRA_COLUMN};
use crate::store::{FilterOp, FindManyQuery, WhereClause};

/// A value as bound to a parameter: booleans as 1/0, everything else as is.
pub fn param(value: &Value) -> Value {
    match value {
        Value::Bool(b) => Value::Int(i64::from(*b)),
        other => other.clone(),
    }
}

/// `WHERE …` or empty. Clause groups are joined exactly as the in-memory
/// store groups them: AND binds tighter than OR.
pub fn where_sql(model: &str, r#where: &[WhereClause]) -> AuthResult<(String, Vec<Value>)> {
    if r#where.is_empty() {
        return Ok((String::new(), Vec::new()));
    }
    let mut sql = String::from("WHERE ");
    let mut params = Vec::new();
    for (index, clause) in r#where.iter().enumerate() {
        clause.check(model)?;
        if index > 0 {
            sql.push_str(if clause.connector_or { " OR " } else { " AND " });
        }
        let column = format!("{:?}", clause.field);
        match clause.op {
            FilterOp::Eq if clause.value.is_null() => sql.push_str(&format!("{column} IS NULL")),
            FilterOp::Ne if clause.value.is_null() => {
                sql.push_str(&format!("{column} IS NOT NULL"))
            }
            FilterOp::Eq => {
                sql.push_str(&format!("{column} = ?"));
                params.push(param(&clause.value));
            }
            FilterOp::Ne => {
                sql.push_str(&format!("{column} != ?"));
                params.push(param(&clause.value));
            }
            FilterOp::Lt => {
                sql.push_str(&format!("{column} < ?"));
                params.push(param(&clause.value));
            }
            FilterOp::Lte => {
                sql.push_str(&format!("{column} <= ?"));
                params.push(param(&clause.value));
            }
            FilterOp::Gt => {
                sql.push_str(&format!("{column} > ?"));
                params.push(param(&clause.value));
            }
            FilterOp::Gte => {
                sql.push_str(&format!("{column} >= ?"));
                params.push(param(&clause.value));
            }
            FilterOp::In => {
                let items = clause.value.as_array().cloned().unwrap_or_default();
                if items.is_empty() {
                    // `IN ()` is a syntax error; an empty list matches nothing.
                    sql.push_str("1 = 0");
                } else {
                    let marks = vec!["?"; items.len()].join(", ");
                    sql.push_str(&format!("{column} IN ({marks})"));
                    params.extend(items.iter().map(param));
                }
            }
            FilterOp::Contains | FilterOp::StartsWith | FilterOp::EndsWith => {
                let pattern = match clause.op {
                    FilterOp::Contains => format!("%{}%", like_escape(&clause.value)),
                    FilterOp::StartsWith => format!("{}%", like_escape(&clause.value)),
                    FilterOp::EndsWith => format!("%{}", like_escape(&clause.value)),
                    _ => unreachable!(),
                };
                sql.push_str(&format!("{column} LIKE ? ESCAPE '\\'"));
                params.push(Value::str(pattern));
            }
        }
    }
    Ok((sql, params))
}

/// Escapes the three characters `LIKE` gives a meaning to.
fn like_escape(value: &Value) -> String {
    let text = value.as_str().unwrap_or("");
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// `INSERT INTO …` for a record that has already been prepared.
///
/// The fields the table declares become columns; the rest travel as JSON in
/// the extra column.
pub fn insert_sql(model: &str, record: &RecordMap) -> AuthResult<(String, Vec<Value>)> {
    check_model(model)?;
    let declared = models::columns(model).expect("checked");
    let mut columns = Vec::new();
    let mut params = Vec::new();
    let mut extra = RecordMap::new();
    for (key, value) in record.iter() {
        if declared.contains(&key.as_str()) {
            columns.push(format!("{key:?}"));
            params.push(param(value));
        } else {
            extra.set(key.clone(), value.clone());
        }
    }
    columns.push(format!("{EXTRA_COLUMN:?}"));
    params.push(Value::str(mosql::to_json(&Value::Object(extra))));
    let marks = vec!["?"; columns.len()].join(", ");
    Ok((
        format!(
            "INSERT INTO {model:?} ({}) VALUES ({marks})",
            columns.join(", ")
        ),
        params,
    ))
}

/// `SELECT * FROM …`, with the ordering and the window the query asks for.
pub fn select_sql(model: &str, query: &FindManyQuery) -> AuthResult<(String, Vec<Value>)> {
    let (where_sql, params) = where_sql(model, &query.r#where)?;
    let mut sql = format!("SELECT * FROM {model:?} {where_sql}");
    if let Some((field, ascending)) = &query.sort_by {
        check_field(model, field)?;
        sql.push_str(&format!(
            " ORDER BY {field:?} {}",
            if *ascending { "ASC" } else { "DESC" }
        ));
    }
    let mut params = params;
    if let Some(limit) = query.limit {
        sql.push_str(" LIMIT ?");
        params.push(Value::Int(limit as i64));
    }
    if let Some(offset) = query.offset {
        sql.push_str(" OFFSET ?");
        params.push(Value::Int(offset as i64));
    }
    Ok((sql, params))
}

/// `SELECT * FROM … LIMIT 1`.
pub fn select_one_sql(model: &str, r#where: &[WhereClause]) -> AuthResult<(String, Vec<Value>)> {
    let (where_sql, params) = where_sql(model, r#where)?;
    let sql = format!("SELECT * FROM {model:?} {where_sql} LIMIT 1");
    Ok((sql, params))
}

/// `SELECT COUNT(*) AS n FROM …`.
pub fn count_sql(model: &str, r#where: &[WhereClause]) -> AuthResult<(String, Vec<Value>)> {
    let (where_sql, params) = where_sql(model, r#where)?;
    let sql = format!("SELECT COUNT(*) AS n FROM {model:?} {where_sql}");
    Ok((sql, params))
}

/// `UPDATE … SET … WHERE …`.
///
/// `None` means there is nothing to set: the caller should report that no
/// row changed rather than run a statement that sets nothing.
pub fn update_sql(
    model: &str,
    r#where: &[WhereClause],
    update: &RecordMap,
) -> AuthResult<Option<(String, Vec<Value>)>> {
    check_update(model, update)?;
    let declared = models::columns(model).expect("checked");
    let mut sets = Vec::new();
    let mut params = Vec::new();
    for (key, value) in update.iter() {
        sets.push(format!("{key:?} = ?"));
        params.push(param(value));
    }
    if declared.contains(&"updatedAt") && update.get("updatedAt").is_none() {
        sets.push("\"updatedAt\" = ?".to_string());
        params.push(Value::str(models::now_iso()));
    }
    if sets.is_empty() {
        return Ok(None);
    }
    let (where_sql, where_params) = where_sql(model, r#where)?;
    params.extend(where_params);
    let sql = format!("UPDATE {model:?} SET {} {where_sql}", sets.join(", "));
    Ok(Some((sql, params)))
}

/// `DELETE FROM … WHERE …`.
pub fn delete_sql(model: &str, r#where: &[WhereClause]) -> AuthResult<(String, Vec<Value>)> {
    let (where_sql, params) = where_sql(model, r#where)?;
    let sql = format!("DELETE FROM {model:?} {where_sql}");
    Ok((sql, params))
}

/// A row as a record.
///
/// The extra column is spread over the record, but only for names the
/// record does not already have: a declared column always wins. A declared
/// boolean column that came back as `0` or `1` becomes a boolean.
pub fn row_to_record(model: &str, columns: &[String], values: &[Value]) -> RecordMap {
    let mut record = RecordMap::new();
    let booleans = models::boolean_columns(model);
    for (column, value) in columns.iter().zip(values.iter()) {
        if column == EXTRA_COLUMN {
            if let Some(text) = value.as_str() {
                if let Ok(Value::Object(extra)) = mosql::parse_json(text) {
                    for (key, value) in extra.iter() {
                        if record.get(key).is_none() {
                            record.set(key.clone(), value.clone());
                        }
                    }
                }
            }
            continue;
        }
        let value = if booleans.contains(&column.as_str()) {
            match value {
                Value::Int(i) => Value::Bool(*i != 0),
                other => other.clone(),
            }
        } else {
            value.clone()
        };
        record.set(column.clone(), value);
    }
    record
}

/// A field the model does not declare cannot be compared.
pub fn check_field(model: &str, field: &str) -> AuthResult<()> {
    let declared = models::columns(model)
        .ok_or_else(|| AuthError::internal(format!("unknown model {model}")))?;
    if !declared.contains(&field) {
        return Err(AuthError::internal(format!(
            "{model} has no field {field:?}"
        )));
    }
    Ok(())
}

/// A model nobody has heard of cannot be queried.
pub fn check_model(model: &str) -> AuthResult<()> {
    if models::columns(model).is_none() {
        return Err(AuthError::internal(format!("unknown model {model}")));
    }
    Ok(())
}

/// An update may only set fields the model declares.
pub fn check_update(model: &str, update: &RecordMap) -> AuthResult<()> {
    for (key, _) in update.iter() {
        let declared = models::columns(model)
            .ok_or_else(|| AuthError::internal(format!("unknown model {model}")))?;
        if !declared.contains(&key.as_str()) {
            return Err(AuthError::internal(format!(
                "{model} has no field {key:?}; undeclared fields can only be set at create time"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object;

    #[test]
    fn a_boolean_binds_as_a_number() {
        assert_eq!(param(&Value::Bool(true)), Value::Int(1));
        assert_eq!(param(&Value::Bool(false)), Value::Int(0));
        assert_eq!(param(&Value::Int(3)), Value::Int(3));
    }

    #[test]
    fn a_like_pattern_escapes_the_wildcards() {
        assert_eq!(like_escape(&Value::str("a%b_c")), "a\\%b\\_c");
    }

    #[test]
    fn the_extra_column_carries_what_the_table_does_not_declare() {
        let record = object! { "email" => Value::str("a@b.test"), "nickname" => Value::str("Ada") };
        let (sql, params) = insert_sql("user", &record).expect("insert");
        assert!(sql.starts_with("INSERT INTO \"user\" ("), "{sql}");
        assert!(sql.contains("\"email\""), "{sql}");
        assert!(sql.contains("\"data\""), "{sql}");
        // The extra column holds the undeclared field, as JSON.
        let extra = params.last().and_then(|v| v.as_str()).unwrap_or("");
        assert!(extra.contains("nickname"), "{extra}");
    }

    #[test]
    fn a_row_expands_the_extra_column_and_turns_integers_into_booleans() {
        let columns = vec![
            "id".to_string(),
            "emailVerified".to_string(),
            "data".to_string(),
        ];
        let values = vec![
            Value::str("u1"),
            Value::Int(1),
            Value::str(r#"{"nickname":"Ada"}"#),
        ];
        let record = row_to_record("user", &columns, &values);
        assert_eq!(record.get("emailVerified"), Some(&Value::Bool(true)));
        assert_eq!(record.get("nickname").and_then(|v| v.as_str()), Some("Ada"));
    }
}
