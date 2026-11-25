//! The app's own rules.
//!
//! An app may say who may do what with its rows. The rules are JSON in
//! the app's document, the evaluator is `uflite`'s, and the answer is
//! the same in every engine.
//!
//! The platform owner is above the rules — the owner wrote them — so
//! the tests here use a scoped key, which is a `user`, and the rules
//! decide.

#![cfg(feature = "native")]

use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, AppDoc, MemoryStore, Request, Response, Service};

fn service() -> Arc<Service> {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    // Every app's rows are in memory: the rules are about the rows, not
    // about the engine they are in.
    let db = Arc::new(nanobase::db::memory::MemoryDb::new());
    let db2 = Arc::clone(&db);
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(Arc::new(move |_record: &nanobase::platform::AppRecord| {
            Ok(db2.clone() as Arc<dyn nanobase::db::AppDb>)
        }));
    Arc::new(svc)
}

fn api() -> Api {
    Api::new(service())
}

fn body_of(response: &Response) -> Value {
    let text = String::from_utf8(response.body.clone()).unwrap();
    mosql::parse_json(&text).unwrap()
}

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut o = Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

fn token(api: &Api) -> String {
    let response = api.handle(Request::new("POST", "/api/auth/login").with_json(obj(&[
        ("email", Value::str("ada@example.com")),
        ("password", Value::str("correct horse")),
    ])));
    assert_eq!(response.status, 200, "login failed");
    body_of(&response)
        .get("token")
        .and_then(|v| v.as_str())
        .expect("a token")
        .to_string()
}

fn authed(method: &str, path: &str, token: &str) -> Request {
    Request::new(method, path).with_header("authorization", &format!("Bearer {token}"))
}

/// An app whose rules say what the tests need them to say.
fn make_app(api: &Api, token: &str, name: &str, rules: &str) -> String {
    let mut doc = AppDoc::empty(name);
    doc.config.acl.enabled = true;
    doc.config.acl.rules = mosql::parse_json(rules)
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    let response = api.handle(authed("POST", "/api/apps", token).with_json(doc.to_value()));
    assert_eq!(response.status, 201, "the app was made");
    body_of(&response)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string()
}

/// A key with the given scopes, limited to one app: its id and its
/// secret.
fn make_key(api: &Api, token: &str, name: &str, scopes: &[&str], app: &str) -> (String, String) {
    let body = obj(&[
        ("name", Value::str(name)),
        (
            "scopes",
            Value::Array(scopes.iter().map(|s| Value::str(*s)).collect()),
        ),
    ]);
    let response =
        api.handle(authed("POST", &format!("/api/apps/{app}/keys"), token).with_json(body));
    assert_eq!(response.status, 201, "the key was made");
    let body = body_of(&response);
    (
        body.get("key")
            .and_then(|k| k.get("id"))
            .and_then(|v| v.as_str())
            .expect("the id")
            .to_string(),
        body.get("secret")
            .and_then(|v| v.as_str())
            .expect("the secret")
            .to_string(),
    )
}

/// Put a row in, as the owner.
fn put(api: &Api, token: &str, app: &str, collection: &str, row: Value) {
    let response = api
        .handle(authed("POST", &format!("/api/apps/{app}/db/{collection}"), token).with_json(row));
    assert_eq!(response.status, 200, "the row went in");
}

/// The rows a caller can see.
fn visible(api: &Api, token: &str, app: &str, collection: &str) -> Vec<String> {
    let response = api.handle(authed(
        "GET",
        &format!("/api/apps/{app}/db/{collection}"),
        token,
    ));
    assert_eq!(response.status, 200, "the rows came back");
    let body = body_of(&response);
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    items
        .iter()
        .map(|row| {
            row.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn a_rule_hides_the_rows_it_does_not_allow() {
    let api = api();
    let token = token(&api);
    // Anyone may read a row that is public, and nobody may read one
    // that is not.
    let app = make_app(
        &api,
        &token,
        "todos",
        r#"[{"id":"pub","permissions":["*:todos:list"],"check":{"public":true}}]"#,
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[
            ("title", Value::str("shared")),
            ("public", Value::Bool(true)),
        ]),
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[
            ("title", Value::str("private")),
            ("public", Value::Bool(false)),
        ]),
    );

    let (_id, key) = make_key(&api, &token, "reader", &["apps:read", "rows:read"], &app);
    let seen = visible(&api, &key, &app, "todos");
    assert_eq!(seen.len(), 1, "only the public row is visible");
}

#[test]
fn a_rule_without_a_match_hides_everything() {
    let api = api();
    let token = token(&api);
    let app = make_app(
        &api,
        &token,
        "todos",
        r#"[{"id":"none","permissions":["admin:todos:list"]}]"#,
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[("title", Value::str("private"))]),
    );
    let (_id, key) = make_key(&api, &token, "reader", &["apps:read", "rows:read"], &app);
    assert!(visible(&api, &key, &app, "todos").is_empty());
}

