//! The remote stores: the engine's data on another machine.
//!
//! Both adapters talk the same protocol, so the same server answers both.
//! The test signs up and signs in through the engine with the store on the
//! far side of a socket, which is the thing that has to work.

mod common;

use std::sync::Arc;

use better_auth_core::auth::{Auth, AuthConfig, HttpClient, PlainHttpClient};
use better_auth_core::query_server::QueryServer;
use better_auth_core::store::http_store::HttpAuthStore;
use better_auth_core::store::tcp_store::TcpAuthStore;
use better_auth_core::store::{AuthStore, MemoryStore, WhereClause};
use mosql::Value;

/// A store server over HTTP, and its address.
fn http_store() -> (Arc<MemoryStore>, String) {
    let store = Arc::new(MemoryStore::new());
    let server = Arc::new(QueryServer::new(store.clone() as Arc<dyn AuthStore>));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = better_auth_core::query_server::serve_http_on(server, listener, 2);
    });
    (store, format!("http://{addr}"))
}

/// A store server over TCP, and its address.
fn tcp_store() -> (Arc<MemoryStore>, String) {
    let store = Arc::new(MemoryStore::new());
    let server = Arc::new(QueryServer::new(store.clone() as Arc<dyn AuthStore>));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = better_auth_core::query_server::serve_tcp(server, listener);
    });
    (store, addr.to_string())
}

/// An engine over `store`.
fn engine(store: Arc<dyn AuthStore>) -> Arc<Auth> {
    let mut config = AuthConfig::new("https://app.test", common::SECRET);
    config.password_scrypt_n = 1024;
    Arc::new(Auth::new(store, config))
}

#[test]
fn the_http_store_round_trips_a_record() {
    let (store, base) = http_store();
    let client: Arc<dyn HttpClient> = Arc::new(PlainHttpClient);
    let remote = HttpAuthStore::new(base, client);

    let mut data = better_auth_core::models::RecordMap::new();
    data.set("email", Value::str("ada@example.com"));
    let created = uflite::db::block_on(remote.create("user", data)).expect("created");
    assert_eq!(
        created.get("email").and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
    // The record really is in the store on the other side.
    let found = uflite::db::block_on(remote.find_one(
        "user",
        &[WhereClause::eq("email", Value::str("ada@example.com"))],
    ))
    .expect("found");
    assert!(found.is_some());
    // And the far side really holds one record.
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 1);
}

#[test]
fn the_tcp_store_round_trips_a_record() {
    let (store, address) = tcp_store();
    let remote = TcpAuthStore::new(address);

    let mut data = better_auth_core::models::RecordMap::new();
    data.set("email", Value::str("ada@example.com"));
    uflite::db::block_on(remote.create("user", data)).expect("created");
    let found = uflite::db::block_on(remote.find_one(
        "user",
        &[WhereClause::eq("email", Value::str("ada@example.com"))],
    ))
    .expect("found");
    assert!(found.is_some());
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 1);
}

#[test]
fn the_engine_signs_up_through_a_remote_store() {
    let (store, base) = http_store();
    let client: Arc<dyn HttpClient> = Arc::new(PlainHttpClient);
    let remote = Arc::new(HttpAuthStore::new(base, client));
    let auth = engine(remote as Arc<dyn AuthStore>);
    let rig = common::Rig::over(Arc::clone(&auth));

    let (user, access, _refresh) =
        uflite::db::block_on(rig.sign_up("ada@example.com", "a good long password"));
    assert!(!access.is_empty(), "a token came back");
    assert_eq!(
        user.get("email").and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
    // The user row was written on the far side of the socket.
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 1);
}

#[test]
fn the_engine_signs_in_through_a_tcp_store() {
    let (store, address) = tcp_store();
    let remote = Arc::new(TcpAuthStore::new(address));
    let auth = engine(remote as Arc<dyn AuthStore>);
    let rig = common::Rig::over(Arc::clone(&auth));

    uflite::db::block_on(rig.sign_up("ada@example.com", "a good long password"));
    let (status, json, _) =
        uflite::db::block_on(rig.sign_in_raw("ada@example.com", "a good long password"));
    assert_eq!(status, 200, "{json:?}");
    // The user row is on the far side of the socket.
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 1);
}

#[test]
fn a_store_that_is_not_there_is_an_error_not_a_panic() {
    let remote = TcpAuthStore::new("127.0.0.1:1");
    let error = uflite::db::block_on(remote.find_one("user", &[])).expect_err("no store");
    assert!(
        error.to_string().contains("cannot reach the store"),
        "{error}"
    );
}
