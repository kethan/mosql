//! API keys: a credential a program holds instead of the owner's password.
//!
//! A key is a *program*: it belongs to one app, it may be limited to
//! what it may do, and it may expire on its own. These tests are about
//! the limits — a key that may only read must not be able to write, a
//! key made for one app must not see another, and an expired key must
//! stop working without anybody having to remember to delete it.
//!
//! A key belongs to exactly one app and lives in that app's own
//! database. The platform keeps a directory of which keys exist and
//! which app they belong to — the preview and the row id — and nothing
//! that could be used as a key.

#![cfg(feature = "native")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mosql::{Object, Value};
use nanobase::apikey::{self, ApiKey};
use nanobase::platform::{create_owner_with, now_secs, AppRecord, Platform, PlatformStore};
use nanobase::{Api, AppDb, MemoryStore, Request, Response, Service};

/// Every app gets a database of its own, in memory.
///
/// The keys live in the app's own database, so a test that wants to see
/// two apps' keys apart needs two databases.
fn service() -> Arc<Service> {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
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

/// Put a platform back into the store, and say so.
fn save(store: &Arc<MemoryStore>, platform: Platform) {
    let store2 = Arc::clone(store);
    uflite::db::block_on(async move {
        store2.save(&platform).await.unwrap();
    });
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

/// An app to hang the keys off.
fn make_app(api: &Api, token: &str, name: &str) -> String {
    let response = api
        .handle(authed("POST", "/api/apps", token).with_json(obj(&[("name", Value::str(name))])));
    assert_eq!(response.status, 201, "the app was made");
    body_of(&response)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string()
}

/// Make a key for an app, and return its secret.
fn make_key(api: &Api, token: &str, app: &str, name: &str, scopes: &[&str]) -> String {
    let scopes: Vec<Value> = scopes.iter().map(|s| Value::str(*s)).collect();
    let mut body = Object::new();
    body.set("name", Value::str(name));
    body.set("scopes", Value::Array(scopes));
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), token).with_json(Value::Object(body)),
    );
    assert_eq!(response.status, 201, "the key was made");
    body_of(&response)
        .get("secret")
        .and_then(|v| v.as_str())
        .expect("the secret")
        .to_string()
}

/// The id of the first key an app has.
fn first_key_id(api: &Api, token: &str, app: &str) -> String {
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/keys"), token));
    assert_eq!(response.status, 200, "the keys were listed");
    body_of(&response)
        .get("items")
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("id"))
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string()
}

// ------------------------------------------------------------------ keys --

#[test]
fn a_key_may_be_made_and_used() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "the deploy script", &["rows:read"]);

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
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "reader", &["apps:read"]);
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);
}

#[test]
fn a_key_is_shown_once_and_never_again() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "once", &["apps:read"]);

    // The list never carries the key itself.
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/keys"), &token));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains(&secret), "the key is not in the list");
    assert!(raw.contains("\"preview\""), "but the preview is");
    // And the hash is not in it either.
    assert!(!raw.contains("\"hash\""), "nor the hash");
}

#[test]
fn a_key_belongs_to_one_app() {
    let api = api();
    let token = token(&api);
    let todos = make_app(&api, &token, "todos");
    let billing = make_app(&api, &token, "billing");
    let secret = make_key(
        &api,
        &token,
        &todos,
        "todo reader",
        &["apps:read", "rows:read"],
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
    let secret = make_key(&api, &token, &todos, "todo reader", &["apps:read"]);

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
fn a_key_may_not_make_keys_for_another_app() {
    let api = api();
    let token = token(&api);
    let todos = make_app(&api, &token, "todos");
    let billing = make_app(&api, &token, "billing");
    let secret = make_key(&api, &token, &todos, "admin", &["admin"]);

    // The key may not touch another app's keys, even with `admin`.
    let response = api.handle(authed("GET", &format!("/api/apps/{billing}/keys"), &secret));
    assert_eq!(response.status, 403);
}

#[test]
fn an_expired_key_stops_working() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    // A key made in the past, so it is already over.
    let mut body = Object::new();
    body.set("name", Value::str("old"));
    body.set("scopes", Value::Array(vec![Value::str("apps:read")]));
    body.set("expiresAt", Value::Int(now_secs() - 10));
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), &token).with_json(Value::Object(body)),
    );
    assert_eq!(response.status, 201);
    let secret = body_of(&response)
        .get("secret")
        .and_then(|v| v.as_str())
        .expect("the secret")
        .to_string();

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
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "revoked", &["apps:read"]);
    let id = first_key_id(&api, &token, &app);

    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token)
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
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "admin", &["admin"]);
    // It reached the handler: the failure is about the scopes, not about
    // who is asking.
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), &secret).with_json(obj(&[
            ("name", Value::str("another")),
            ("scopes", Value::Array(vec![])),
        ])),
    );
    assert_eq!(response.status, 400);
}

