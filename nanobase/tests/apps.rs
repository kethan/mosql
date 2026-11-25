//! Creating, renaming, cloning, exporting and deleting apps.

// The serverless edition has no file store, no local database and no
// listener, so this test belongs to the full edition only.
#![cfg(feature = "native")]

use nanobase::platform::{create_owner_with, iso8601, FileStore, Platform, PlatformStore};
use nanobase::{AppDoc, CloneRequest, DbKind, Secrets};
use uflite::db::block_on;

const NOW: i64 = 1_700_000_000;
const FAST: u32 = 1000;

fn signed_up() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", NOW, FAST).unwrap();
    p
}

fn todos() -> AppDoc {
    AppDoc::empty("todos")
}

// -------------------------------------------------------------- creating ---

#[test]
fn an_app_is_created_with_an_id_and_a_timestamp() {
    let mut p = signed_up();
    let app = p.create_app(todos(), NOW).expect("created");
    assert!(app.id.starts_with("app_"), "{}", app.id);
    assert_eq!(app.id.len(), "app_".len() + 24);
    assert_eq!(app.created_at, iso8601(NOW));
    assert_eq!(app.doc.app.name, "todos");
}

#[test]
fn two_apps_get_different_ids() {
    let mut p = signed_up();
    let a = p.create_app(AppDoc::empty("one"), NOW).unwrap().id.clone();
    let b = p.create_app(AppDoc::empty("two"), NOW).unwrap().id.clone();
    assert_ne!(a, b);
}

#[test]
fn two_apps_cannot_share_a_name() {
    let mut p = signed_up();
    p.create_app(todos(), NOW).unwrap();
    let err = p
        .create_app(AppDoc::empty("todos"), NOW)
        .expect_err("a duplicate name must be refused");
    assert_eq!(err.status(), 409);
    assert_eq!(p.apps.len(), 1);
}

#[test]
fn names_are_compared_case_insensitively() {
    let mut p = signed_up();
    p.create_app(todos(), NOW).unwrap();
    assert!(p.create_app(AppDoc::empty("Todos"), NOW).is_err());
    assert!(p.create_app(AppDoc::empty("  TODOS  "), NOW).is_err());
}

#[test]
fn a_name_can_be_reused_after_the_app_is_deleted() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    p.remove_app(&id, &id).unwrap();
    assert!(p.create_app(todos(), NOW).is_ok());
}

#[test]
fn an_invalid_document_creates_no_app() {
    let mut p = signed_up();
    let mut doc = todos();
    doc.config.database.kind = DbKind::Postgres;
    doc.config.database.raw_type = "neon".to_string();
    doc.config.database.url = None; // a remote database with no URL
    assert!(p.create_app(doc, NOW).is_err());
    assert!(
        p.apps.is_empty(),
        "a failed create must not leave a half-app"
    );
}

#[test]
fn a_bad_name_creates_no_app() {
    let mut p = signed_up();
    let mut doc = todos();
    doc.app.name = "not a valid name!".to_string();
    assert!(p.create_app(doc, NOW).is_err());
    assert!(p.apps.is_empty());
}

// ---------------------------------------------------------------- finding ---

#[test]
fn an_app_is_found_by_id_and_by_name() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    assert_eq!(p.find_app(&id).map(|a| a.id.as_str()), Some(id.as_str()));
    assert_eq!(
        p.find_app("todos").map(|a| a.id.as_str()),
        Some(id.as_str())
    );
    assert_eq!(
        p.find_app("TODOS").map(|a| a.id.as_str()),
        Some(id.as_str())
    );
    assert!(p.find_app("nope").is_none());
}

#[test]
fn an_id_wins_over_a_matching_name() {
    // If an app is literally named after another app's id, the id still wins.
    let mut p = signed_up();
    let first = p.create_app(AppDoc::empty("one"), NOW).unwrap().id.clone();
    let second = p.create_app(AppDoc::empty(&first), NOW).unwrap().id.clone();
    assert_ne!(first, second);
    assert_eq!(
        p.find_app(&first).map(|a| a.id.as_str()),
        Some(first.as_str())
    );
}

#[test]
fn app_names_are_sorted_for_listing() {
    let mut p = signed_up();
    for name in ["zeta", "Alpha", "beta"] {
        p.create_app(AppDoc::empty(name), NOW).unwrap();
    }
    assert_eq!(p.app_names(), vec!["Alpha", "beta", "zeta"]);
}

