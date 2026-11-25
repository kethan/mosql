//! A fake database for the tests.
//!
//! It answers introspection queries from a real table/column map that the
//! generated `CREATE TABLE` / `ALTER TABLE` statements maintain, so a test that
//! asserts "the column was added" is checking the DDL round-trip rather than a
//! string that happens to appear in a log. Everything else comes from a
//! scripted queue.

#![allow(dead_code)]

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use mosql::{Db, Value};
use mosql_schemaless::executor::{BoxFuture, ExecResult, SqlError, SqlExecutor};
use mosql_schemaless::{CollectionOptions, SqlAdapter, SqlCollection};

/// What the fake answers for a statement it does not understand.
#[derive(Debug, Clone)]
pub enum Reply {
    /// Rows to return.
    Rows(Vec<Value>),
    /// An affected count.
    Exec(i64),
    /// An affected count and an insert id.
    Insert(i64, i64),
    /// An error to raise.
    Err(SqlError),
}

#[derive(Default)]
pub struct FakeDb {
    pub tables: Mutex<HashMap<String, Vec<(String, String)>>>,
    pub log: Mutex<Vec<String>>,
    pub replies: Mutex<VecDeque<Reply>>,
}

impl FakeDb {
    /// A shared fake, as a driver would be.
    pub fn shared() -> Arc<FakeDb> {
        Arc::new(FakeDb::default())
    }

    /// Queue the answer to the next non-DDL statement.
    pub fn say(&self, reply: Reply) {
        self.replies.lock().unwrap().push_back(reply)
    }

    /// Every statement seen so far.
    pub fn logged(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    /// The user columns the fake currently has for `table`.
    pub fn columns(&self, table: &str) -> Vec<(String, String)> {
        self.tables
            .lock()
            .unwrap()
            .get(table)
            .cloned()
            .unwrap_or_default()
    }
}

/// A collection over a shared fake.
pub fn collection_on(
    fake: &Arc<FakeDb>,
    db: Db,
    table: &str,
    options: CollectionOptions,
) -> SqlCollection {
    let executor: Arc<FakeDb> = Arc::clone(fake);
    let executor: Arc<dyn SqlExecutor> = executor;
    SqlCollection::new(
        table,
        Arc::new(SqlAdapter::with_shared(db, executor)),
        options,
    )
}

impl SqlExecutor for FakeDb {
    fn execute<'a>(
        &'a self,
        sql: &'a str,
        _params: &'a [Value],
    ) -> BoxFuture<'a, Result<ExecResult, SqlError>> {
        Box::pin(async move {
            self.log.lock().unwrap().push(sql.to_string());
            let upper = sql.trim().to_uppercase();

            // Introspection: answered from the map, in every dialect's shape at
            // once (the adapter reads whichever keys its dialect uses).
            if upper.starts_with("PRAGMA TABLE_INFO") {
                // Read the argument from the original text: the upper-cased copy
                // would look up `USERS` in a map keyed by `users`.
                let table = between(sql, '(', ')').unwrap_or_default();
                return Ok(ExecResult::rows(
                    self.columns(&table)
                        .into_iter()
                        .map(|(name, kind)| {
                            row(&[("name", Value::str(name)), ("type", Value::str(kind))])
                        })
                        .collect(),
                ));
            }
            if upper.contains("INFORMATION_SCHEMA.COLUMNS") {
                let table = literal(sql);
                return Ok(ExecResult::rows(
                    self.columns(&table)
                        .into_iter()
                        .map(|(name, kind)| {
                            row(&[
                                ("column_name", Value::str(name.clone())),
                                ("COLUMN_NAME", Value::str(name)),
                                ("data_type", Value::str(kind.clone())),
                                ("DATA_TYPE", Value::str(kind)),
                            ])
                        })
                        .collect(),
                ));
            }
            if upper.contains("INFORMATION_SCHEMA.TABLES") {
                let table = literal(sql);
                let exists = self.tables.lock().unwrap().contains_key(&table);
                return Ok(ExecResult::rows(if exists {
                    vec![row(&[
                        ("exists", Value::Int(1)),
                        ("count", Value::Int(1)),
                        ("name", Value::str(table)),
                    ])]
                } else {
                    vec![]
                }));
            }
            if upper.contains("SQLITE_MASTER") && upper.starts_with("SELECT NAME") {
                let table = literal(sql);
                let exists = self.tables.lock().unwrap().contains_key(&table);
                return Ok(ExecResult::rows(if exists {
                    vec![row(&[("name", Value::str(table))])]
                } else {
                    vec![]
                }));
            }

            // DDL: maintain the map so introspection reflects it.
            if upper.starts_with("CREATE TABLE") {
                if let Some(table) = create_table_name(sql) {
                    self.tables.lock().unwrap().entry(table).or_default();
                }
                return Ok(ExecResult::affected(0));
            }
            if upper.starts_with("ALTER TABLE") && upper.contains(" ADD COLUMN ") {
                if let Some((table, column, kind)) = add_column_parts(sql) {
                    self.tables
                        .lock()
                        .unwrap()
                        .entry(table)
                        .or_default()
                        .push((column, kind));
                }
                return Ok(ExecResult::affected(0));
            }
            if upper.starts_with("ALTER TABLE") {
                return Ok(ExecResult::affected(0));
            }
            if upper.starts_with("SHOW INDEX") {
                return Ok(ExecResult::rows(Vec::new()));
            }
            if upper.starts_with("CREATE ") && upper.contains("INDEX") {
                return Ok(ExecResult::affected(0));
            }

            // Everything else: scripted.
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(Reply::Exec(0));
            match reply {
                Reply::Rows(rows) => Ok(ExecResult::rows(rows)),
                Reply::Exec(n) => Ok(ExecResult::affected(n)),
                Reply::Insert(affected, id) => Ok(ExecResult {
                    rows: Vec::new(),
                    affected,
                    insert_id: Some(Value::Int(id)),
                }),
                Reply::Err(e) => Err(e),
            }
        })
    }
}

