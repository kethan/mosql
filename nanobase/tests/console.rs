//! The console, and the three things that protect the API from a browser.
//!
//! The console is the platform's own web interface: three files compiled
//! into the binary and served from the same origin as the API. The other
//! three subjects here — the security headers, CORS and the rate limit —
//! are what make the API safe to hand to a browser at all.
//!
//! They are checked together because that is how they matter: a security
//! header that is only on the happy path is not a security header, and a
//! CORS policy that forgets the error answers is not a policy.

// The serverless edition has no file store, no local database and no
// listener, so this test belongs to the full edition only.
#![cfg(feature = "native")]

use std::sync::Arc;
use std::time::Duration;

use mosql::{Object, Value};
use nanobase::middleware::{CorsPolicy, HttpOptions};
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, MemoryStore, Request, Response, Service};

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
    let svc = Service::new("apps", store as Arc<dyn PlatformStore>).unwrap();
    Arc::new(svc)
}

fn api() -> Api {
    Api::new(service())
}

fn api_with(options: HttpOptions) -> Api {
    Api::with_options(service(), options)
}

fn body_of(response: &Response) -> Value {
    let text = String::from_utf8(response.body.clone()).unwrap();
    mosql::parse_json(&text).unwrap()
}

/// The integer a value holds, if it is one.
fn int_of(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Int(number)) => Some(*number),
        _ => None,
    }
}

fn header<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
    response
        .headers
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
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

// ------------------------------------------------------------ the secrets --

#[test]
fn a_session_never_carries_the_password_hash() {
    let api = api();
    let response = api.handle(Request::new("POST", "/api/auth/login").with_json(obj(&[
        ("email", Value::str("ada@example.com")),
        ("password", Value::str("correct horse")),
    ])));
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    let user = body.get("user").expect("the user");
    assert_eq!(
        user.get("email").and_then(|v| v.as_str()),
        Some("ada@example.com")
    );
    assert!(
        user.get("password").is_none(),
        "the hash must not leave the platform"
    );
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains("\"hash\""), "no hash in the answer");
    assert!(!raw.contains("\"salt\""), "no salt in the answer");
}

#[test]
fn asking_who_you_are_does_not_tell_everyone_the_hash() {
    let api = api();
    let token = token(&api);
    let response = api.handle(authed("GET", "/api/auth/me", &token));
    assert_eq!(response.status, 200);
    let raw = String::from_utf8(response.body).unwrap();
    assert!(!raw.contains("\"hash\""), "no hash in the answer");
    assert!(!raw.contains("\"salt\""), "no salt in the answer");
    assert!(raw.contains("ada@example.com"), "but the name is there");
}

// ------------------------------------------------------------ the console --

#[test]
fn the_console_is_served_at_the_root() {
    let response = nanobase::console::respond(&Request::new("GET", "/")).expect("the console");
    assert_eq!(response.status, 200);
    assert_eq!(
        header(&response, "content-type"),
        Some("text/html; charset=utf-8")
    );
    let body = String::from_utf8(response.body).unwrap();
    assert!(body.contains("Nanobase Console"), "the page is the console");
    assert!(body.contains("/console.js"), "and it loads its script");
}

#[test]
fn the_console_serves_its_assets() {
    let js = nanobase::console::respond(&Request::new("GET", "/console.js")).expect("the script");
    assert_eq!(
        header(&js, "content-type"),
        Some("text/javascript; charset=utf-8")
    );
    let css = nanobase::console::respond(&Request::new("GET", "/console.css")).expect("the styles");
    assert_eq!(
        header(&css, "content-type"),
        Some("text/css; charset=utf-8")
    );
}

#[test]
fn the_console_has_an_api_keys_page() {
    // The page is in the sidebar…
    let response = nanobase::console::respond(&Request::new("GET", "/")).expect("the console");
    let body = String::from_utf8(response.body).unwrap();
    assert!(body.contains("API keys"), "the sidebar names the page");
    assert!(body.contains("#keys"), "and it routes to it");
    // …and the script talks to the key routes, so a key can be made,
    // listed and revoked from the browser.
    let js = nanobase::console::respond(&Request::new("GET", "/console.js")).expect("the script");
    let script = String::from_utf8(js.body).unwrap();
    assert!(
        script.contains("/keys"),
        "the script makes, lists and revokes keys"
    );
    assert!(
        !script.contains("/api/keys"),
        "and the keys of an app are the app's own keys"
    );
}

#[test]
fn the_console_marks_the_page_that_is_open() {
    // One rule, one place: the item whose route is the one being shown is
    // the active one, and the style sheet knows what that looks like.
    let js = nanobase::console::respond(&Request::new("GET", "/console.js")).expect("the script");
    let script = String::from_utf8(js.body).unwrap();
    assert!(
        script.contains(".nav-item[data-route]"),
        "the sidebar items that are routes are found"
    );
    assert!(script.contains("active"), "and one of them is active");
    let css = nanobase::console::respond(&Request::new("GET", "/console.css")).expect("the styles");
    let styles = String::from_utf8(css.body).unwrap();
    assert!(
        styles.contains(".nav-item.active"),
        "and the style sheet says what active looks like"
    );
}

