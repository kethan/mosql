//! The wire, and who is allowed on it.
//!
//! A store service that holds somebody else's data wants a token on the
//! door. This checks the door: the right token gets in, the wrong one
//! does not, and a token that never arrives is refused rather than
//! treated as a guest.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;

use better_auth_core::auth::{Auth, AuthConfig, HttpClient, PlainHttpClient};
use better_auth_core::query_server::QueryServer;
use better_auth_core::store::http_store::HttpAuthStore;
use better_auth_core::store::tcp_store::TcpAuthStore;
use better_auth_core::store::{AuthStore, MemoryStore};
use common::obj;
use mosql::Value;

const TOKEN: &str = "s3cret";

/// A store server over TCP that wants a token, and its address.
fn guarded_tcp_store() -> String {
    let store = Arc::new(MemoryStore::new());
    let server = Arc::new(QueryServer::new(store as Arc<dyn AuthStore>).with_token(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = better_auth_core::query_server::serve_tcp(server, listener);
    });
    addr.to_string()
}

/// A store server over HTTP that wants a token, and its address.
fn guarded_http_store() -> String {
    let store = Arc::new(MemoryStore::new());
    let server = Arc::new(QueryServer::new(store as Arc<dyn AuthStore>).with_token(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = better_auth_core::query_server::serve_http_on(server, listener, 2);
    });
    format!("http://{addr}")
}

/// A count request, as the client sends it.
fn count_request() -> String {
    let mut request =
        better_auth_core::query::QueryRequest::new(better_auth_core::query::QueryOp::Count, "user");
    request.r#where = Vec::new();
    mosql::to_json(&request.to_value())
}

/// Send a line and read the reply.
fn exchange(address: &str, lines: &[String]) -> String {
    let mut stream = TcpStream::connect(address).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    for line in lines {
        stream.write_all(line.as_bytes()).expect("write");
        stream.write_all(b"\n").expect("write");
    }
    stream.flush().ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut reply = String::new();
    reader.read_line(&mut reply).expect("read");
    reply
}

#[test]
fn the_right_token_gets_in() {
    let address = guarded_tcp_store();
    let store = TcpAuthStore::new(address).with_token(TOKEN);
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 0);
}

#[test]
fn the_wrong_token_does_not() {
    let address = guarded_tcp_store();
    let store = TcpAuthStore::new(address).with_token("wrong");
    let error = uflite::db::block_on(store.count("user", &[])).expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");
}

#[test]
fn no_token_does_not_either() {
    let address = guarded_tcp_store();
    let store = TcpAuthStore::new(address);
    let error = uflite::db::block_on(store.count("user", &[])).expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");
}

#[test]
fn the_header_line_is_read_as_a_header() {
    let address = guarded_tcp_store();
    let request = count_request();
    let reply = exchange(
        &address,
        &[format!("authorization: Bearer {TOKEN}"), request.clone()],
    );
    assert!(!reply.contains("UNAUTHORIZED"), "{reply}");
    assert!(reply.contains("count"), "{reply}");
}

#[test]
fn the_token_may_be_sent_the_other_way_round() {
    let address = guarded_tcp_store();
    let request = count_request();
    let reply = exchange(
        &address,
        &[format!("Authorization: bearer {TOKEN}"), request.clone()],
    );
    assert!(!reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn a_request_without_the_token_is_refused() {
    let address = guarded_tcp_store();
    let request = count_request();
    let reply = exchange(&address, &[request]);
    assert!(reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn the_wrong_token_is_refused() {
    let address = guarded_tcp_store();
    let request = count_request();
    let reply = exchange(&address, &["authorization: Bearer wrong".into(), request]);
    assert!(reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn a_request_after_the_header_still_works() {
    // The connection carries as many requests as the client sends.
    let address = guarded_tcp_store();
    let request = count_request();
    let mut stream = TcpStream::connect(&address).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    for _ in 0..2 {
        stream
            .write_all(format!("authorization: Bearer {TOKEN}\n{request}\n").as_bytes())
            .expect("write");
        stream.flush().ok();
        let mut reply = String::new();
        reader.read_line(&mut reply).expect("read");
        assert!(!reply.contains("UNAUTHORIZED"), "{reply}");
    }
}

#[test]
fn the_http_store_wants_a_token_too() {
    let base = guarded_http_store();
    let client: Arc<dyn HttpClient> = Arc::new(PlainHttpClient);
    let store = HttpAuthStore::new(base.clone(), client.clone());
    let error = uflite::db::block_on(store.count("user", &[])).expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");

    let store = HttpAuthStore::new(base, client).with_token(TOKEN);
    let count = uflite::db::block_on(store.count("user", &[])).expect("counted");
    assert_eq!(count, 0);
}

#[test]
fn the_engine_signs_in_through_a_guarded_store() {
    // The whole thing, through the engine: the store is on the far side
    // of a socket and the door is locked.
    let store = Arc::new(MemoryStore::new());
    let server = Arc::new(QueryServer::new(store.clone() as Arc<dyn AuthStore>).with_token(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = better_auth_core::query_server::serve_tcp(server, listener);
    });
    let remote = TcpAuthStore::new(addr.to_string()).with_token(TOKEN);
    let mut config = AuthConfig::new("https://app.test", common::SECRET);
    config.password_scrypt_n = 1024;
    let auth = Arc::new(Auth::new(Arc::new(remote), config));

    let rig = common::Rig::over(auth);
    let (status, json, _) = uflite::db::block_on(async {
        rig.call(
            "POST",
            "/api/auth/sign-up/email",
            None,
            Some(obj(&[
                ("email", Value::str("ada@example.com")),
                ("password", Value::str("a-password-of-ten-characters")),
                ("name", Value::str("Ada")),
            ])),
        )
        .await
    });
    assert_eq!(status, 200, "{json:?}");
    // The user landed in the store on the far side.
    let found = uflite::db::block_on(async {
        store
            .find_one(
                "user",
                &[better_auth_core::store::WhereClause::eq(
                    "email",
                    Value::str("ada@example.com"),
                )],
            )
            .await
    })
    .expect("found");
    assert!(found.is_some());
}