#[test]
fn the_owner_is_above_the_rules() {
    let api = api();
    let token = token(&api);
    let app = make_app(
        &api,
        &token,
        "todos",
        r#"[{"id":"none","permissions":["user:todos:list"]}]"#,
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[("title", Value::str("private"))]),
    );
    assert_eq!(visible(&api, &token, &app, "todos").len(), 1);
}

#[test]
fn a_rule_refuses_a_write() {
    let api = api();
    let token = token(&api);
    let app = make_app(
        &api,
        &token,
        "todos",
        r#"[{"id":"read","permissions":["user:todos:list"]}]"#,
    );
    let (_id, key) = make_key(
        &api,
        &token,
        "reader",
        &["apps:read", "rows:read", "rows:write"],
        &app,
    );
    // Reading is allowed.
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/db/todos"), &key));
    assert_eq!(response.status, 200);

    // Writing is not: the rule says nothing about creating.
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/db/todos"), &key)
            .with_json(obj(&[("title", Value::str("new"))])),
    );
    assert_eq!(response.status, 403, "the rules refused the row");
}

#[test]
fn a_rule_says_whose_rows_may_be_changed() {
    let api = api();
    let token = token(&api);
    let app = make_app(
        &api,
        &token,
        "todos",
        r#"[{"id":"own","permissions":["user:todos:update"],"check":{"owner":"{{user}}"}}]"#,
    );
    let (key_id, key) = make_key(
        &api,
        &token,
        "editor",
        &["apps:read", "rows:read", "rows:write"],
        &app,
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[
            ("title", Value::str("mine")),
            ("owner", Value::str(key_id.clone())),
        ]),
    );
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[
            ("title", Value::str("yours")),
            ("owner", Value::str("key-2")),
        ]),
    );
    // The key's own row may be changed.
    let mine = body_of(&api.handle(authed("GET", &format!("/api/apps/{app}/db/todos"), &token)));
    let id = mine
        .get("items")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .find(|row| row.get("owner").and_then(|v| v.as_str()) == Some(key_id.as_str()))
        .and_then(|row| row.get("id").and_then(|v| v.as_str()))
        .unwrap()
        .to_string();
    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/db/todos"), &key).with_json(obj(&[
            ("filter", obj(&[("id", Value::str(id.clone()))])),
            ("changes", obj(&[("title", Value::str("changed"))])),
        ])),
    );
    assert_eq!(response.status, 200, "the key changed its own row");

    // The other one may not: it looks as if it is not there.
    let other = body_of(&api.handle(authed("GET", &format!("/api/apps/{app}/db/todos"), &token)));
    let other_id = other
        .get("items")
        .and_then(|v| v.as_array())
        .unwrap()
        .iter()
        .find(|row| row.get("owner").and_then(|v| v.as_str()) == Some("key-2"))
        .and_then(|row| row.get("id").and_then(|v| v.as_str()))
        .unwrap()
        .to_string();
    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/db/todos"), &key).with_json(obj(&[
            ("filter", obj(&[("id", Value::str(other_id))])),
            ("changes", obj(&[("title", Value::str("changed"))])),
        ])),
    );
    assert_eq!(response.status, 404, "the other row is out of reach");
}

#[test]
fn an_app_with_no_rules_is_untouched() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos", "[]");
    put(
        &api,
        &token,
        &app,
        "todos",
        obj(&[("title", Value::str("open"))]),
    );
    let (_id, key) = make_key(&api, &token, "reader", &["apps:read", "rows:read"], &app);
    assert_eq!(visible(&api, &key, &app, "todos").len(), 1);
}
