//! The HTTP API: the routes, the bearer token, and the events.

use std::sync::Arc;

use mosql::{Object, Value};
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, AppDoc, MemoryStore, Request, Service};

const FAST: u32 = 1000;

fn platform() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), FAST).unwrap();
    p
}

fn service() -> Arc<Service> {
    let store = Arc::new(MemoryStore::new());
    let p = platform();
    // Put the owner in the store the service reads.
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

fn body_of(response: &nanobase::Response) -> Value {
    let text = String::from_utf8(response.body.clone()).unwrap();
    mosql::parse_json(&text).unwrap()
}

fn get(api: &Api, path: &str) -> nanobase::Response {
    api.handle(Request::new("GET", path))
}

fn post(api: &Api, path: &str, body: Value) -> nanobase::Response {
    api.handle(Request::new("POST", path).with_json(body))
}

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut o = Object::new();
    for (k, v) in pairs {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

fn token(api: &Api) -> String {
    let response = post(
        api,
        "/api/auth/login",
        obj(&[
            ("email", Value::str("ada@example.com")),
            ("password", Value::str("correct horse")),
        ]),
    );
    assert_eq!(response.status, 200, "login failed");
    body_of(&response)
        .get("token")
        .and_then(|v| v.as_str())
        .expect("token")
        .to_string()
}

// ---------------------------------------------------------------- health ---

#[test]
fn health_is_open() {
    let api = api();
    let response = get(&api, "/api/health");
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert_eq!(body.get("status").and_then(|v| v.as_str()), Some("ok"));
}

#[test]
fn an_unknown_route_is_a_404() {
    let api = api();
    let response = get(&api, "/api/nope");
    assert_eq!(response.status, 404);
    assert_eq!(
        body_of(&response).get("code").and_then(|v| v.as_str()),
        Some("NOT_FOUND")
    );
}

// ------------------------------------------------------------------ auth ---

#[test]
fn login_gives_a_token_and_the_user() {
    let api = api();
    let response = post(
        &api,
        "/api/auth/login",
        obj(&[
            ("email", Value::str("ada@example.com")),
            ("password", Value::str("correct horse")),
        ]),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert!(body.get("token").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        body.get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
}

#[test]
fn a_wrong_password_is_refused() {
    let api = api();
    let response = post(
        &api,
        "/api/auth/login",
        obj(&[
            ("email", Value::str("ada@example.com")),
            ("password", Value::str("wrong")),
        ]),
    );
    assert_eq!(response.status, 401);
}

#[test]
fn the_apps_need_a_token() {
    let api = api();
    assert_eq!(get(&api, "/api/apps").status, 401);
    let response = api.handle(
        Request::new("GET", "/api/apps").with_header("Authorization", "Bearer not-a-token"),
    );
    assert_eq!(response.status, 401);
}

#[test]
fn the_token_identifies_the_owner() {
    let api = api();
    let token = token(&api);
    let response = api.handle(
        Request::new("GET", "/api/auth/me")
            .with_header("Authorization", &format!("Bearer {token}")),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert_eq!(
        body.get("user")
            .and_then(|u| u.get("email"))
            .and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
}

// ------------------------------------------------------------------ apps ---

#[test]
fn an_app_is_created_listed_shown_and_removed() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");

    let created = api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    assert_eq!(created.status, 201, "create failed");
    let body = body_of(&created);
    let id = body
        .get("id")
        .and_then(|v| v.as_str())
        .expect("id")
        .to_string();
    assert_eq!(
        body.get("doc")
            .and_then(|d| d.get("app"))
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str()),
        Some("todos")
    );

    let listed = api.handle(Request::new("GET", "/api/apps").with_header("Authorization", &auth));
    assert_eq!(listed.status, 200);
    assert_eq!(
        body_of(&listed).get("total").and_then(|v| v.as_f64()),
        Some(1.0)
    );

    let shown = api.handle(
        Request::new("GET", &format!("/api/apps/{id}")).with_header("Authorization", &auth),
    );
    assert_eq!(shown.status, 200);
    assert_eq!(
        body_of(&shown).get("id").and_then(|v| v.as_str()),
        Some(id.as_str())
    );

    let removed = api.handle(
        Request::new("DELETE", &format!("/api/apps/{id}"))
            .with_header("Authorization", &auth)
            .with_json(obj(&[("confirm", Value::str(id.clone()))])),
    );
    assert_eq!(removed.status, 200);
    assert_eq!(
        get(&api, "/api/apps").status,
        401,
        "listing without a token is still refused"
    );
}

#[test]
fn an_app_can_be_found_by_name() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");
    api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    let shown =
        api.handle(Request::new("GET", "/api/apps/todos").with_header("Authorization", &auth));
    assert_eq!(shown.status, 200);
    assert_eq!(
        body_of(&shown)
            .get("doc")
            .and_then(|d| d.get("app"))
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str()),
        Some("todos")
    );
}

#[test]
fn a_patch_changes_part_of_the_document() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");
    let created = api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    let id = body_of(&created)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("id")
        .to_string();
    let patched = api.handle(
        Request::new("PATCH", &format!("/api/apps/{id}"))
            .with_header("Authorization", &auth)
            .with_json(obj(&[("app", obj(&[("name", Value::str("renamed"))]))])),
    );
    assert_eq!(patched.status, 200);
    assert_eq!(
        body_of(&patched)
            .get("doc")
            .and_then(|d| d.get("app"))
            .and_then(|a| a.get("name"))
            .and_then(|v| v.as_str()),
        Some("renamed")
    );
}

#[test]
fn a_removal_needs_the_id_to_confirm() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");
    let created = api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    let id = body_of(&created)
        .get("id")
        .and_then(|v| v.as_str())
        .expect("id")
        .to_string();
    let refused = api.handle(
        Request::new("DELETE", &format!("/api/apps/{id}"))
            .with_header("Authorization", &auth)
            .with_json(obj(&[("confirm", Value::str("something else"))])),
    );
    assert_eq!(refused.status, 500);
    // And with the right confirmation it goes.
    let removed = api.handle(
        Request::new("DELETE", &format!("/api/apps/{id}"))
            .with_header("Authorization", &auth)
            .with_json(obj(&[("confirm", Value::str(id.clone()))])),
    );
    assert_eq!(removed.status, 200);
}

