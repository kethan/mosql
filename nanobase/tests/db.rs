//! The app database: one set of operations, four engines.
//!
//! Every adapter is held to the same contract, because an app's document
//! chooses the engine and the code that serves the app does not change.

use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::db::http::HttpDb;
use nanobase::db::memory::MemoryDb;
use nanobase::db::sqlite::SqliteDb;
use nanobase::db::tcp::TcpDb;
use nanobase::db::{AppDb, DbOp, DbRequest, DbServer};
use nanobase::httpclient::{HttpClient, PlainHttpClient};

fn row(pairs: &[(&str, Value)]) -> Value {
    let mut o = Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

/// The operations every adapter has to get right.
fn behaves_like_a_database(db: &dyn AppDb) {
    // A row gets an id when it arrives without one.
    let inserted = db
        .insert(
            "todos",
            row(&[
                ("title", Value::str("write it down")),
                ("done", Value::Bool(false)),
            ]),
        )
        .expect("inserted");
    let id = inserted
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string();
    assert!(!id.is_empty());

    // The row is there.
    let found = db
        .find_one("todos", &row(&[("id", Value::str(id.clone()))]))
        .expect("found");
    assert!(found.is_some(), "the row is not there");

    // A filter means the same thing everywhere.
    db.insert(
        "todos",
        row(&[
            ("title", Value::str("done already")),
            ("done", Value::Bool(true)),
        ]),
    )
    .expect("inserted");
    let open = db
        .find("todos", &row(&[("done", Value::Bool(false))]), None, None)
        .expect("found");
    assert_eq!(open.len(), 1, "one row is not done");
    assert_eq!(
        open[0].get("title").and_then(|v| v.as_str()),
        Some("write it down")
    );

    // Counting and limiting.
    assert_eq!(db.count("todos", &row(&[])).expect("counted"), 2);
    let one = db.find("todos", &row(&[]), Some(1), None).expect("found");
    assert_eq!(one.len(), 1);
    let skipped = db.find("todos", &row(&[]), None, Some(1)).expect("found");
    assert_eq!(skipped.len(), 1);

    // A change is applied, and only to the row that matched.
    let changed = db
        .update(
            "todos",
            &row(&[("id", Value::str(id.clone()))]),
            &row(&[("done", Value::Bool(true))]),
        )
        .expect("updated")
        .expect("a row came back");
    assert_eq!(changed.get("done").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        db.count("todos", &row(&[("done", Value::Bool(true))]))
            .expect("counted"),
        2
    );

    // The collections are listed.
    let collections = db.collections().expect("listed");
    assert!(collections.iter().any(|c| c == "todos"), "{collections:?}");

    // A removal takes the matching rows and only those.
    let removed = db
        .remove("todos", &row(&[("id", Value::str(id.clone()))]))
        .expect("removed");
    assert_eq!(removed, 1);
    assert_eq!(db.count("todos", &row(&[])).expect("counted"), 1);
}

#[test]
fn the_memory_engine_behaves() {
    behaves_like_a_database(&MemoryDb::new());
}

#[test]
fn the_sqlite_engine_behaves() {
    let dir = std::env::temp_dir().join(format!("nanobase-db-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let db = SqliteDb::open(dir.join("app.db")).expect("opened");
    behaves_like_a_database(&db);
    // The rows really are in the file.
    let again = SqliteDb::open(dir.join("app.db")).expect("opened");
    assert_eq!(again.count("todos", &row(&[])).expect("counted"), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_http_engine_behaves() {
    let (address, server) = http_server();
    let client: Arc<dyn HttpClient> = Arc::new(PlainHttpClient);
    let db = HttpDb::new(format!("http://{address}"), client);
    behaves_like_a_database(&db);
    drop(server);
}

#[test]
fn the_tcp_engine_behaves() {
    let (address, server) = tcp_server();
    let db = TcpDb::new(address);
    behaves_like_a_database(&db);
    drop(server);
}

#[test]
fn an_engine_that_is_not_there_is_an_error_not_a_panic() {
    let db = TcpDb::new("127.0.0.1:1");
    let error = db.rows("todos").expect_err("no database");
    assert!(
        error.to_string().contains("cannot reach the database"),
        "{error}"
    );
}

#[test]
fn a_request_survives_the_wire() {
    let request = DbRequest {
        op: DbOp::Find,
        collection: "todos".into(),
        query: row(&[("done", Value::Bool(false))]),
        data: Value::Object(Object::new()),
        limit: Some(10),
        offset: Some(0),
    };
    let value = request.to_value();
    let back = DbRequest::from_value(&value).expect("parsed");
    assert_eq!(back.op, DbOp::Find);
    assert_eq!(back.collection, "todos");
    assert_eq!(back.limit, Some(10));
    assert_eq!(
        back.query.get("done").and_then(|v| v.as_bool()),
        Some(false)
    );
}

/// A server over a memory database, over HTTP.
fn http_server() -> (String, Arc<DbServer>) {
    let server = Arc::new(DbServer::new(Arc::new(MemoryDb::new())));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = Arc::clone(&server);
    std::thread::spawn(move || {
        let _ = nanobase::http::serve_on(listener, 2, move |req| worker.handle_http(&req));
    });
    (addr.to_string(), server)
}

/// A server over a memory database, over TCP.
fn tcp_server() -> (String, Arc<DbServer>) {
    let server = Arc::new(DbServer::new(Arc::new(MemoryDb::new())));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = Arc::clone(&server);
    std::thread::spawn(move || {
        let _ = nanobase::db::serve_tcp(worker, listener);
    });
    (addr.to_string(), server)
}
