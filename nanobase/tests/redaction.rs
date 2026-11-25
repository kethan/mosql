//! The app's secrets: what the platform hands out, and what it keeps.
//!
//! An app's document holds its database password, its token and its
//! storage keys. Anybody who may read the app list — which is anybody
//! with a program that holds `apps:read` — may read every app's
//! credentials, so the document the API hands out does not carry them.
//!
//! The owner is the platform's own root credential and may ask for them.
//! A program may not, whatever it asks for.

#![cfg(feature = "native")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mosql::{Object, Value};
use nanobase::platform::{create_owner_with, now_secs, AppRecord, Platform, PlatformStore};
use nanobase::{Api, AppDb, AppDoc, MemoryStore, Request, Response, Service};

const FAST: u32 = 1000;

fn service() -> Arc<Service> {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), FAST).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    // Every app gets a database of its own: the keys live in the app's
    // own database, and a test that makes a key needs one.
    let dbs: Mutex<BTreeMap<String, Arc<dyn AppDb>>> = Mutex::new(BTreeMap::new());
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(Arc::new(move |record: &AppRecord| {
            let mut dbs = dbs.lock().expect("not poisoned");
            let db = dbs
                .entry(record.id.clone())
                .or_insert_with(|| {
                    Arc::new(nanobase::db::memory::MemoryDb::new()) as Arc<dyn AppDb>
                })
                .clone();
            Ok(db)
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

/// An app with a database password, a token and a storage key in it.
fn make_secret_app(api: &Api, token: &str) -> String {
    let mut doc = AppDoc::empty("secretapp");
    doc.config.database.kind = nanobase::config::DbKind::Postgres;
    doc.config.database.raw_type = "postgres".to_string();
    doc.config.database.url = Some("postgres://me:hunter2@neon.example.com/mydb".to_string());
    doc.config.database.token = Some("neon-token".to_string());
    let response = api.handle(authed("POST", "/api/apps", token).with_json(doc.to_value()));
    assert_eq!(response.status, 201, "the app was made");
    body_of(&response)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string()
}

#[test]
fn the_app_list_does_not_carry_the_secrets() {
    let api = api();
    let token = token(&api);
    make_secret_app(&api, &token);

    let response = api.handle(authed("GET", "/api/apps", &token));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    for secret in ["hunter2", "neon-token"] {
        assert!(!raw.contains(secret), "{secret} leaked into the app list");
    }
    // The URL is still there — it says which database it is — but not
    // the password in it.
    assert!(raw.contains("neon.example.com"), "the host is still there");
}

#[test]
fn one_app_does_not_carry_the_secrets() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    let response = api.handle(authed("GET", &format!("/api/apps/{app}"), &token));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    for secret in ["hunter2", "neon-token"] {
        assert!(!raw.contains(secret), "{secret} leaked into the app");
    }
    assert!(raw.contains("neon.example.com"), "the host is still there");
}

#[test]
fn the_answer_says_which_fields_hold_a_secret() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    let response = api.handle(authed("GET", &format!("/api/apps/{app}"), &token));
    let body = body_of(&response);
    let secrets = body
        .get("secrets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let names: Vec<&str> = secrets.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"config.database.token"), "{names:?}");
    assert!(names.contains(&"config.database.url"), "{names:?}");
}

#[test]
fn the_owner_may_ask_for_the_secrets() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    let response = api.handle(authed(
        "GET",
        &format!("/api/apps/{app}?includeSecrets=true"),
        &token,
    ));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(raw.contains("neon-token"), "the owner sees the token");
    assert!(raw.contains("hunter2"), "and the password in the URL");
}

#[test]
fn a_program_may_not_ask_for_the_secrets() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    // A key with the scopes to read the apps.
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), &token).with_json(obj(&[
            ("name", Value::str("reader")),
            ("scopes", Value::Array(vec![Value::str("apps:read")])),
        ])),
    );
    assert_eq!(response.status, 201, "the key was made");
    let secret = body_of(&response)
        .get("secret")
        .and_then(|v| v.as_str())
        .expect("the secret")
        .to_string();

    let response = api.handle(authed(
        "GET",
        &format!("/api/apps/{app}?includeSecrets=true"),
        &secret,
    ));
    assert_eq!(response.status, 403, "a program may not have the secrets");
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains("neon-token"), "and it does not get them");
}

#[test]
fn a_save_that_does_not_touch_a_secret_keeps_it() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    // The console sends only the fields the person changed.
    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}"), &token).with_json(obj(&[(
            "config",
            obj(&[("database", obj(&[("type", Value::str("postgres"))]))]),
        )])),
    );
    assert_eq!(response.status, 200, "the app was saved");

    // The token is still there — the platform's copy of it.
    let platform = api.service().platform().unwrap();
    let record = platform.find_app(&app).expect("the app");
    assert_eq!(
        record.doc.config.database.token.as_deref(),
        Some("neon-token"),
        "the token was kept"
    );
    assert_eq!(
        record.doc.config.database.url.as_deref(),
        Some("postgres://me:hunter2@neon.example.com/mydb"),
        "the URL was kept"
    );
}

#[test]
fn the_export_of_an_app_is_redacted() {
    let api = api();
    let token = token(&api);
    let app = make_secret_app(&api, &token);

    let platform = api.service().platform().unwrap();
    let record = platform.find_app(&app).expect("the app");
    let exported = record.doc.to_json_pretty_redacted();
    assert!(!exported.contains("hunter2"), "the password leaked");
    assert!(!exported.contains("neon-token"), "the token leaked");
    assert!(
        exported.contains("neon.example.com"),
        "the host is still there"
    );
}
