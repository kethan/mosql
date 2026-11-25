//! The same app, in every engine.
//!
//! An app's rows live in whichever engine its document names: a local
//! SQLite file, memory, a serverless PostgreSQL, a serverless MySQL, a
//! Turso, a D1. The rows are documents and the filters are `mosql`'s
//! matcher in every one of them, so the operations below are the same
//! operations and the answers are the same answers.
//!
//! The serverless engines are driven through a mock transport, so the
//! wire protocol is checked without a network.

use std::sync::{Arc, Mutex};

use mosql_core::driver::{AnyServerlessDriver, DbError, HttpTransport};
use nanobase::db::memory::MemoryDb;
use nanobase::db::provider::ProviderDb;
use nanobase::db::AppDb;
use nanobase::service::Service;
use nanobase::store_serverless::{ServerlessStore, COLLECTION};
use nanobase::{AppDoc, CloneData, CloneRequest, DbKind};

/// A transport that answers a statement with rows.
///
/// A statement that reads gets the rows; a statement that writes gets
/// nothing back. The statements themselves are kept, so a test can see
/// what the engine asked for.
struct Rows {
    rows: String,
    seen: Mutex<Vec<String>>,
}

impl Rows {
    fn new(rows: &str) -> Arc<Rows> {
        Arc::new(Rows {
            rows: rows.to_string(),
            seen: Mutex::new(Vec::new()),
        })
    }

