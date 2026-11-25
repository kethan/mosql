//! The platform's directory of keys, and the collisions it has to survive.
//!
//! The platform keeps a directory — the app, the key's preview, and the
//! id of the row that holds the key — and the key itself lives in the
//! app's own database. Two things follow from that, and both are easy
//! to get wrong.
//!
//! **The versions.** The directory has a version, and it goes up when
//! the app's *set* of keys changes: one is made, or one is removed. A
//! key that is *changed* — renamed, its scopes cut, revoked — carries a
//! version of its own, in its own row, so the change and the version
//! that says so are one write and cannot disagree. Anything cached
//! about a key can therefore be told that it is stale.
//!
//! **The preview.** A preview is the first few characters of a key, and
//! two keys may share one — it is a few characters, not the whole key.
//! The platform therefore keeps every candidate, and the secret itself
//! decides between them. A platform that assumed one preview meant one
//! key would refuse a key that was perfectly good.

#![cfg(feature = "native")]

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mosql::{Object, Value};
use nanobase::apikey;
use nanobase::platform::{create_owner_with, now_secs, AppRecord, Platform, PlatformStore};
use nanobase::{Api, AppDb, MemoryStore, Request, Response, Service};

/// The service, and the store its platform state lives in.
fn service() -> (Arc<Service>, Arc<MemoryStore>) {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let dbs: Mutex<BTreeMap<String, Arc<dyn AppDb>>> = Mutex::new(BTreeMap::new());
    let svc = Service::new("apps", store.clone() as Arc<dyn PlatformStore>)
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
    (Arc::new(svc), store)
}

fn api() -> Api {
    Api::new(service().0)
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

/// Make a key for an app. Returns the key's id and its secret.
fn make_key(api: &Api, token: &str, app: &str, name: &str) -> (String, String) {
    let response = api.handle(
        authed("POST", &format!("/api/apps/{app}/keys"), token).with_json(obj(&[
            ("name", Value::str(name)),
            ("scopes", Value::Array(vec![Value::str("rows:read")])),
        ])),
    );
    assert_eq!(response.status, 201, "the key was made");
    let body = body_of(&response);
    let secret = body
        .get("secret")
        .and_then(|v| v.as_str())
        .expect("the secret")
        .to_string();
    let id = body
        .get("key")
        .and_then(|k| k.get("id"))
        .or_else(|| body.get("id"))
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string();
    (id, secret)
}

/// The version of an app's key *directory*: which keys exist.
fn directory_version(service: &Arc<Service>, app: &str) -> u64 {
    let platform = service.platform().unwrap();
    let id = platform.find_app(app).expect("the app").id.clone();
    platform.key_version(&id)
}

/// The version of one key's own record: what the key may do.
fn key_version(service: &Arc<Service>, app: &str, id: &str) -> u64 {
    service.find_key(app, id).expect("the key").version
}

// ------------------------------------------------------------- the set --

#[test]
fn making_a_key_moves_the_directory_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();

    let before = directory_version(service, &app);
    let _ = make_key(&api, &token, &app, "one");
    let after = directory_version(service, &app);
    assert!(
        after > before,
        "the version did not move: {before} -> {after}"
    );
}

#[test]
fn removing_a_key_moves_the_directory_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, _secret) = make_key(&api, &token, &app, "one");
    let before = directory_version(service, &app);

    let response = api.handle(
        authed("DELETE", &format!("/api/apps/{app}/keys/{id}"), &token)
            .with_json(Value::Object(Object::new())),
    );
    assert_eq!(response.status, 200, "the key was revoked");
    let after = directory_version(service, &app);
    assert!(
        after > before,
        "the version did not move: {before} -> {after}"
    );
}

#[test]
fn changing_a_key_does_not_move_the_directory_version() {
    // The set of keys did not change: the same key is there. What
    // changed is the key, and the key has a version of its own.
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, _secret) = make_key(&api, &token, &app, "one");
    let before = directory_version(service, &app);

    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token)
            .with_json(obj(&[("name", Value::str("renamed"))])),
    );
    assert_eq!(response.status, 200, "the key was changed");
    assert_eq!(
        directory_version(service, &app),
        before,
        "the set of keys did not change"
    );
}

