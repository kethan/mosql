//! An app's rows, served over the API.
//!
//! The rows live in whichever engine the app's document names; the API
//! does not change when the engine does. These drive the API over HTTP
//! and the service underneath it.

// The serverless edition has no file store, no local database and no
// listener, so this test belongs to the full edition only.
#![cfg(feature = "native")]

use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::db::memory::MemoryDb;
use nanobase::db::AppDb;
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, MemoryStore, Request, Service};

const FAST: u32 = 1000;

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut o = Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

/// A platform with an owner and one app.
fn platform() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), FAST).unwrap();
    p
}

/// The service, with an app whose rows are in memory.
fn service() -> (Arc<Service>, Arc<MemoryDb>) {
    let store = Arc::new(MemoryStore::new());
    let mut p = platform();
    p.create_app(nanobase::AppDoc::empty("todos"), now_secs())
        .expect("an app");
    let db = Arc::new(MemoryDb::new());
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let db2 = Arc::clone(&db);
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>)
        .unwrap()
        .with_dbs(Arc::new(move |_record: &nanobase::platform::AppRecord| {
            Ok(db2.clone() as Arc<dyn AppDb>)
        }));
    (Arc::new(svc), db)
}

fn api() -> (Api, Arc<MemoryDb>) {
    let (service, db) = service();
    (Api::new(service), db)
}

fn body_of(response: &nanobase::Response) -> Value {
    let text = String::from_utf8(response.body.clone()).unwrap();
    mosql::parse_json(&text).unwrap()
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
        .expect("token")
        .to_string()
}

/// The app's id.
fn app_id(service: &Service) -> String {
    service
        .list()
        .expect("the apps")
        .first()
        .and_then(|app| app.get("id").and_then(|v| v.as_str()).map(str::to_string))
        .expect("an app")
}