// --------------------------------------------------------------- renaming ---

#[test]
fn renaming_keeps_the_id() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    let renamed = p.rename_app(&id, "todo-list").unwrap();
    assert_eq!(renamed.id, id);
    assert_eq!(renamed.doc.app.name, "todo-list");
    // The old name no longer finds it.
    assert!(p.find_app("todos").is_none());
    assert!(p.find_app("todo-list").is_some());
}

#[test]
fn renaming_to_the_same_name_is_allowed() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    assert!(p.rename_app(&id, "todos").is_ok());
}

#[test]
fn renaming_onto_another_apps_name_is_refused() {
    let mut p = signed_up();
    p.create_app(AppDoc::empty("one"), NOW).unwrap();
    let two = p.create_app(AppDoc::empty("two"), NOW).unwrap().id.clone();
    let err = p.rename_app(&two, "one").unwrap_err();
    assert_eq!(err.status(), 409);
}

#[test]
fn renaming_to_an_invalid_name_is_refused() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    assert!(p.rename_app(&id, "no spaces allowed!").is_err());
    assert!(p.rename_app(&id, "").is_err());
    // And the app is untouched.
    assert_eq!(p.find_app(&id).unwrap().doc.app.name, "todos");
}

#[test]
fn renaming_an_unknown_app_is_a_404() {
    let mut p = signed_up();
    let err = p.rename_app("nope", "x").unwrap_err();
    assert_eq!(err.status(), 404);
}

// ---------------------------------------------------------------- cloning ---

#[test]
fn cloning_copies_the_shape_but_not_the_rows() {
    let mut p = signed_up();
    let mut doc = todos();
    doc.schema
        .insert("notes".to_string(), nanobase::CollectionDef::default());
    doc.data
        .insert("notes".to_string(), vec![mosql::Value::object()]);
    doc.meta.includes_data = true;
    let id = p.create_app(doc, NOW).unwrap().id.clone();

    let clone = p
        .clone_app(&id, &CloneRequest::named("todos-staging"), NOW)
        .unwrap()
        .0;
    assert_ne!(clone.id, id);
    assert_eq!(clone.doc.app.name, "todos-staging");
    assert!(clone.doc.schema.contains_key("notes"), "schema is copied");
    assert!(clone.doc.data.is_empty(), "rows are not copied");
    assert!(!clone.doc.meta.includes_data);
    // The original still has its rows.
    assert_eq!(p.find_app(&id).unwrap().doc.data.len(), 1);
}

#[test]
fn cloning_copies_the_document_as_written() {
    // The clone's *document* is the source's document: the same schema,
    // the same rules, the same auth settings — and the same signing
    // secret, because the secret is in the document and the document is
    // what is copied.
    //
    // The database is the exception: it is chosen by the clone's own
    // policy (omit, copy or share), and a clone that does not share the
    // source's database does not inherit its database token.
    //
    // Whether a clone should have a secret *of its own* is a question
    // about what a clone is for, and it is not answered here: a clone
    // that shares the source's database is the same app under a second
    // name, and a clone that does not is a new app — and the two may
    // want different answers.
    let mut p = signed_up();
    let mut doc = todos();
    doc.config.auth.secret = Some("0123456789abcdef".to_string());
    doc.config.auth.features.email_password = true;
    let id = p.create_app(doc, NOW).unwrap().id.clone();
    let clone = p
        .clone_app(&id, &CloneRequest::named("todos-staging"), NOW)
        .unwrap()
        .0;
    assert_eq!(
        clone.doc.config.auth.secret.as_deref(),
        Some("0123456789abcdef"),
        "the document is copied as written; secrets are not synthesised"
    );
}

#[test]
fn cloning_cannot_collide_with_an_existing_name() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    p.create_app(AppDoc::empty("taken"), NOW).unwrap();
    assert_eq!(
        p.clone_app(&id, &CloneRequest::named("taken"), NOW)
            .unwrap_err()
            .status(),
        409
    );
}

// --------------------------------------------------------------- replacing ---

#[test]
fn replacing_an_app_keeps_its_id_and_creation_time() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    let mut doc = AppDoc::empty("todos");
    doc.config.acl.enabled = true;
    let replaced = p.replace_app(&id, doc).unwrap();
    assert_eq!(replaced.id, id);
    assert_eq!(replaced.created_at, iso8601(NOW));
    assert!(replaced.doc.config.acl.enabled);
    assert_eq!(p.apps.len(), 1, "a replace must not add an app");
}