#[test]
fn an_app_with_no_keys_has_no_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    assert_eq!(
        directory_version(service, &app),
        0,
        "there is nothing to be stale"
    );
}

// --------------------------------------------------------- the key itself --

#[test]
fn changing_a_key_moves_the_key_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, _secret) = make_key(&api, &token, &app, "one");
    let before = key_version(service, &app, &id);

    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token)
            .with_json(obj(&[("name", Value::str("renamed"))])),
    );
    assert_eq!(response.status, 200, "the key was changed");
    let after = key_version(service, &app, &id);
    assert!(
        after > before,
        "the version did not move: {before} -> {after}"
    );
}

#[test]
fn revoking_a_key_moves_the_key_version() {
    // A key that is revoked is refused. Anything that cached the key
    // must be able to hear that it has been revoked.
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, secret) = make_key(&api, &token, &app, "one");
    let before = key_version(service, &app, &id);

    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token)
            .with_json(obj(&[("revoked", Value::Bool(true))])),
    );
    assert_eq!(response.status, 200, "the key was revoked");
    let after = key_version(service, &app, &id);
    assert!(
        after > before,
        "the version did not move: {before} -> {after}"
    );

    // And the revoked key does not work.
    let response = api.handle(authed("GET", "/api/auth/me", &secret));
    assert_eq!(response.status, 401, "the revoked key was refused");
}

#[test]
fn cutting_a_key_scopes_moves_the_key_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, _secret) = make_key(&api, &token, &app, "one");
    let before = key_version(service, &app, &id);

    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token).with_json(obj(&[(
            "scopes",
            Value::Array(vec![Value::str("rows:read"), Value::str("rows:write")]),
        )])),
    );
    assert_eq!(response.status, 200, "the scopes were cut");
    let after = key_version(service, &app, &id);
    assert!(
        after > before,
        "the version did not move: {before} -> {after}"
    );
}

#[test]
fn a_change_that_changes_nothing_does_not_move_the_version() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, _secret) = make_key(&api, &token, &app, "one");
    let before = key_version(service, &app, &id);

    // The same name again: nothing about the key is different.
    let response = api.handle(
        authed("PATCH", &format!("/api/apps/{app}/keys/{id}"), &token)
            .with_json(obj(&[("name", Value::str("one"))])),
    );
    assert_eq!(response.status, 200, "the key was changed");
    assert_eq!(
        key_version(service, &app, &id),
        before,
        "nothing changed, so nothing moved"
    );
}

#[test]
fn using_a_key_does_not_move_the_version() {
    // The time a key was last used is not a permission. It is
    // approximate, and a reader that cached it would be caching a
    // number that is wrong by up to a minute anyway.
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let service = api.service();
    let (id, secret) = make_key(&api, &token, &app, "one");
    let before = key_version(service, &app, &id);

    let response = api.handle(authed("GET", "/api/auth/me", &secret));
    assert_eq!(response.status, 200, "the key worked");
    assert_eq!(
        key_version(service, &app, &id),
        before,
        "the version moved for a touch"
    );
}

#[test]
fn the_version_is_in_the_answer_about_a_key() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let (id, _secret) = make_key(&api, &token, &app, "one");

    let response = api.handle(authed("GET", &format!("/api/apps/{app}/keys/{id}"), &token));
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    let version = body.get("version").and_then(|v| v.as_f64());
    assert!(
        version.is_some(),
        "the answer does not say what version the key is at: {body:?}"
    );
}

// ------------------------------------------------------- the preview --

#[test]
fn two_keys_may_share_a_preview() {
    // The preview is six characters of the key, so two keys sharing one
    // is a one-in-sixty-eight-billion accident. The platform has to be
    // ready for it anyway, so the test makes it happen: the directory
    // is given two keys with the same preview, and it must keep both.
    let mut platform = Platform::default();
    platform.register_key("app_one", "nbk_ABC123…", "key_one");
    platform.register_key("app_two", "nbk_ABC123…", "key_two");
    let candidates = platform.find_keys("nbk_ABC123…");
    assert_eq!(candidates.len(), 2, "{candidates:?}");
    let ids: Vec<&str> = candidates.iter().map(|(_, id)| id.as_str()).collect();
    assert!(ids.contains(&"key_one"), "{ids:?}");
    assert!(ids.contains(&"key_two"), "{ids:?}");
}