#[test]
fn a_key_with_no_scopes_may_do_nothing() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), &token)
            .with_json(obj(&[("name", Value::str("empty"))])),
    );
    assert_eq!(response.status, 400);
}

#[test]
fn an_unknown_scope_is_refused() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), &token).with_json(obj(&[
            ("name", Value::str("typo")),
            ("scopes", Value::Array(vec![Value::str("rows:reed")])),
        ])),
    );
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
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "temporary", &["apps:read"]);
    let id = first_key_id(&api, &token, &app);

    let response = api.handle(authed(
        "DELETE",
        &format!("/api/apps/{app}/keys/{id}"),
        &token,
    ));
    assert_eq!(response.status, 200);

    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 401);
}

#[test]
fn a_key_may_come_in_the_key_header() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "header", &["apps:read"]);
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
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "nosy", &["apps:read"]);
    let response = api.handle(authed("GET", "/api/auth/me", &secret));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains("\"hash\""), "no hash in the answer");
    assert!(raw.contains("\"apiKey\""), "but it says what it is");
}

#[test]
fn the_keys_of_an_app_are_not_a_collection_of_the_app() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    make_key(&api, &token, &app, "hidden", &["apps:read"]);

    // The collection the keys live in is not in the list.
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/db"), &token));
    assert_eq!(response.status, 200);
    let names = body_of(&response)
        .get("collections")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        !names.iter().any(|v| v.as_str() == Some("_api_keys")),
        "the keys are not a collection of the app: {names:?}"
    );

    // And the row routes refuse it.
    let response = api.handle(authed(
        "GET",
        &format!("/api/apps/{app}/db/_api_keys"),
        &token,
    ));
    assert_eq!(response.status, 404);
}

#[test]
fn a_key_survives_a_restart() {
    // The key is in the app's own database and the directory is in the
    // platform, so both have to be there for the key to work.
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let dbs: Mutex<BTreeMap<String, Arc<dyn AppDb>>> = Mutex::new(BTreeMap::new());
    let resolver = {
        let dbs = Arc::new(dbs);
        Arc::new(move |record: &AppRecord| {
            let mut dbs = dbs.lock().expect("not poisoned");
            let db = dbs
                .entry(record.id.clone())
                .or_insert_with(|| {
                    Arc::new(nanobase::db::memory::MemoryDb::new()) as Arc<dyn AppDb>
                })
                .clone();
            Ok(db)
        })
    };
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver.clone());
    let api = Api::new(Arc::new(svc));
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "durable", &["apps:read"]);

    // The same platform, opened again: the key still works.
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver);
    let api = Api::new(Arc::new(svc));
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);
}

#[test]
fn removing_an_app_takes_its_keys_with_it() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let secret = make_key(&api, &token, &app, "doomed", &["apps:read"]);

    let response = api.handle(
        authed("DELETE", &format!("/api/apps/{app}"), &token)
            .with_json(obj(&[("confirm", Value::str(app.clone()))])),
    );
    assert_eq!(response.status, 200, "the app was removed");

    // The key cannot work any more: the app it belonged to is not here.
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 401);
}

// --------------------------------------------------------------- legacy --