#[test]
fn the_console_answers_nothing_else() {
    assert!(nanobase::console::respond(&Request::new("GET", "/api/apps")).is_none());
    assert!(nanobase::console::respond(&Request::new("GET", "/nope")).is_none());
}

#[test]
fn the_console_has_its_own_content_policy() {
    let response = nanobase::console::respond(&Request::new("GET", "/")).unwrap();
    let csp = header(&response, "content-security-policy").unwrap();
    assert!(
        csp.contains("script-src 'self'"),
        "the policy is the console's"
    );
    assert!(!csp.contains("default-src 'none'"), "and not the API's");
}

// ------------------------------------------------------- security headers --

#[test]
fn every_answer_carries_the_security_headers() {
    let api = api();
    let response = api.handle(Request::new("GET", "/api/health"));
    assert_eq!(header(&response, "x-content-type-options"), Some("nosniff"));
    assert_eq!(header(&response, "x-frame-options"), Some("DENY"));
    assert_eq!(header(&response, "referrer-policy"), Some("no-referrer"));
    assert_eq!(
        header(&response, "x-permitted-cross-domain-policies"),
        Some("none")
    );
    assert!(header(&response, "content-security-policy")
        .unwrap()
        .contains("default-src 'none'"));
}

#[test]
fn an_error_carries_the_security_headers_too() {
    let api = api();
    let response = api.handle(Request::new("GET", "/api/apps"));
    assert_eq!(response.status, 401);
    assert_eq!(header(&response, "x-content-type-options"), Some("nosniff"));
}

#[test]
fn a_204_says_no_content() {
    assert_eq!(nanobase::http::status_text(204), "No Content");
}

// -------------------------------------------------------------------- cors --

#[test]
fn an_allowed_origin_is_allowed() {
    let options = HttpOptions::with_cors(CorsPolicy::from_origins("https://app.test"));
    let api = api_with(options);
    let mut request = Request::new("GET", "/api/health");
    request
        .headers
        .push(("origin".to_string(), "https://app.test".to_string()));
    let response = api.handle(request);
    assert_eq!(
        header(&response, "access-control-allow-origin"),
        Some("https://app.test")
    );
}

#[test]
fn an_unknown_origin_is_not_allowed() {
    let options = HttpOptions::with_cors(CorsPolicy::from_origins("https://app.test"));
    let api = api_with(options);
    let mut request = Request::new("GET", "/api/health");
    request
        .headers
        .push(("origin".to_string(), "https://evil.test".to_string()));
    let response = api.handle(request);
    assert_eq!(header(&response, "access-control-allow-origin"), None);
}

#[test]
fn a_preflight_is_answered() {
    let options = HttpOptions::with_cors(CorsPolicy::from_origins("https://app.test"));
    let api = api_with(options);
    let mut request = Request::new("OPTIONS", "/api/apps");
    request
        .headers
        .push(("origin".to_string(), "https://app.test".to_string()));
    let response = api.handle(request);
    assert_eq!(response.status, 204);
    assert_eq!(
        header(&response, "access-control-allow-origin"),
        Some("https://app.test")
    );
    assert!(header(&response, "access-control-allow-methods")
        .unwrap()
        .contains("PATCH"));
}

#[test]
fn a_closed_policy_allows_nothing() {
    let api = api();
    let mut request = Request::new("GET", "/api/health");
    request
        .headers
        .push(("origin".to_string(), "https://app.test".to_string()));
    let response = api.handle(request);
    assert_eq!(header(&response, "access-control-allow-origin"), None);
}

// -------------------------------------------------------------- rate limit --

#[test]
fn a_client_over_the_limit_is_told_to_wait() {
    let options = HttpOptions::default_closed().with_rate_limit(3, Duration::from_secs(60));
    let api = api_with(options);
    for _ in 0..3 {
        assert_eq!(api.handle(Request::new("GET", "/api/health")).status, 200);
    }
    let response = api.handle(Request::new("GET", "/api/health"));
    assert_eq!(response.status, 429);
    assert!(header(&response, "retry-after").is_some());
    assert_eq!(
        body_of(&response).get("code").and_then(|v| v.as_str()),
        Some("TOO_MANY_REQUESTS")
    );
}

#[test]
fn the_limit_is_per_client() {
    let options = HttpOptions::default_closed().with_rate_limit(1, Duration::from_secs(60));
    let api = api_with(options);
    let mut first = Request::new("GET", "/api/health");
    first
        .headers
        .push(("x-forwarded-for".to_string(), "10.0.0.1".to_string()));
    let mut second = Request::new("GET", "/api/health");
    second
        .headers
        .push(("x-forwarded-for".to_string(), "10.0.0.2".to_string()));
    assert_eq!(api.handle(first).status, 200);
    assert_eq!(api.handle(second).status, 200);
}

// ---------------------------------------------------------- search and page --

