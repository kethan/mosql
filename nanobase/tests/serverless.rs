//! The platform store, in a serverless database.
//!
//! The full edition keeps the platform in a SQLite file and the serverless
//! edition keeps it in a database reached over the network. The two stores
//! have one contract: the same platform goes in and the same platform comes
//! out, and a second write does not duplicate anything.
//!
//! The database here is the in-memory engine, which speaks the same
//! interface every other engine does — so these tests hold for a Turso
//! database and a D1 database exactly as they hold here.

#![cfg(feature = "serverless")]

use std::sync::Arc;

use nanobase::db::memory::MemoryDb;
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::store_serverless::COLLECTION;
use nanobase::ServerlessStore;
use uflite::db::block_on;

const NOW: i64 = 1_700_000_000;
const FAST: u32 = 1000;

/// A store over a fresh in-memory database.
fn store() -> ServerlessStore {
    ServerlessStore::new(Arc::new(MemoryDb::new()), "http://database.example")
}

/// A platform with an owner and a signing key.
fn signed_up() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", NOW, FAST).unwrap();
    p
}

#[test]
fn an_empty_database_holds_an_empty_platform() {
    let store = store();
    let platform = block_on(store.load()).expect("loaded");
    assert!(
        platform.users.is_empty(),
        "a fresh database has no accounts"
    );
    assert!(platform.apps.is_empty(), "a fresh database has no apps");
}

#[test]
fn a_platform_survives_a_round_trip() {
    let store = store();
    let platform = signed_up();
    block_on(store.save(&platform)).expect("saved");
    let loaded = block_on(store.load()).expect("loaded");
    assert_eq!(loaded.users.len(), 1, "the owner is still there");
    assert_eq!(
        loaded.users[0].email, "ada@example.com",
        "and it is the same owner"
    );
    assert_eq!(
        loaded.jwt_secret, platform.jwt_secret,
        "the signing key came back unchanged"
    );
}

#[test]
fn saving_twice_does_not_duplicate_anything() {
    let store = store();
    let platform = signed_up();
    block_on(store.save(&platform)).expect("saved");
    block_on(store.save(&platform)).expect("saved again");
    let loaded = block_on(store.load()).expect("loaded");
    assert_eq!(loaded.users.len(), 1, "one owner, not two");
}

#[test]
fn an_app_survives_a_round_trip() {
    let store = store();
    let mut platform = signed_up();
    let app_id = platform
        .create_app(nanobase::AppDoc::empty("todos"), now_secs())
        .expect("created")
        .id
        .clone();
    block_on(store.save(&platform)).expect("saved");
    let loaded = block_on(store.load()).expect("loaded");
    assert_eq!(loaded.apps.len(), 1, "the app is still there");
    assert_eq!(loaded.apps[0].id, app_id, "with the same id");
    assert_eq!(loaded.apps[0].doc.app.name, "todos", "and the same name");
}

#[test]
fn the_state_is_one_row() {
    let store = store();
    let platform = signed_up();
    block_on(store.save(&platform)).expect("saved");
    let db = store.db();
    let rows = db.rows(COLLECTION).expect("rows");
    assert_eq!(rows.len(), 1, "the whole platform is one row");
    let document = rows[0].get("document").and_then(|v| v.as_str());
    assert!(
        document.is_some(),
        "and the row holds the platform document"
    );
}