#[test]
fn a_key_from_an_older_platform_is_moved_into_its_app() {
    // A platform from before the keys lived in their app's database.
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    save(&store, p);
    let dbs: Mutex<BTreeMap<String, Arc<dyn AppDb>>> = Mutex::new(BTreeMap::new());
    let resolver = {
        let dbs = Arc::new(dbs);
        Arc::new(move |record: &AppRecord| {
            let mut dbs = dbs.lock().expect("not poisoned");
            let db = dbs
                .entry(record.id.clone())
                .or_insert_with(|| {
                    Arc::new(nanobase::db::memory::MemoryDb::new()) as Arc<dyn AppDb>
                })
                .clone();
            Ok(db)
        })
    };
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver.clone());
    let api = Api::new(Arc::new(svc));
    let token = token(&api);
    let app = make_app(&api, &token, "todos");

    // A key made by the older platform, for that app.
    let (record, secret) = ApiKey::create(
        "old key",
        app.clone(),
        vec![apikey::SCOPE_APPS_READ.to_string()],
        None,
        now_secs(),
    );
    let mut platform = api.service().platform().unwrap();
    platform.legacy_keys.push(nanobase::apikey::LegacyKey {
        id: record.id.clone(),
        name: record.name.clone(),
        hash: record.hash.clone(),
        preview: record.preview.clone(),
        app: Some(app.clone()),
        scopes: record.scopes.clone(),
        expires_at: record.expires_at,
        created_at: record.created_at.clone(),
        last_used_at: record.last_used_at,
        revoked: record.revoked,
    });
    save(&store, platform);

    // The platform starts again, and the keys of the older platform are
    // moved into the app they belong to.
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver);
    let report = svc.ensure_migrated().unwrap();
    assert_eq!(report.moved.len(), 1, "the key was moved");
    assert!(report.unbound.is_empty());
    let api = Api::new(Arc::new(svc));

    // The secret still works: the hash came with the key.
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);

    // And it is now one of the app's own keys.
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/keys"), &token));
    assert_eq!(response.status, 200);
    let items = body_of(&response)
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("name").and_then(|v| v.as_str()),
        Some("old key")
    );
}

#[test]
fn a_key_from_an_older_platform_with_no_app_waits() {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    save(&store, p);
    let dbs: Mutex<BTreeMap<String, Arc<dyn AppDb>>> = Mutex::new(BTreeMap::new());
    let resolver = {
        let dbs = Arc::new(dbs);
        Arc::new(move |record: &AppRecord| {
            let mut dbs = dbs.lock().expect("not poisoned");
            let db = dbs
                .entry(record.id.clone())
                .or_insert_with(|| {
                    Arc::new(nanobase::db::memory::MemoryDb::new()) as Arc<dyn AppDb>
                })
                .clone();
            Ok(db)
        })
    };
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver.clone());
    let api = Api::new(Arc::new(svc));
    let token = token(&api);
    let app = make_app(&api, &token, "todos");

    // A key made by the older platform, for the whole platform.
    let (record, secret) = ApiKey::create(
        "platform key",
        "",
        vec![apikey::SCOPE_APPS_READ.to_string()],
        None,
        now_secs(),
    );
    let mut platform = api.service().platform().unwrap();
    platform.legacy_keys.push(nanobase::apikey::LegacyKey {
        id: record.id.clone(),
        name: record.name.clone(),
        hash: record.hash.clone(),
        preview: record.preview.clone(),
        app: None,
        scopes: record.scopes.clone(),
        expires_at: record.expires_at,
        created_at: record.created_at.clone(),
        last_used_at: record.last_used_at,
        revoked: record.revoked,
    });
    save(&store, platform);

    // The platform starts again. The key waits: a key belongs to an
    // app, and nobody has said which.
    let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(resolver);
    let report = svc.ensure_migrated().unwrap();
    assert_eq!(report.unbound.len(), 1, "the key is waiting");
    assert!(report.moved.is_empty());

    // It is still there, and it is still not lost.
    let waiting = svc.legacy_keys().unwrap();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].name, "platform key");

    // Given an app, it works — and it is that app's key from then on.
    svc.bind_legacy_key(&app, &record.id).unwrap();
    let api = Api::new(Arc::new(svc));
    let response = api.handle(authed("GET", "/api/apps", &secret));
    assert_eq!(response.status, 200);
    let response = api.handle(authed("GET", &format!("/api/apps/{app}/keys"), &token));
    let items = body_of(&response)
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(items.len(), 1);
}
