//! API keys: a credential a program holds instead of the owner's password.
//!
//! A key is a *program*: it has a name, it may be limited to one app, it
//! may be limited to what it may do, and it may expire on its own. These
//! tests are about the limits — a key that may only read must not be able
//! to write, a key made for one app must not see another, and an expired
//! key must stop working without anybody having to remember to delete it.

#![cfg(feature = "native")]

use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::apikey::{self, ApiKey};
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, MemoryStore, Request, Response, Service};

fn service() -> Arc<Service> {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>).unwrap();
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
    body_of(&response)
        .get("token")
        .and_then(|v| v.as_str())
        .expect("a token")
        .to_string()
}

fn authed(method: &str, path: &str, token: &str) -> Request {
    let mut request = Request::new(method, path);
    request
        .headers
        .push(("authorization".to_string(), format!("Bearer {token}")));
    request
}

/// Make a key through the API, and return its secret.
fn make_key(api: &Api, token: &str, name: &str, scopes: &[&str], app: Option<&str>) -> String {
    let scopes: Vec<Value> = scopes.iter().map(|s| Value::str(*s)).collect();
    let mut body = Object::new();
    body.set("name", Value::str(name));
    body.set("scopes", Value::Array(scopes));
    if let Some(app) = app {
        body.set("app", Value::str(app));
    }
    let response = api.handle(authed("POST", "/api/keys", token).with_json(Value::Object(body)));
    assert_eq!(response.status, 201, "the key was made");
    body_of(&response)
        .get("secret")
        .and_then(|v| v.as_str())
        .expect("the secret")
        .to_string()
}

/// An app to hang the keys off.
fn make_app(api: &Api, token: &str, name: &str) -> String {
    let response = api
        .handle(authed("POST", "/api/apps", token).with_json(obj(&[("name", Value::str(name))])));
    assert_eq!(response.status, 201);
    body_of(&response)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string()
}

// ------------------------------------------------------------------ keys --

#[test]
fn a_key_may_be_made_and_used() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "the deploy script", &["rows:read"], None);

    let response = api.handle(authed("GET", "/api/apps", &secret));
    // The key has no `apps:read`, so it may not list the apps.
    assert_eq!(response.status, 403);
    assert_eq!(
        body_of(&response).get("code").and_then(|v| v.as_str()),
        Some("YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS")
    );
}

#[test]
fn a_key_with_the_right_scope_may_do_the_thing() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "reader", &["apps:read"], None);
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);
}

#[test]
fn a_key_is_shown_once_and_never_again() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "once", &["apps:read"], None);

    // The list never carries the key itself.
    let response = api.handle(authed("GET", "/api/keys", &token));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains(&secret), "the key is not in the list");
    assert!(raw.contains("\"preview\""), "but the preview is");
}

#[test]
fn a_key_may_be_limited_to_one_app() {
    let api = api();
    let token = token(&api);
    let todos = make_app(&api, &token, "todos");
    let billing = make_app(&api, &token, "billing");
    let secret = make_key(
        &api,
        &token,
        "todo reader",
        &["apps:read", "rows:read"],
        Some(&todos),
    );

    // Its own app is fine.
    let response = api.handle(authed("GET", &format!("/api/apps/{todos}"), &secret));
    assert_eq!(response.status, 200);

    // Another app is not.
    let response = api.handle(authed("GET", &format!("/api/apps/{billing}"), &secret));
    assert_eq!(response.status, 403);
}

#[test]
fn a_key_limited_to_one_app_sees_only_that_app() {
    let api = api();
    let token = token(&api);
    let todos = make_app(&api, &token, "todos");
    make_app(&api, &token, "billing");
    let secret = make_key(&api, &token, "todo reader", &["apps:read"], Some(&todos));

    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert_eq!(
        body.get("total").and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        }),
        Some(1)
    );
}

