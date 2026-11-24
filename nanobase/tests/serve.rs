//! The server and the command line's client, end to end over HTTP.

use std::sync::Arc;

use mosql::Value;
use nanobase::platform::{create_owner_with, now_secs, Platform, PlatformStore};
use nanobase::{Api, ApiClient, MemoryStore, Request, Service};

/// A running platform on a free port, and the address it is on.
fn serve() -> (String, ApiClient) {
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let service = Arc::new(Service::new("apps", store as Arc<dyn PlatformStore>).unwrap());
    let api = Api::new(service);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let _ = nanobase::http::serve_on(listener, 2, move |req| api.handle(req));
    });
    let base = format!("http://{addr}");
    (
        base.clone(),
        ApiClient::from_options(Some(&base), None).unwrap(),
    )
}

#[test]
fn the_client_signs_in_and_reaches_the_api() {
    let (base, _client) = serve();
    let client = ApiClient::from_options(Some(&base), None).unwrap();
    // Sign in over HTTP and keep the token.
    let mut body = mosql::Object::new();
    body.set("email", Value::str("ada@example.com"));
    body.set("password", Value::str("correct horse"));
    let value = client
        .request("POST", "/api/auth/login", Some(&Value::Object(body)))
        .expect("login");
    let token = value
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();
    assert!(!token.is_empty());

    let client = ApiClient::from_options(Some(&base), Some(&token)).unwrap();
    let created = client
        .request(
            "POST",
            "/api/apps",
            Some(&{
                let mut o = mosql::Object::new();
                o.set("name", Value::str("todos"));
                Value::Object(o)
            }),
        )
        .expect("create");
    let id = created.get("id").and_then(|v| v.as_str()).expect("id");
    assert!(id.starts_with("app_"), "{id}");

    let listed = client.request("GET", "/api/apps", None).expect("list");
    assert_eq!(listed.get("total").and_then(|v| v.as_f64()), Some(1.0));
}

#[test]
fn a_refused_request_says_why() {
    let (base, _client) = serve();
    let client = ApiClient::from_options(Some(&base), None).unwrap();
    let error = client.request("GET", "/api/apps", None).unwrap_err();
    assert!(error.contains("UNAUTHORIZED"), "{error}");
}

#[test]
fn the_server_answers_over_tcp() {
    let (base, _client) = serve();
    // A plain request, the way an operator would make it.
    let mut stream = std::net::TcpStream::connect(base.trim_start_matches("http://")).unwrap();
    use std::io::{Read, Write};
    stream
        .write_all(b"GET /api/health HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
        .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 200 OK"), "{reply}");
    assert!(reply.contains("\"status\":\"ok\""), "{reply}");
}

#[test]
fn the_api_and_the_service_are_one_path() {
    // A change made through the API is visible to the service, and fires the
    // service's events.
    let store = Arc::new(MemoryStore::new());
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner_with(&mut p, "ada@example.com", "correct horse", now_secs(), 1000).unwrap();
    let store2 = Arc::clone(&store);
    uflite::db::block_on(async move {
        store2.save(&p).await.unwrap();
    });
    let service = Arc::new(Service::new("apps", store as Arc<dyn PlatformStore>).unwrap());
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    service.on(
        nanobase::Event::Created,
        Arc::new(move |ctx| {
            sink.lock().unwrap().push(ctx.method.clone());
        }),
    );
    let api = Api::new(Arc::clone(&service));

    // Sign in through the API.
    let mut body = mosql::Object::new();
    body.set("email", Value::str("ada@example.com"));
    body.set("password", Value::str("correct horse"));
    let login = api.handle(Request::new("POST", "/api/auth/login").with_json(Value::Object(body)));
    assert_eq!(login.status, 200);
    let token = mosql::parse_json(&String::from_utf8(login.body).unwrap())
        .unwrap()
        .get("token")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let mut name = mosql::Object::new();
    name.set("name", Value::str("todos"));
    let created = api.handle(
        Request::new("POST", "/api/apps")
            .with_header("Authorization", &format!("Bearer {token}"))
            .with_json(Value::Object(name)),
    );
    assert_eq!(created.status, 201);

    // The service sees it, and the event fired.
    assert_eq!(service.list().unwrap().len(), 1);
    assert_eq!(*seen.lock().unwrap(), vec!["create".to_string()]);
}