    /// The statements the engine asked for.
    fn statements(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl HttpTransport for Rows {
    async fn post_json(
        &self,
        _url: &str,
        _headers: &[(&str, &str)],
        body: String,
    ) -> Result<(u16, String), DbError> {
        let parsed = mosql::parse_json(&body).map_err(|e| DbError::new(e.to_string()))?;
        let query = parsed
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        self.seen.lock().unwrap().push(query.clone());
        if query.to_ascii_uppercase().starts_with("SELECT") {
            Ok((
                200,
                format!(
                    r#"[{{"fields":[{{"name":"data","dataTypeID":25}}],"rows":{},"rowCount":2}}]"#,
                    self.rows
                ),
            ))
        } else {
            Ok((200, r#"[{"fields":[],"rows":[]}]"#.to_string()))
        }
    }
}

/// The rows a serverless engine holds.
const TWO_ROWS: &str = r#"[["{\"id\":\"u1\",\"title\":\"write the tests\",\"done\":false}"],["{\"id\":\"u2\",\"title\":\"ship it\",\"done\":true}"]]"#;

/// A Neon engine over a mock.
fn neon(rows: &str) -> (ProviderDb<Arc<Rows>>, Arc<Rows>) {
    let transport = Rows::new(rows);
    let driver = AnyServerlessDriver::from_url(
        "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb",
        Arc::clone(&transport),
    )
    .unwrap();
    (ProviderDb::with_driver(driver, "neon"), transport)
}

/// The answers every engine gives to the same questions.
///
/// The same rows go in and the same filter comes out, so the answers are
/// the same in every engine: one row, the one that is done.
fn answers(db: &dyn AppDb) -> Vec<String> {
    let mut row = mosql::Object::new();
    row.set("title", mosql::Value::str("write the tests"));
    row.set("done", mosql::Value::Bool(false));
    db.insert("todos", mosql::Value::Object(row)).unwrap();

    let mut row = mosql::Object::new();
    row.set("title", mosql::Value::str("ship it"));
    row.set("done", mosql::Value::Bool(true));
    db.insert("todos", mosql::Value::Object(row)).unwrap();

    let mut done = mosql::Object::new();
    done.set("done", mosql::Value::Bool(true));
    let query = mosql::Value::Object(done);

    let found = db.find("todos", &query, None, None).unwrap();
    found
        .iter()
        .map(|row| {
            row.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn the_memory_engine_and_the_postgres_engine_agree() {
    let memory = MemoryDb::new();
    let (postgres, _transport) = neon(TWO_ROWS);
    assert_eq!(answers(&memory), vec!["ship it".to_string()]);
    assert_eq!(answers(&postgres), vec!["ship it".to_string()]);
}

#[cfg(feature = "sqlite")]
#[test]
fn the_sqlite_engine_agrees_too() {
    let dir = std::env::temp_dir().join(format!("nanobase-engines-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let sqlite = nanobase::db::sqlite::SqliteDb::open(dir.join("app.db")).unwrap();
    assert_eq!(answers(&sqlite), vec!["ship it".to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_serverless_engine_stores_the_row_as_a_document() {
    let (postgres, transport) = neon(TWO_ROWS);
    let mut row = mosql::Object::new();
    row.set("title", mosql::Value::str("hello"));
    postgres.insert("posts", mosql::Value::Object(row)).unwrap();
    let statements = transport.statements();
    let insert = statements
        .iter()
        .find(|s| s.starts_with("INSERT"))
        .expect("the row went in");
    assert!(
        insert.contains("\"id\"") && insert.contains("\"data\""),
        "the row is a document in a table: {insert}"
    );
}

#[test]
fn the_platform_catalog_lives_in_a_serverless_postgres() {
    // The owner's choice: the platform's own state in a serverless
    // PostgreSQL, with no local file and no service in front of it.
    let transport = Rows::new("[]");
    let driver = AnyServerlessDriver::from_url(
        "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/platform",
        Arc::clone(&transport),
    )
    .unwrap();
    let db = ProviderDb::with_driver(driver, "neon");
    let store = ServerlessStore::new(Arc::new(db), "postgres://platform");
    let service = Service::new("apps", Arc::new(store)).unwrap();
    service
        .create(AppDoc::empty("todos"))
        .expect("the catalog accepts an app");
    // The catalog was written to the serverless database: the mock saw
    // the statements.
    let statements = transport.statements();
    assert!(
        statements.iter().any(|s| s.contains(COLLECTION)),
        "the platform row went to the serverless database: {statements:?}"
    );
}

#[test]
fn a_clone_of_an_app_in_a_serverless_database_has_no_database_of_its_own() {
    // The safety rule: a clone never quietly shares the source's rows.
    let mut doc = AppDoc::empty("todos");
    doc.config.database.kind = DbKind::Postgres;
    doc.config.database.raw_type = "postgres".to_string();
    doc.config.database.url = Some("postgres://user:pass@host/production".to_string());
    doc.config.database.token = Some("production-secret".to_string());

    let store = nanobase::platform::MemoryStore::default();
    let service = Service::new("apps", Arc::new(store)).unwrap();
    let created = service.create(doc).unwrap();
    let id = created
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let summary = service
        .clone_app(&id, &CloneRequest::named("todos-staging"))
        .unwrap();
    assert!(!summary.shares_database);
    assert!(!summary.has_database, "the clone has no database yet");
    assert_eq!(summary.rows_copied, 0);

    // And the clone's document says so.
    let clone = service.find("todos-staging").unwrap().unwrap();
    let database = clone
        .get("doc")
        .and_then(|d| d.get("config"))
        .and_then(|c| c.get("database"))
        .unwrap();
    assert_eq!(
        database.get("type").and_then(|v| v.as_str()),
        Some("none"),
        "the clone has no database"
    );
    assert!(
        database.get("url").is_none(),
        "the clone does not inherit the source's URL"
    );
}

#[test]
fn a_clone_that_shares_the_database_says_so() {
    let mut doc = AppDoc::empty("todos");
    doc.config.database.kind = DbKind::Postgres;
    doc.config.database.raw_type = "postgres".to_string();
    doc.config.database.url = Some("postgres://user:pass@host/production".to_string());

    let store = nanobase::platform::MemoryStore::default();
    let service = Service::new("apps", Arc::new(store)).unwrap();
    let created = service.create(doc).unwrap();
    let id = created
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let request = CloneRequest::named("todos-demo").with_data(CloneData::Share);
    let summary = service.clone_app(&id, &request).unwrap();
    assert!(summary.shares_database);
    assert!(summary.has_database);
}

#[test]
fn copying_the_rows_of_a_serverless_app_needs_a_destination() {
    let mut doc = AppDoc::empty("todos");
    doc.config.database.kind = DbKind::Postgres;
    doc.config.database.raw_type = "postgres".to_string();
    doc.config.database.url = Some("postgres://user:pass@host/production".to_string());

    let store = nanobase::platform::MemoryStore::default();
    let service = Service::new("apps", Arc::new(store)).unwrap();
    let created = service.create(doc).unwrap();
    let id = created
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let request = CloneRequest::named("todos-staging").with_data(CloneData::Copy);
    let err = service.clone_app(&id, &request).unwrap_err();
    assert!(
        err.to_string().contains("needs a database"),
        "the clone is refused rather than pointed at the source: {err}"
    );
}