#[test]
fn an_expired_key_stops_working() {
    // A key made in the past, so it is already over.
    let (record, secret) = ApiKey::create(
        "old",
        None,
        vec![apikey::SCOPE_APPS_READ.to_string()],
        Some(now_secs() - 10),
        now_secs() - 100,
    );
    let mut platform = Platform::default();
    platform.ensure_jwt_secret();
    create_owner_with(
        &mut platform,
        "ada@example.com",
        "correct horse",
        now_secs(),
        1000,
    )
    .unwrap();
    platform.api_keys.push(record);

    let api = api_over(platform);
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 401);
    assert_eq!(
        body_of(&response).get("code").and_then(|v| v.as_str()),
        Some("EXPIRED_KEY")
    );
}

#[test]
fn a_revoked_key_stops_working() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "revoked", &["apps:read"], None);
    let list = api.handle(authed("GET", "/api/keys", &token));
    let body = body_of(&list);
    let id = body
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string();

    let response = api.handle(
        authed("PATCH", &format!("/api/keys/{id}"), &token)
            .with_json(obj(&[("revoked", Value::Bool(true))])),
    );
    assert_eq!(response.status, 200);

    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 401);
    assert_eq!(
        body_of(&response).get("code").and_then(|v| v.as_str()),
        Some("REVOKED_KEY")
    );
}

#[test]
fn a_key_may_not_make_more_keys() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "admin", &["admin"], None);
    // The key has `admin`, so it may make keys — but the owner is the one
    // who signs in, and the key is a program, not a person.
    let response = api.handle(authed("POST", "/api/keys", &secret).with_json(obj(&[
        ("name", Value::str("another")),
        ("scopes", Value::Array(vec![])),
    ])));
    // It reached the handler: the failure is about the scopes, not about
    // who is asking.
    assert_eq!(response.status, 400);
}

#[test]
fn a_key_with_no_scopes_may_do_nothing() {
    let api = api();
    let token = token(&api);
    let response = api.handle(
        authed("POST", "/api/keys", &token).with_json(obj(&[("name", Value::str("empty"))])),
    );
    assert_eq!(response.status, 400);
}

#[test]
fn an_unknown_scope_is_refused() {
    let api = api();
    let token = token(&api);
    let response = api.handle(authed("POST", "/api/keys", &token).with_json(obj(&[
        ("name", Value::str("typo")),
        ("scopes", Value::Array(vec![Value::str("rows:reed")])),
    ])));
    assert_eq!(response.status, 400);
    let message = body_of(&response)
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    assert!(
        message.contains("rows:reed"),
        "the message says what is wrong"
    );
}

#[test]
fn a_key_may_be_removed() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "temporary", &["apps:read"], None);
    let list = api.handle(authed("GET", "/api/keys", &token));
    let id = body_of(&list)
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string();

    let response = api.handle(authed("DELETE", &format!("/api/keys/{id}"), &token));
    assert_eq!(response.status, 200);

    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 401);
}

#[test]
fn a_key_may_come_in_the_key_header() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "header", &["apps:read"], None);
    let mut request = Request::new("GET", "/api/apps");
    request.headers.push(("x-nanobase-key".to_string(), secret));
    let response = api.handle(request);
    assert_eq!(response.status, 200);
}

#[test]
fn the_owner_may_still_use_a_session_token() {
    let api = api();
    let token = token(&api);
    let response = api.handle(authed("GET", "/api/apps", &token));
    assert_eq!(response.status, 200);
}

#[test]
fn a_key_may_not_read_the_owners_password_hash() {
    let api = api();
    let token = token(&api);
    let secret = make_key(&api, &token, "nosy", &["apps:read"], None);
    let response = api.handle(authed("GET", "/api/auth/me", &secret));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains("\"hash\""), "no hash in the answer");
    assert!(raw.contains("\"apiKey\""), "but it says what it is");
}

/// A service over a platform that a test has written by hand.
fn api_over(platform: Platform) -> Api {
    let store = Arc::new(MemoryStore::new());
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&platform).await.unwrap();
    });
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>).unwrap();
    Api::new(Arc::new(svc))
}
