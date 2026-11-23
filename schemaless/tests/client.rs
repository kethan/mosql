//! `createSchemalessClient` — the factory and its backends.

mod common;

use std::sync::Arc;

use mosql::{Db, Value};
use mosql_schemaless::{Client, Kind};
use uflite::db::{block_on, Collection};
use uflite::{DbOptions, DbService, Params};

use common::{json, row, FakeDb, Reply};

#[test]
fn memory_databases_do_not_share_documents() {
    let client = Client::memory();
    let a = client.db("a").collection("users");
    let b = client.db("b").collection("users");

    block_on(a.insert_one(json(r#"{"name":"Ada"}"#))).unwrap();

    assert_eq!(block_on(a.count_documents(&Value::object())).unwrap(), 1);
    assert_eq!(block_on(b.count_documents(&Value::object())).unwrap(), 0);
    assert_eq!(client.kind(), Kind::Memory);
}

#[test]
fn a_memory_collection_is_shared_by_database_and_name() {
    let client = Client::memory();
    block_on(
        client
            .db("a")
            .collection("users")
            .insert_one(json(r#"{"name":"Ada"}"#)),
    )
    .unwrap();

    let again = client.db("a").collection("users");
    assert_eq!(
        block_on(again.count_documents(&Value::object())).unwrap(),
        1
    );

    client.close();
    let after = client.db("a").collection("users");
    assert_eq!(
        block_on(after.count_documents(&Value::object())).unwrap(),
        0
    );
}

#[test]
fn a_database_reports_its_name() {
    let client = Client::memory();
    assert_eq!(client.db("app").name(), "app");
}

#[test]
fn the_kind_reports_the_dialect() {
    let fake = FakeDb::shared();
    assert_eq!(
        Client::with_executor(Db::Sqlite, Arc::clone(&fake)).kind(),
        Kind::Sqlite
    );
    assert_eq!(
        Client::with_executor(Db::Pg, Arc::clone(&fake)).kind(),
        Kind::Pg
    );
    assert_eq!(
        Client::with_executor(Db::Mysql, Arc::clone(&fake)).kind(),
        Kind::Mysql
    );
}

#[test]
fn the_service_layer_runs_unchanged_over_sql() {
    let fake = FakeDb::shared();
    let client = Client::with_executor(Db::Sqlite, Arc::clone(&fake));
    let service = DbService::new(client.db("app").collection("users"), DbOptions::default());

    fake.say(Reply::Rows(vec![row(&[
        ("_id", Value::Int(1)),
        ("name", Value::str("Ada")),
    ])]));

    let doc = block_on(service.get(&Value::Int(1), &Params::new())).unwrap();
    assert_eq!(doc.get("name"), Some(&Value::str("Ada")));

    let sql = fake
        .logged()
        .into_iter()
        .find(|s| s.starts_with("SELECT") && !s.contains("sqlite_master"))
        .unwrap_or_default();
    assert!(sql.contains("_id"), "{sql}");
    assert!(sql.contains("1"), "{sql}");
}

#[test]
fn a_missing_record_is_still_a_404_over_sql() {
    let fake = FakeDb::shared();
    let client = Client::with_executor(Db::Sqlite, Arc::clone(&fake));
    let service = DbService::new(client.db("app").collection("users"), DbOptions::default());

    fake.say(Reply::Rows(Vec::new()));

    let err = block_on(service.get(&Value::Int(1), &Params::new())).unwrap_err();
    assert_eq!(err.status(), 404);
    assert!(
        err.to_string().contains("No record found for id '1'"),
        "{err}"
    );
}