#[test]
fn the_apps_are_searched_by_name() {
    let api = api();
    let token = token(&api);
    for name in ["todos", "notes", "recipes"] {
        let response = api.handle(
            authed("POST", "/api/apps", &token).with_json(obj(&[("name", Value::str(name))])),
        );
        assert_eq!(response.status, 201, "the app {name} was created");
    }
    let response = api.handle(authed("GET", "/api/apps?search=rec", &token));
    let body = body_of(&response);
    assert_eq!(int_of(body.get("total")), Some(1));
    let items = body.get("items").and_then(|v| v.as_array()).unwrap();
    assert_eq!(items.len(), 1);
    let name = items[0]
        .get("doc")
        .and_then(|d| d.get("app"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str());
    assert_eq!(name, Some("recipes"));
}

#[test]
fn the_apps_are_paged() {
    let api = api();
    let token = token(&api);
    for index in 0..7 {
        let response = api.handle(
            authed("POST", "/api/apps", &token)
                .with_json(obj(&[("name", Value::str(format!("app-{index}")))])),
        );
        assert_eq!(response.status, 201);
    }
    let body = body_of(&api.handle(authed("GET", "/api/apps?page=1&perPage=3", &token)));
    assert_eq!(int_of(body.get("total")), Some(7));
    assert_eq!(int_of(body.get("page")), Some(1));
    assert_eq!(int_of(body.get("perPage")), Some(3));
    assert_eq!(int_of(body.get("pages")), Some(3));
    assert_eq!(
        body.get("items").and_then(|v| v.as_array()).unwrap().len(),
        3
    );

    let body = body_of(&api.handle(authed("GET", "/api/apps?page=3&perPage=3", &token)));
    assert_eq!(
        body.get("items").and_then(|v| v.as_array()).unwrap().len(),
        1
    );
}

#[test]
fn a_search_and_a_page_together() {
    let api = api();
    let token = token(&api);
    for name in ["alpha-one", "alpha-two", "beta-one"] {
        api.handle(
            authed("POST", "/api/apps", &token).with_json(obj(&[("name", Value::str(name))])),
        );
    }
    let body = body_of(&api.handle(authed("GET", "/api/apps?search=alpha&perPage=1", &token)));
    assert_eq!(int_of(body.get("total")), Some(2));
    assert_eq!(int_of(body.get("pages")), Some(2));
    assert_eq!(
        body.get("items").and_then(|v| v.as_array()).unwrap().len(),
        1
    );
}

// ---------------------------------------------------------- database test --

#[test]
fn a_database_can_be_tested() {
    let api = api().with_db_tester(Arc::new(|_key, config| {
        if config.kind == nanobase::DbKind::Memory {
            Ok(vec!["todos".to_string()])
        } else {
            Err("the database is not there".to_string())
        }
    }));
    let token = token(&api);
    let response = api.handle(
        authed("POST", "/api/apps/todos/db/test", &token)
            .with_json(obj(&[("database", obj(&[("type", Value::str("memory"))]))])),
    );
    assert_eq!(response.status, 200);
    let body = body_of(&response);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        body.get("collections")
            .and_then(|v| v.as_array())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_database_that_does_not_answer_is_reported() {
    let api = api().with_db_tester(Arc::new(|_key, _config| Err("no answer".to_string())));
    let token = token(&api);
    let response = api.handle(
        authed("POST", "/api/apps/todos/db/test", &token)
            .with_json(obj(&[("database", obj(&[("type", Value::str("libsql"))]))])),
    );
    let body = body_of(&response);
    assert_eq!(body.get("ok").and_then(|v| v.as_bool()), Some(false));
    assert_eq!(
        body.get("error").and_then(|v| v.as_str()),
        Some("no answer")
    );
}

#[test]
fn an_unknown_engine_is_refused_before_anything_is_opened() {
    let api = api().with_db_tester(Arc::new(|_key, _config| Ok(vec![])));
    let token = token(&api);
    let response = api.handle(
        authed("POST", "/api/apps/todos/db/test", &token).with_json(obj(&[(
            "database",
            obj(&[("type", Value::str("weaviate"))]),
        )])),
    );
    assert_eq!(response.status, 400);
}

// ------------------------------------------------------------ per-app cors --

#[test]
fn an_app_may_open_its_own_api() {
    let api = api();
    let token = token(&api);
    let created = api.handle(
        authed("POST", "/api/apps", &token).with_json(obj(&[("name", Value::str("todos"))])),
    );
    let id = body_of(&created)
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    api.handle(
        authed("PATCH", &format!("/api/apps/{id}"), &token).with_json(obj(&[(
            "config",
            obj(&[(
                "cors",
                obj(&[
                    ("enabled", Value::Bool(true)),
                    (
                        "origins",
                        Value::Array(vec![Value::str("https://front.test")]),
                    ),
                ]),
            )]),
        )])),
    );
    let mut request = authed("GET", &format!("/api/apps/{id}"), &token);
    request
        .headers
        .push(("origin".to_string(), "https://front.test".to_string()));
    let response = api.handle(request);
    assert_eq!(
        header(&response, "access-control-allow-origin"),
        Some("https://front.test")
    );
}