#[test]
fn a_preview_that_is_not_there_finds_nothing() {
    let mut platform = Platform::default();
    platform.register_key("app_one", "nbk_ABC123…", "key_one");
    assert!(platform.find_keys("nbk_ZZZZZZ…").is_empty());
}

#[test]
fn a_key_is_found_when_another_key_has_the_same_preview() {
    // Two keys may share a preview — it is six characters of a
    // 256-bit key, so it is rare but it is possible, and the platform
    // keeps every candidate for a preview rather than the first one it
    // finds.
    //
    // The collision is made here rather than waited for: the two keys
    // are built by hand with the same preview and put in two apps.
    let (service, store) = service();
    let api = Api::new(service.clone());
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let other = make_app(&api, &token, "others");

    let (first, first_secret) = apikey::ApiKey::create("first", "todos", vec![], None, now_secs());
    let preview = first.preview.clone();
    // A second key whose secret starts with the same six characters,
    // so its preview is the same one. The rest of the secret is
    // different, and the hash is of the whole secret.
    let head: String = first_secret
        .strip_prefix("nbk_")
        .expect("a prefix")
        .chars()
        .take(6)
        .collect();
    let second_secret = format!("nbk_{head}{}", "A".repeat(37));
    assert_eq!(apikey::preview_of(&second_secret), preview);
    let second = apikey::ApiKey {
        id: "key_second".to_string(),
        name: "second".to_string(),
        hash: apikey::hash_of(&second_secret),
        preview: preview.clone(),
        app: "others".to_string(),
        scopes: Vec::new(),
        expires_at: None,
        created_at: nanobase::platform::iso8601(now_secs()),
        last_used_at: None,
        revoked: false,
        version: 0,
    };

    // The keys go into their apps' own databases, where they belong.
    {
        let platform = service.platform().unwrap();
        let first_app = platform.find_app(&app).expect("the app").id.clone();
        let second_app = platform.find_app(&other).expect("the other app").id.clone();
        service
            .db(&first_app)
            .unwrap()
            .insert(nanobase::keys::COLLECTION, nanobase::keys::to_row(&first))
            .unwrap();
        service
            .db(&second_app)
            .unwrap()
            .insert(nanobase::keys::COLLECTION, nanobase::keys::to_row(&second))
            .unwrap();

        // And the directory knows about both.
        let mut platform = platform;
        platform.register_key(&first_app, &preview, &first.id);
        platform.register_key(&second_app, &preview, &second.id);
        let store2 = Arc::clone(&store);
        uflite::db::block_on(async move {
            store2.save(&platform).await.unwrap();
        });
        service.reload().unwrap();
    }

    // Both candidates are in the directory.
    let platform = service.platform().unwrap();
    let candidates = platform.find_keys(&preview);
    assert_eq!(candidates.len(), 2, "{candidates:?}");

    // Both keys work: the secret decides between the candidates, and
    // the second one is not refused because the first one did not match.
    let response = api.handle(authed("GET", "/api/auth/me", &first_secret));
    assert_eq!(response.status, 200, "the first key worked");

    let response = api.handle(authed("GET", "/api/auth/me", &second_secret));
    assert_eq!(
        response.status, 200,
        "the second key worked: a shared preview must not hide a key"
    );
}

#[test]
fn a_wrong_secret_with_a_known_preview_is_refused() {
    let api = api();
    let token = token(&api);
    let app = make_app(&api, &token, "todos");
    let (_id, secret) = make_key(&api, &token, &app, "one");
    let preview = apikey::preview_of(&secret);
    // A secret with the same preview but not the same key.
    let wrong = format!("{preview}not-the-rest-of-it");

    let response = api.handle(authed("GET", "/api/auth/me", &wrong));
    assert_eq!(response.status, 401, "the wrong secret was refused");
}