/// Build an object from key/value pairs.
pub fn row(pairs: &[(&str, Value)]) -> Value {
    let mut out = mosql::Object::new();
    for (key, value) in pairs {
        out.set((*key).to_string(), value.clone());
    }
    Value::Object(out)
}

/// `parse_json`, panicking on malformed literals — test data is hand-written.
pub fn json(text: &str) -> Value {
    mosql::parse_json(text).expect("valid json")
}

fn between(s: &str, open: char, close: char) -> Option<String> {
    let start = s.find(open)? + 1;
    let end = s[start..].find(close)? + start;
    Some(s[start..end].trim().to_string())
}

/// The table name in `... table_name = 'users'` (or the PRAGMA argument).
fn literal(sql: &str) -> String {
    if let Some(rest) = sql.split("table_name = '").nth(1) {
        if let Some(name) = rest.split('\'').next() {
            return name.to_string();
        }
    }
    if let Some(rest) = sql.split("TABLE_NAME = '").nth(1) {
        if let Some(name) = rest.split('\'').next() {
            return name.to_string();
        }
    }
    if let Some(rest) = sql.split("name='").nth(1) {
        if let Some(name) = rest.split('\'').next() {
            return name.to_string();
        }
    }
    if sql.to_uppercase().starts_with("PRAGMA") {
        if let Some(name) = between(sql, '(', ')') {
            return name;
        }
    }
    String::new()
}

fn create_table_name(sql: &str) -> Option<String> {
    let rest = sql.trim().strip_prefix("CREATE TABLE")?;
    let rest = rest.trim_start();
    let rest = rest
        .strip_prefix("IF NOT EXISTS")
        .unwrap_or(rest)
        .trim_start();
    let end = rest.find([' ', '(']).unwrap_or(rest.len());
    Some(rest[..end].trim_matches('`').to_string())
}

fn add_column_parts(sql: &str) -> Option<(String, String, String)> {
    let mut parts = sql.split_whitespace();
    let _alter = parts.next()?;
    let _table = parts.next()?;
    let table = parts.next()?.trim_matches('`').to_string();
    let _add = parts.next()?;
    let _column = parts.next()?;
    let column = parts.next()?.trim_matches('`').to_string();
    let kind = parts.collect::<Vec<_>>().join(" ");
    Some((table, column, kind))
}
