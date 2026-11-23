//! The port of `schemaless.js`, statement by statement.

mod common;

use std::sync::Arc;

use mosql::{Db, Value};
use mosql_schemaless::executor::SqlError;
use mosql_schemaless::{infer_sql_type, widen_type, CollectionOptions, IdStrategy};
use uflite::db::{block_on, Collection, FindOptions};

use common::{collection_on, json, row, FakeDb, Reply};

/// Only the statements a test is asserting on: the introspection probes the
/// collection runs before every statement are noise.
fn statements(fake: &FakeDb) -> Vec<String> {
    fake.logged()
        .into_iter()
        .filter(|s| {
            let u = s.to_uppercase();
            let probe = u.starts_with("PRAGMA")
                || u.contains("SQLITE_MASTER")
                || u.contains("INFORMATION_SCHEMA");
            !probe
                && (u.starts_with("CREATE")
                    || u.starts_with("ALTER")
                    || u.starts_with("INSERT")
                    || u.starts_with("UPDATE")
                    || u.starts_with("DELETE")
                    || u.starts_with("SELECT")
                    || u.starts_with("SHOW"))
        })
        .collect()
}

fn sqlite(fake: &Arc<FakeDb>, table: &str) -> mosql_schemaless::SqlCollection {
    collection_on(fake, Db::Sqlite, table, CollectionOptions::default())
}