// --------------------------------------------------------------- methods ---

#[test]
fn a_custom_method_is_listed_and_called() {
    let service = service();
    service.method(
        "count",
        Arc::new(|svc, _args| {
            let apps = svc.list()?;
            Ok(Value::Int(apps.len() as i64))
        }),
    );
    let api = Api::new(Arc::clone(&service));
    let token = token(&api);
    let auth = format!("Bearer {token}");
    api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );

    let listed = api
        .handle(Request::new("GET", "/api/apps/todos/methods").with_header("Authorization", &auth));
    assert_eq!(listed.status, 200);
    let body = body_of(&listed);
    assert_eq!(
        body.get("methods")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(1)
    );

    let called = api.handle(
        Request::new("POST", "/api/apps/todos/methods/count")
            .with_header("Authorization", &auth)
            .with_json(Value::Object(Object::new())),
    );
    assert_eq!(called.status, 200);
    assert_eq!(body_of(&called).as_f64(), Some(1.0));
}

#[test]
fn an_unknown_method_is_an_error() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");
    api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    let called = api.handle(
        Request::new("POST", "/api/apps/todos/methods/nope")
            .with_header("Authorization", &auth)
            .with_json(Value::Object(Object::new())),
    );
    assert_eq!(called.status, 500);
}

// ---------------------------------------------------------------- events ---

#[test]
fn the_events_are_listed() {
    let api = api();
    let token = token(&api);
    let response = api.handle(
        Request::new("GET", "/api/events").with_header("Authorization", &format!("Bearer {token}")),
    );
    assert_eq!(response.status, 200);
    let names: Vec<String> = body_of(&response)
        .get("events")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(names, vec!["created", "updated", "patched", "removed"]);
}

#[test]
fn a_change_over_http_fires_the_event() {
    let service = service();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    service.on(
        nanobase::Event::Created,
        Arc::new(move |ctx| {
            sink.lock().unwrap().push(ctx.method.clone());
        }),
    );
    let api = Api::new(service);
    let token = token(&api);
    api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &format!("Bearer {token}"))
            .with_json(obj(&[("name", Value::str("todos"))])),
    );
    assert_eq!(*seen.lock().unwrap(), vec!["create".to_string()]);
}

// ------------------------------------------------------------------ docs ---

#[test]
fn a_full_document_can_be_posted() {
    let api = api();
    let token = token(&api);
    let auth = format!("Bearer {token}");
    let doc = AppDoc::empty("todos");
    let value = doc.to_value();
    let created = api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &auth)
            .with_json(value),
    );
    assert_eq!(created.status, 201);
}
