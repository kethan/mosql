//! The platform store, on a real SQLite file.
//!
//! `platform.rs` tests the invariants with an in-memory store; these prove
//! the same invariants hold when the state is in SQLite on disk, and that an
//! install kept in the old JSON file is carried across.

use std::path::{Path, PathBuf};

use nanobase::platform::{create_owner_with, now_secs, FileStore, Platform, PlatformStore};
use nanobase::AppDoc;
use nanobase::SqliteStore;
use uflite::db::block_on;

const NOW: i64 = 1_700_000_000;
const FAST: u32 = 1000;

/// A temporary directory that removes itself, so a failing test does not
/// leave state behind for the next run.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nanobase-store-{}-{}",
            std::process::id(),
            nanobase::crypto::random_id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn signed_up() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", NOW, FAST).unwrap();
    p
}

fn save(store: &SqliteStore, platform: &Platform) {
    block_on(store.save(platform)).expect("saved");
}

fn load(store: &SqliteStore) -> Platform {
    block_on(store.load()).expect("loaded")
}

// ------------------------------------------------------------- the file ---

#[test]
fn a_fresh_store_creates_a_sqlite_file_in_a_new_directory() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("nested/deeper/platform.db"));
    assert!(!store.path().exists());
    let platform = load(&store);
    assert!(platform.needs_setup());
    assert!(store.path().is_file());

    let mut header = [0u8; 16];
    use std::io::Read as _;
    std::fs::File::open(store.path())
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    assert_eq!(&header, b"SQLite format 3\0", "it must be a SQLite file");
}

#[test]
fn the_root_is_the_database_s_directory() {
    let store = SqliteStore::new(PathBuf::from("/srv/nanobase/platform.db"));
    assert_eq!(store.root(), PathBuf::from("/srv/nanobase"));
}

#[test]
fn a_file_that_is_not_a_database_is_reported_not_panicked() {
    let dir = TempDir::new();
    let path = dir.path().join("platform.db");
    std::fs::write(&path, b"this is not a database").unwrap();
    let store = SqliteStore::new(path);
    let err = block_on(store.load()).expect_err("garbage must not load");
    assert_eq!(err.status(), 500);
}

// ------------------------------------------------------------ round trip ---

#[test]
fn a_saved_platform_loads_back_identically() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));

    let mut p = signed_up();
    p.create_app(AppDoc::empty("todos"), NOW).unwrap();
    p.create_app(AppDoc::empty("notes"), NOW + 10).unwrap();
    save(&store, &p);

    let back = load(&store);
    assert_eq!(back.jwt_secret, p.jwt_secret);
    // The store lists apps by name; creation order is not part of the state.
    let mut saved = p.apps.clone();
    saved.sort_by(|a, b| a.doc.app.name.cmp(&b.doc.app.name));
    assert_eq!(
        back.apps, saved,
        "ids, timestamps and documents must survive"
    );
    assert_eq!(back.app_names(), vec!["notes", "todos"]);
    assert_eq!(back.users.len(), 1);
    assert_eq!(back.users[0].email, "ada@example.com");
    assert_eq!(back.users[0].created_at, p.users[0].created_at);
    assert!(back.users[0].password.verify("correct horse"));
    assert!(!back.users[0].password.verify("wrong horse"));
}

#[test]
fn apps_are_listed_by_name_regardless_of_creation_order() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let mut p = signed_up();
    p.create_app(AppDoc::empty("zeta"), NOW).unwrap();
    p.create_app(AppDoc::empty("Alpha"), NOW).unwrap();
    p.create_app(AppDoc::empty("mid"), NOW).unwrap();
    save(&store, &p);
    assert_eq!(load(&store).app_names(), vec!["Alpha", "mid", "zeta"]);
}

#[test]
fn a_second_store_on_the_same_file_sees_the_saved_state() {
    // The file is the source of truth, not whatever a process has cached.
    let dir = TempDir::new();
    let path = dir.path().join("platform.db");
    let mut p = signed_up();
    p.create_app(AppDoc::empty("todos"), NOW).unwrap();
    save(&SqliteStore::new(&path), &p);

    let other = SqliteStore::new(&path);
    assert_eq!(load(&other).app_names(), vec!["todos"]);
}

#[test]
fn a_change_is_visible_after_reload() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let mut p = signed_up();
    let id = p
        .create_app(AppDoc::empty("todos"), NOW)
        .unwrap()
        .id
        .clone();
    p.create_app(AppDoc::empty("notes"), NOW).unwrap();
    save(&store, &p);

    let mut p = load(&store);
    p.rename_app(&id, "todo-list").unwrap();
    let notes_id = p.find_app("notes").unwrap().id.clone();
    p.remove_app("notes", &notes_id).unwrap();
    save(&store, &p);

    let back = load(&store);
    assert_eq!(back.app_names(), vec!["todo-list"]);
    assert!(back.app_by_name("todos").is_none());
    assert_eq!(
        back.app(&id).unwrap().created_at,
        p.app(&id).unwrap().created_at
    );
}