#[test]
fn a_row_goes_in_and_comes_back() {
    let (api, _db) = api();
    let bearer = token(&api);
    let app = "todos";
    let response = api.handle(
        Request::new("POST", &format!("/api/apps/{app}/db/notes"))
            .with_header("authorization", &format!("Bearer {bearer}"))
            .with_json(obj(&[("title", Value::str("write it down"))])),
    );
    assert_eq!(response.status, 200, "{}", response.body.len());
    let row = body_of(&response);
    assert_eq!(
        row.get("title").and_then(|v| v.as_str()),
        Some("write it down")
    );
    assert!(row.get("id").and_then(|v| v.as_str()).is_some());

    let response = api.handle(
        Request::new("GET", &format!("/api/apps/{app}/db/notes"))
            .with_header("authorization", &format!("Bearer {bearer}")),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    let items = body.get("items").and_then(|v| v.as_array()).expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(body.get("total").and_then(|v| v.as_f64()), Some(1.0));
}

#[test]
fn the_collections_are_listed() {
    let (api, db) = api();
    db.insert("notes", obj(&[("title", Value::str("one"))]))
        .expect("inserted");
    let bearer = token(&api);
    let response = api.handle(
        Request::new("GET", "/api/apps/todos/db")
            .with_header("authorization", &format!("Bearer {bearer}")),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    let names: Vec<&str> = body
        .get("collections")
        .and_then(|v| v.as_array())
        .expect("collections")
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(names.contains(&"notes"), "{names:?}");
}

#[test]
fn a_filter_narrows_the_rows() {
    let (api, db) = api();
    db.insert("notes", obj(&[("done", Value::Bool(false))]))
        .expect("inserted");
    db.insert("notes", obj(&[("done", Value::Bool(true))]))
        .expect("inserted");
    let bearer = token(&api);
    let response = api.handle(
        Request::new(
            "GET",
            "/api/apps/todos/db/notes?filter=%7B%22done%22%3Afalse%7D",
        )
        .with_header("authorization", &format!("Bearer {bearer}")),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    let items = body.get("items").and_then(|v| v.as_array()).expect("items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("done").and_then(|v| v.as_bool()), Some(false));
}

#[test]
fn a_row_is_changed_and_taken_out() {
    let (api, _db) = api();
    let bearer = token(&api);
    let inserted = api.handle(
        Request::new("POST", "/api/apps/todos/db/notes")
            .with_header("authorization", &format!("Bearer {bearer}"))
            .with_json(obj(&[("title", Value::str("write it down"))])),
    );
    let row = body_of(&inserted);
    let id = row
        .get("id")
        .and_then(|v| v.as_str())
        .expect("an id")
        .to_string();

    let filter = obj(&[("id", Value::str(id.clone()))]);
    let mut changes = Object::new();
    changes.set("done", Value::Bool(true));
    let mut body = Object::new();
    body.set("filter", filter);
    body.set("changes", Value::Object(changes));
    let response = api.handle(
        Request::new("PATCH", "/api/apps/todos/db/notes")
            .with_header("authorization", &format!("Bearer {bearer}"))
            .with_json(Value::Object(body)),
    );
    assert_eq!(response.status, 200, "{}", response.body.len());
    let updated = body_of(&response);
    assert_eq!(updated.get("done").and_then(|v| v.as_bool()), Some(true));

    let mut filter = Object::new();
    filter.set("id", Value::str(id));
    let response = api.handle(
        Request::new("DELETE", "/api/apps/todos/db/notes")
            .with_header("authorization", &format!("Bearer {bearer}"))
            .with_json(Value::Object(filter)),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        body_of(&response).get("removed").and_then(|v| v.as_f64()),
        Some(1.0)
    );
}

#[test]
fn a_filter_that_matches_nothing_is_not_found() {
    let (api, _db) = api();
    let bearer = token(&api);
    let mut filter = Object::new();
    filter.set("title", Value::str("nothing like this"));
    let mut changes = Object::new();
    changes.set("done", Value::Bool(true));
    let mut body = Object::new();
    body.set("filter", Value::Object(filter));
    body.set("changes", Value::Object(changes));
    let response = api.handle(
        Request::new("PATCH", "/api/apps/todos/db/notes")
            .with_header("authorization", &format!("Bearer {bearer}"))
            .with_json(Value::Object(body)),
    );
    assert_eq!(response.status, 404, "{}", response.body.len());
}

#[test]
fn the_rows_of_an_app_need_a_token() {
    let (api, _db) = api();
    let response = api.handle(Request::new("GET", "/api/apps/todos/db/notes"));
    assert_eq!(response.status, 401);
}

#[test]
fn the_service_reaches_the_same_rows() {
    let (service, _db) = service();
    let app = app_id(&service);
    service
        .insert_row(&app, "notes", &obj(&[("title", Value::str("one"))]))
        .expect("inserted");
    let rows = service.rows(&app, "notes").expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("title").and_then(|v| v.as_str()), Some("one"));
    // And the same rows, through the API.
    let api = Api::new(Arc::clone(&service));
    let bearer = token(&api);
    let response = api.handle(
        Request::new("GET", &format!("/api/apps/{app}/db/notes"))
            .with_header("authorization", &format!("Bearer {bearer}")),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert_eq!(
        body.get("items")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(1)
    );
}

#[test]
fn a_change_to_a_row_fires_an_event() {
    let (service, _db) = service();
    let app = app_id(&service);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = Arc::clone(&seen);
    service.on(
        nanobase::Event::Created,
        Arc::new(move |ctx| {
            seen2
                .lock()
                .unwrap()
                .push((ctx.method.clone(), ctx.app.clone(), ctx.id.clone()));
        }),
    );
    service
        .insert_row(&app, "notes", &obj(&[("title", Value::str("one"))]))
        .expect("inserted");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let (method, whose, id) = &seen[0];
    assert!(method.starts_with("db.insert:"), "{seen:?}");
    // The listener knows whose rows changed, and which row.
    assert_eq!(whose.as_deref(), Some(app.as_str()), "{seen:?}");
    assert!(id.is_some(), "{seen:?}");
}
