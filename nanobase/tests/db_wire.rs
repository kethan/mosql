//! The app database on the wire, and who is allowed on it.
//!
//! A database service that holds an app's rows wants a token on the
//! door. This checks the door over both transports: the right token gets
//! in, the wrong one does not, and a request that never arrives is
//! refused rather than treated as a guest.

// The serverless edition has no file store, no local database and no
// listener, so this test belongs to the full edition only.
#![cfg(feature = "native")]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::db::http::HttpDb;
use nanobase::db::memory::MemoryDb;
use nanobase::db::tcp::TcpDb;
use nanobase::db::{AppDb, DbOp, DbRequest, DbServer};
use nanobase::httpclient::PlainHttpClient;

const TOKEN: &str = "s3cret";

fn row(pairs: &[(&str, Value)]) -> Value {
    let mut o = Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

/// A server over a memory database, over TCP, that wants a token.
fn guarded_tcp_server() -> String {
    let server = Arc::new(DbServer::new(Arc::new(MemoryDb::new())).with_token(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = Arc::clone(&server);
    std::thread::spawn(move || {
        let _ = nanobase::db::serve_tcp(worker, listener);
    });
    addr.to_string()
}

/// A server over a memory database, over HTTP, that wants a token.
fn guarded_http_server() -> String {
    let server = Arc::new(DbServer::new(Arc::new(MemoryDb::new())).with_token(TOKEN));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = Arc::clone(&server);
    std::thread::spawn(move || {
        let _ = nanobase::http::serve_on(listener, 2, move |req| worker.handle_http(&req));
    });
    format!("http://{addr}")
}

/// Send lines and read the reply.
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

/// A collections request, as the client sends it.
fn collections_request() -> String {
    let request = DbRequest::new(DbOp::Collections, "");
    mosql::to_json(&request.to_value())
}

#[test]
fn the_right_token_gets_in() {
    let address = guarded_tcp_server();
    let db = TcpDb::new(address).with_token(TOKEN);
    // The server answers, so the door is open.
    assert!(db.collections().is_ok());
}

#[test]
fn the_wrong_token_does_not() {
    let address = guarded_tcp_server();
    let db = TcpDb::new(address).with_token("wrong");
    let error = db.collections().expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");
}

#[test]
fn no_token_does_not_either() {
    let address = guarded_tcp_server();
    let db = TcpDb::new(address);
    let error = db.collections().expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");
}

#[test]
fn the_header_line_is_read_as_a_header() {
    let address = guarded_tcp_server();
    let request = collections_request();
    let reply = exchange(
        &address,
        &[format!("authorization: Bearer {TOKEN}"), request],
    );
    assert!(!reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn the_token_may_be_sent_the_other_way_round() {
    let address = guarded_tcp_server();
    let request = collections_request();
    let reply = exchange(
        &address,
        &[format!("Authorization: bearer {TOKEN}"), request],
    );
    assert!(!reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn a_request_without_the_token_is_refused() {
    let address = guarded_tcp_server();
    let reply = exchange(&address, &[collections_request()]);
    assert!(reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn the_wrong_token_is_refused() {
    let address = guarded_tcp_server();
    let reply = exchange(
        &address,
        &["authorization: Bearer wrong".into(), collections_request()],
    );
    assert!(reply.contains("UNAUTHORIZED"), "{reply}");
}

#[test]
fn a_request_after_the_header_still_works() {
    // The connection carries as many requests as the client sends.
    let address = guarded_tcp_server();
    let request = collections_request();
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
fn the_http_database_wants_a_token_too() {
    let base = guarded_http_server();
    let db = HttpDb::new(base.clone(), Arc::new(PlainHttpClient));
    let error = db.collections().expect_err("refused");
    assert!(error.to_string().contains("token"), "{error}");

    let db = HttpDb::new(base, Arc::new(PlainHttpClient)).with_token(TOKEN);
    assert!(db.collections().is_ok());
}

#[test]
fn the_rows_go_through_the_guarded_socket() {
    let address = guarded_tcp_server();
    let db = TcpDb::new(address).with_token(TOKEN);
    db.insert("todos", row(&[("title", Value::str("write it down"))]))
        .expect("inserted");
    let rows = db.rows("todos").expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].get("title").and_then(|v| v.as_str()),
        Some("write it down")
    );
}