#[test]
fn saving_twice_does_not_duplicate_anything() {
    // The store rewrites each table on save; a bug there would double the
    // owner or the apps on every write.
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let mut p = signed_up();
    p.create_app(AppDoc::empty("todos"), NOW).unwrap();
    save(&store, &p);
    save(&store, &p);
    let back = load(&store);
    assert_eq!(back.users.len(), 1);
    assert_eq!(back.apps.len(), 1);
}

#[test]
fn a_document_round_trips_byte_for_byte() {
    // The app document is stored as its JSON text; whatever the user wrote —
    // column types, storage paths, ACL rules — must come back unchanged.
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let doc = AppDoc::parse(
        r#"{
            "meta": {"nanobaseVersion": "0.1.0"},
            "app": {"name": "notes", "type": "remote"},
            "config": {
                "auth": {"database": {"type": "memory"}, "secret": "0123456789abcdef0123456789abcdef"},
                "database": {"type": "sqlite"},
                "storage": {"type": "local", "local": {"basePath": "./data/files", "publicUrl": "https://cdn.example.com"}},
                "acl": {"enabled": true, "rules": [{"role": "member", "collection": "notes", "read": true}]}
            },
            "schema": {"notes": {"columns": {
                "id": {"type": "integer", "primary": true, "autoIncrement": true},
                "title": {"type": "text", "notNull": true},
                "done": {"type": "boolean"},
                "tags": {"type": "json"}
            }}}
        }"#,
    )
    .unwrap();
    let mut p = signed_up();
    p.create_app(doc, NOW).unwrap();
    save(&store, &p);

    let back = load(&store);
    assert_eq!(
        back.apps[0].doc.to_json(),
        p.apps[0].doc.to_json(),
        "the stored document must be the one that was saved"
    );
    assert!(back.apps[0].doc.config.acl.enabled);
}

#[test]
fn a_token_issued_before_saving_still_verifies_after_loading() {
    // The signing key lives in the `meta` table; if it were ever regenerated
    // on load, every session would be invalidated on restart.
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let p = signed_up();
    save(&store, &p);
    let token = nanobase::platform::issue_token(&p.users[0], p.jwt_key(), now_secs(), 60);

    let back = load(&store);
    let claims = nanobase::jwt::decode(&token, back.jwt_key(), now_secs()).expect("still valid");
    assert_eq!(claims.sub, p.users[0].id);
}

// ------------------------------------------------------------- migration ---

/// Write a platform in the JSON format the first release used, exactly where
/// the database now lives (same stem, `.json`).
fn legacy_json(dir: &TempDir, platform: &Platform) -> PathBuf {
    let path = dir.path().join("platform.json");
    block_on(FileStore::new(&path).save(platform)).unwrap();
    path
}

#[test]
fn a_json_platform_is_imported_once_and_the_file_retired() {
    let dir = TempDir::new();
    let mut old = signed_up();
    old.create_app(AppDoc::empty("notes"), NOW).unwrap();
    let json = legacy_json(&dir, &old);
    let store = SqliteStore::new(dir.path().join("platform.db"));

    assert!(store.migrate_from_json().unwrap(), "first run imports");
    let back = load(&store);
    assert_eq!(back.users[0].email, "ada@example.com");
    assert!(back.users[0].password.verify("correct horse"));
    assert_eq!(back.app_names(), vec!["notes"]);
    assert_eq!(
        back.apps[0].id, old.apps[0].id,
        "ids must be kept: tokens name them"
    );

    assert!(!json.exists(), "the old file must not be read again");
    assert!(dir.path().join("platform.json.migrated").is_file());

    assert!(!store.migrate_from_json().unwrap(), "second run is a no-op");
}

#[test]
fn without_a_json_file_nothing_is_migrated() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    assert!(!store.migrate_from_json().unwrap());
    assert!(load(&store).needs_setup());
}

#[test]
fn a_json_file_never_overwrites_a_database_in_use() {
    let dir = TempDir::new();
    let store = SqliteStore::new(dir.path().join("platform.db"));
    let current = signed_up();
    save(&store, &current);

    let mut stale = Platform::default();
    stale.ensure_jwt_secret();
    create_owner_with(
        &mut stale,
        "someone-else@example.com",
        "correct horse",
        NOW,
        FAST,
    )
    .unwrap();
    stale.create_app(AppDoc::empty("ghost"), NOW).unwrap();
    let json = legacy_json(&dir, &stale);

    assert!(!store.migrate_from_json().unwrap());
    let back = load(&store);
    assert_eq!(
        back.users[0].email, "ada@example.com",
        "the live owner must win"
    );
    assert!(back.app_by_name("ghost").is_none());
    assert!(
        json.exists(),
        "and the old file is left for the user to look at"
    );
}

#[test]
fn a_corrupt_json_file_is_reported_and_left_in_place() {
    let dir = TempDir::new();
    let json = dir.path().join("platform.json");
    std::fs::write(&json, b"{ not json").unwrap();
    let store = SqliteStore::new(dir.path().join("platform.db"));

    assert!(store.migrate_from_json().is_err(), "the user must be told");
    assert!(
        json.exists(),
        "a bad file is not deleted, so nothing is lost"
    );
}