#[test]
fn replacing_with_someone_elses_name_is_refused() {
    let mut p = signed_up();
    let id = p.create_app(AppDoc::empty("one"), NOW).unwrap().id.clone();
    p.create_app(AppDoc::empty("two"), NOW).unwrap();
    assert_eq!(
        p.replace_app(&id, AppDoc::empty("two"))
            .unwrap_err()
            .status(),
        409
    );
}

#[test]
fn replacing_with_an_invalid_document_is_refused() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    let mut doc = AppDoc::empty("todos");
    doc.config.database.kind = DbKind::Libsql;
    doc.config.database.raw_type = "turso".to_string();
    doc.config.database.url = None;
    assert!(p.replace_app(&id, doc).is_err());
    assert_eq!(
        p.find_app(&id).unwrap().doc.config.database.kind,
        DbKind::Sqlite
    );
}

// --------------------------------------------------------------- deleting ---

#[test]
fn deleting_requires_the_id_as_confirmation() {
    let mut p = signed_up();
    let id = p.create_app(todos(), NOW).unwrap().id.clone();
    // No confirmation.
    assert_eq!(p.remove_app(&id, "").unwrap_err().status(), 400);
    // The name is not confirmation.
    assert_eq!(p.remove_app(&id, "todos").unwrap_err().status(), 400);
    assert_eq!(p.apps.len(), 1, "nothing was deleted");

    let removed = p.remove_app(&id, &id).expect("deleted");
    assert_eq!(removed.id, id);
    assert!(p.apps.is_empty());
}

#[test]
fn deleting_an_unknown_app_is_a_404() {
    let mut p = signed_up();
    assert_eq!(p.remove_app("nope", "nope").unwrap_err().status(), 404);
}

// ------------------------------------------------------------------ store ---

#[test]
fn apps_survive_a_save_and_load() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("platform.json"));
    let mut p = signed_up();
    p.create_app(todos(), NOW).unwrap();
    p.create_app(AppDoc::empty("notes"), NOW).unwrap();
    block_on(store.save(&p)).unwrap();

    let loaded = block_on(store.load()).unwrap();
    assert_eq!(loaded.apps.len(), 2);
    assert_eq!(loaded.app_names(), vec!["notes", "todos"]);
    assert_eq!(
        loaded.find_app("todos").map(|a| a.doc.config.database.kind),
        Some(DbKind::Sqlite)
    );
}

#[test]
fn an_apps_data_directory_is_its_own() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("platform.json"));
    let mut p = signed_up();
    let a = p.create_app(AppDoc::empty("one"), NOW).unwrap().id.clone();
    let b = p.create_app(AppDoc::empty("two"), NOW).unwrap().id.clone();
    let root = store.root();

    let da = p.find_app(&a).unwrap().data_dir(&root);
    let db = p.find_app(&b).unwrap().data_dir(&root);
    assert_ne!(da, db, "each app gets its own directory");
    assert!(da.ends_with(PathBuf::from("apps").join(&a)));
    assert_eq!(
        p.find_app(&a).unwrap().sqlite_path(&root),
        da.join("app.db")
    );
}

#[test]
fn importing_through_the_store_overlays_secrets() {
    // A document that leaves its database URL to the environment is complete
    // once loaded, and must be storable as-is.
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("platform.json"));
    let mut p = signed_up();

    let text = r#"{
      "meta": { "nanobaseVersion": "1.0.0" },
      "app": { "name": "remote", "type": "remote" },
      "config": { "database": { "type": "neon" }, "storage": { "type": "local" } },
      "schema": {}, "data": {}
    }"#;
    let secrets = Secrets::from_pairs([("SECRET_DATA_DB_URL", "postgres://neon")]);
    let doc = AppDoc::load(text, &secrets).expect("complete");
    p.create_app(doc, NOW).expect("created");
    block_on(store.save(&p)).unwrap();

    let loaded = block_on(store.load()).unwrap();
    assert_eq!(
        loaded
            .find_app("remote")
            .unwrap()
            .doc
            .config
            .database
            .url
            .as_deref(),
        Some("postgres://neon")
    );
}

// ------------------------------------------------------------------- temp ---

/// A directory that does not collide with another test, and that is removed
/// afterwards. Named with a random id, never a timestamp: the clock has been
/// seen to repeat a microsecond on macOS.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("nanobase-apps-{}", nanobase::crypto::random_id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

use std::path::PathBuf;