#[test]
fn insert_creates_the_table_then_its_columns() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Insert(1, 7));

    let id = block_on(users.insert_one(json(r#"{"name":"Ada","age":36}"#))).unwrap();

    assert_eq!(id, Value::Int(7));
    let log = statements(&fake);
    assert_eq!(log[0], "CREATE TABLE IF NOT EXISTS users (_id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT DEFAULT (datetime('now')), updated_at TEXT DEFAULT (datetime('now')))");
    assert_eq!(log[1], "ALTER TABLE users ADD COLUMN name TEXT");
    assert_eq!(log[2], "ALTER TABLE users ADD COLUMN age INTEGER");
    assert!(log[3].starts_with("INSERT INTO users"), "{}", log[3]);
    assert!(log[3].contains("'Ada'"), "{}", log[3]);
}

#[test]
fn a_postgres_table_uses_serial_and_its_own_types() {
    let fake = FakeDb::shared();
    let users = collection_on(&fake, Db::Pg, "users", CollectionOptions::default());

    block_on(users.create_table(&json(
        r#"{"a":"x","b":1,"c":1.5,"d":true,"e":[1],"f":"long"}"#,
    )))
    .unwrap();

    let log = statements(&fake);
    assert_eq!(log[0], "CREATE TABLE IF NOT EXISTS users (_id SERIAL PRIMARY KEY, created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP)");
    assert!(log[1].contains("a VARCHAR(255)"), "{}", log[1]);
    assert!(log[2].contains("b INTEGER"), "{}", log[2]);
    assert!(log[3].contains("c DECIMAL(20,6)"), "{}", log[3]);
    assert!(log[4].contains("d BOOLEAN"), "{}", log[4]);
    assert!(log[5].contains("e JSONB"), "{}", log[5]);
}

#[test]
fn a_mysql_table_uses_auto_increment_and_its_own_types() {
    let fake = FakeDb::shared();
    let users = collection_on(&fake, Db::Mysql, "users", CollectionOptions::default());

    block_on(users.create_table(&json(r#"{"a":"x","b":1,"c":1.5,"d":true,"e":[1]}"#))).unwrap();

    let log = statements(&fake);
    assert_eq!(log[0], "CREATE TABLE IF NOT EXISTS users (_id INT AUTO_INCREMENT PRIMARY KEY, created_at DATETIME DEFAULT CURRENT_TIMESTAMP, updated_at DATETIME DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP)");
    assert!(log[1].contains("a VARCHAR(255)"), "{}", log[1]);
    assert!(log[2].contains("b INT"), "{}", log[2]);
    assert!(log[3].contains("c DECIMAL(20,6)"), "{}", log[3]);
    assert!(log[4].contains("d TINYINT(1)"), "{}", log[4]);
    assert!(log[5].contains("e JSON"), "{}", log[5]);
}

#[test]
fn a_string_over_255_characters_becomes_text() {
    assert_eq!(
        infer_sql_type(&Value::str("x".repeat(255)), Db::Pg),
        "VARCHAR(255)"
    );
    assert_eq!(infer_sql_type(&Value::str("x".repeat(256)), Db::Pg), "TEXT");
    assert_eq!(
        infer_sql_type(&Value::str("x".repeat(256)), Db::Mysql),
        "TEXT"
    );
    assert_eq!(
        infer_sql_type(&Value::str("x".repeat(256)), Db::Sqlite),
        "TEXT"
    );
}

#[test]
fn the_declared_schema_wins_over_inference() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Pg,
        "users",
        CollectionOptions {
            schema: json(r#"{"age":"BIGINT","name":{"type":"VARCHAR(40)","required":true}}"#),
            ..CollectionOptions::default()
        },
    );

    block_on(users.create_table(&json(r#"{"name":"Ada","age":36}"#))).unwrap();

    let log = statements(&fake);
    assert!(log[1].contains("age BIGINT"), "{}", log[1]);
    assert!(log[2].contains("name VARCHAR(40) NOT NULL"), "{}", log[2]);
}

#[test]
fn unique_and_default_come_from_the_schema() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Pg,
        "users",
        CollectionOptions {
            schema: json(
                r#"{"email":{"type":"TEXT","unique":true},"role":{"type":"TEXT","default":"user"}}"#,
            ),
            ..CollectionOptions::default()
        },
    );

    block_on(users.create_table(&Value::object())).unwrap();

    let log = statements(&fake);
    assert!(log[1].contains("email TEXT UNIQUE"), "{}", log[1]);
    assert!(log[2].contains("role TEXT DEFAULT 'user'"), "{}", log[2]);
}

#[test]
fn a_new_field_adds_a_column() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Insert(1, 1));
    block_on(users.insert_one(json(r#"{"name":"Ada"}"#))).unwrap();

    fake.say(Reply::Insert(1, 2));
    block_on(users.insert_one(json(r#"{"name":"Bob","age":4}"#))).unwrap();

    let alters: Vec<String> = statements(&fake)
        .into_iter()
        .filter(|s| s.starts_with("ALTER TABLE"))
        .collect();
    assert_eq!(
        alters,
        vec![
            "ALTER TABLE users ADD COLUMN name TEXT",
            "ALTER TABLE users ADD COLUMN age INTEGER"
        ]
    );
}

#[test]
fn a_missing_table_is_created_and_the_statement_retried() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Err(SqlError::with_code(
        "42P01",
        "relation \"users\" does not exist",
    )));
    fake.say(Reply::Rows(vec![row(&[("name", Value::str("Ada"))])]));

    let rows = block_on(users.find(&Value::object(), None, FindOptions::default())).unwrap();

    assert_eq!(rows.len(), 1);
    let log = statements(&fake);
    assert_eq!(log[0], "SELECT * FROM users");
    assert_eq!(log[1], "CREATE TABLE IF NOT EXISTS users (_id INTEGER PRIMARY KEY AUTOINCREMENT, created_at TEXT DEFAULT (datetime('now')), updated_at TEXT DEFAULT (datetime('now')))");
    assert_eq!(log[2], "SELECT * FROM users");
}

#[test]
fn a_missing_column_migrates_when_the_statement_allows_it() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Err(SqlError::with_code(
        "42703",
        "column \"age\" does not exist",
    )));
    fake.say(Reply::Insert(1, 1));

    block_on(users.insert_one(json(r#"{"name":"Ada","age":36}"#))).unwrap();

    let log = statements(&fake);
    assert!(
        log.iter()
            .any(|s| s == "ALTER TABLE users ADD COLUMN age INTEGER"),
        "{log:?}"
    );
}

#[test]
fn a_missing_column_is_reported_when_it_may_not_migrate() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Err(SqlError::with_code(
        "42703",
        "column \"age\" does not exist",
    )));

    let err = block_on(users.find(&json(r#"{"age":36}"#), None, FindOptions::default()))
        .expect_err("reads do not migrate");

    assert!(err.to_string().contains("does not exist"), "{err}");
}

#[test]
fn an_unrelated_error_is_returned_as_is() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Err(SqlError::new(
        "syntax error at or near \"FROM\"",
    )));

    let err = block_on(users.find(&Value::object(), None, FindOptions::default()))
        .expect_err("a syntax error is not a missing table");

    assert_eq!(err.to_string(), "syntax error at or near \"FROM\"");
}

#[test]
fn find_sorts_skips_and_limits() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Rows(vec![row(&[("name", Value::str("Ada"))])]));

    let rows = block_on(users.find(
        &json(r#"{"age":{"$gte":18}}"#),
        None,
        FindOptions {
            skip: 5,
            limit: Some(10),
            sort: Some(json(r#"{"age":-1}"#)),
        },
    ))
    .unwrap();

    assert_eq!(rows.len(), 1);
    let sql = statements(&fake).pop().unwrap();
    assert!(sql.contains("WHERE"), "{sql}");
    assert!(sql.contains("ORDER BY"), "{sql}");
    assert!(sql.contains("LIMIT 10"), "{sql}");
    assert!(sql.contains("OFFSET 5"), "{sql}");
}

#[test]
fn count_documents_reads_the_count_row() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Rows(vec![row(&[("count", Value::Int(42))])]));

    assert_eq!(
        block_on(users.count_documents(&Value::object())).unwrap(),
        42
    );
}

#[test]
fn an_update_reports_how_many_matched_and_how_many_changed() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Rows(vec![row(&[("count", Value::Int(3))])]));
    fake.say(Reply::Exec(2));

    let matched =
        block_on(users.update_one(&json(r#"{"age":36}"#), &json(r#"{"$set":{"age":37}}"#)))
            .unwrap();
    assert_eq!(matched, 1, "one update matches at most one");

    let log = statements(&fake);
    assert!(log[1].starts_with("UPDATE users SET"), "{log:?}");
}

#[test]
fn update_many_matches_every_document() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Rows(vec![row(&[("count", Value::Int(3))])]));
    fake.say(Reply::Exec(2));

    let matched =
        block_on(users.update_many(&json(r#"{"age":36}"#), &json(r#"{"$set":{"age":37}}"#)))
            .unwrap();
    assert_eq!(matched, 3);
}

#[test]
fn delete_returns_the_affected_count() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Exec(4));

    assert_eq!(block_on(users.delete_many(&Value::object())).unwrap(), 4);
    let sql = statements(&fake).pop().unwrap();
    assert!(sql.starts_with("DELETE FROM users"), "{sql}");
}

#[test]
fn an_upsert_inserts_when_nothing_matched() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Rows(vec![row(&[("count", Value::Int(0))])]));
    fake.say(Reply::Exec(0));
    fake.say(Reply::Insert(1, 9));

    let up = block_on(users.upsert_one(
        &json(r#"{"_id":1}"#),
        &json(r#"{"$set":{"name":"Ada"}}"#),
        json(r#"{"name":"Ada"}"#),
    ))
    .unwrap();

    assert!(up.upserted);
    assert_eq!(up.inserted_id, Some(Value::Int(9)));
}

#[test]
fn an_auto_id_is_stripped_so_the_sequence_applies() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Insert(1, 5));

    block_on(users.insert_one(json(r#"{"_id":null,"name":"Ada"}"#))).unwrap();

    let sql = statements(&fake)
        .into_iter()
        .find(|s| s.starts_with("INSERT"))
        .unwrap();
    assert!(!sql.contains("_id"), "{sql}");
}

#[test]
fn a_mongo_id_is_24_hex_characters() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Sqlite,
        "users",
        CollectionOptions {
            id_strategy: IdStrategy::Mongo,
            ..CollectionOptions::default()
        },
    );
    fake.say(Reply::Insert(1, 0));

    let id = block_on(users.insert_one(json(r#"{"name":"Ada"}"#))).unwrap();
    let id = id.as_str().expect("a string id");

    assert_eq!(id.len(), 24, "{id}");
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
}

#[test]
fn a_custom_generator_supplies_the_id() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Sqlite,
        "users",
        CollectionOptions {
            id_strategy: IdStrategy::Custom(std::sync::Arc::new(|| Value::str("custom-1"))),
            ..CollectionOptions::default()
        },
    );
    fake.say(Reply::Insert(1, 0));

    let id = block_on(users.insert_one(json(r#"{"name":"Ada"}"#))).unwrap();
    assert_eq!(id, Value::str("custom-1"));
}

#[test]
fn hidden_fields_never_leave_the_collection() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Sqlite,
        "users",
        CollectionOptions {
            schema: json(r#"{"secret":{"type":"TEXT","hidden":true}}"#),
            ..CollectionOptions::default()
        },
    );
    fake.say(Reply::Rows(vec![row(&[
        ("name", Value::str("Ada")),
        ("secret", Value::str("hunter2")),
    ])]));

    let rows = block_on(users.find(&Value::object(), None, FindOptions::default())).unwrap();
    assert!(rows[0].get("secret").is_none(), "{rows:?}");
    assert_eq!(rows[0].get("name"), Some(&Value::str("Ada")));
}

#[test]
fn schema_defaults_are_applied_to_inserts() {
    let fake = FakeDb::shared();
    let users = collection_on(
        &fake,
        Db::Sqlite,
        "users",
        CollectionOptions {
            schema: json(r#"{"role":{"type":"TEXT","default":"user"}}"#),
            ..CollectionOptions::default()
        },
    );
    fake.say(Reply::Insert(1, 1));

    block_on(users.insert_one(json(r#"{"name":"Ada"}"#))).unwrap();

    let sql = statements(&fake)
        .into_iter()
        .find(|s| s.starts_with("INSERT"))
        .unwrap();
    assert!(sql.contains("'user'"), "{sql}");
}

#[test]
fn an_index_is_created_per_dialect() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    block_on(users.create_index("email", true, None)).unwrap();
    assert_eq!(
        statements(&fake).pop().unwrap(),
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users  (email)"
    );

    let fake = FakeDb::shared();
    let users = collection_on(&fake, Db::Pg, "users", CollectionOptions::default());
    block_on(users.create_index("data", false, Some("gin"))).unwrap();
    assert_eq!(
        statements(&fake).pop().unwrap(),
        "CREATE  INDEX IF NOT EXISTS idx_users_data ON users USING GIN (data)"
    );

    let fake = FakeDb::shared();
    let users = collection_on(&fake, Db::Mysql, "users", CollectionOptions::default());
    block_on(users.create_index("email", true, None)).unwrap();
    let log = statements(&fake);
    assert_eq!(
        log[0],
        "SHOW INDEX FROM users WHERE Key_name = 'idx_users_email'"
    );
    // `qi` only quotes identifiers that collide with a reserved word, so these
    // stay bare.
    assert_eq!(
        log[1],
        "CREATE UNIQUE INDEX idx_users_email ON users (email)"
    );
}

#[test]
fn sqlite_cannot_drop_a_column() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");

    let err = block_on(users.drop_column("age")).expect_err("sqlite cannot");
    assert_eq!(err.to_string(), "SQLite does not support DROP COLUMN");
}

#[test]
fn sqlite_never_widens_a_column() {
    let fake = FakeDb::shared();
    let users = sqlite(&fake, "users");
    fake.say(Reply::Insert(1, 1));
    block_on(users.insert_one(json(r#"{"score":1}"#))).unwrap();

    fake.say(Reply::Insert(1, 2));
    block_on(users.insert_one(json(r#"{"score":1.5}"#))).unwrap();

    let alters: Vec<String> = statements(&fake)
        .into_iter()
        .filter(|s| s.contains("MODIFY COLUMN") || s.contains("ALTER COLUMN"))
        .collect();
    assert!(alters.is_empty(), "{alters:?}");
}

#[test]
fn postgres_widens_a_column_that_needs_it() {
    let fake = FakeDb::shared();
    let users = collection_on(&fake, Db::Pg, "users", CollectionOptions::default());
    fake.say(Reply::Insert(1, 1));
    block_on(users.insert_one(json(r#"{"score":1}"#))).unwrap();

    fake.say(Reply::Insert(1, 2));
    block_on(users.insert_one(json(r#"{"score":1.5}"#))).unwrap();

    let widens: Vec<String> = statements(&fake)
        .into_iter()
        .filter(|s| s.contains("ALTER COLUMN") && s.contains("TYPE"))
        .collect();
    assert_eq!(
        widens,
        vec!["ALTER TABLE users ALTER COLUMN score TYPE DECIMAL(20,6)"]
    );
}

#[test]
fn widen_type_prefers_the_wider_type_and_ties_go_to_the_new_one() {
    assert_eq!(widen_type("VARCHAR(255)", "TEXT"), "TEXT");
    assert_eq!(widen_type("TEXT", "VARCHAR(255)"), "TEXT");
    assert_eq!(widen_type("INTEGER", "DECIMAL(20,6)"), "DECIMAL(20,6)");
    assert_eq!(widen_type("DECIMAL(20,6)", "INTEGER"), "DECIMAL(20,6)");
    assert_eq!(widen_type("INT", "INT"), "INT");
    assert_eq!(widen_type("BIGINT", "INTEGER"), "BIGINT");
}
