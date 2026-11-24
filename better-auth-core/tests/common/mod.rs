//! Shared harness for the integration suites: an `Auth` over a memory
//! store with a recording mailer, and a `call` that goes through the real
//! route table.

// Each suite uses a part of this; not all of it.
#![allow(dead_code)]

use std::sync::Arc;

use better_auth_core::auth::{Auth, AuthConfig, MemoryMailer};
use better_auth_core::http::Request;
use better_auth_core::routes;
use better_auth_core::store::MemoryStore;
use mosql::Value;

pub const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789";

pub struct Rig {
    pub auth: Arc<Auth>,
    pub mailer: Arc<MemoryMailer>,
}

impl Rig {
    /// A rig over an engine the caller built, for tests that need a
    /// particular store.
    pub fn over(auth: Arc<Auth>) -> Rig {
        Rig {
            auth,
            mailer: Arc::new(MemoryMailer::default()),
        }
    }

    pub fn new() -> Rig {
        let mut config = AuthConfig::new("https://app.test", SECRET);
        config.password_scrypt_n = 1024;
        config.allowed_origins = vec!["https://portal.example.com".into()];
        let mailer = Arc::new(MemoryMailer::default());
        let auth = Auth::new(Arc::new(MemoryStore::new()), config).with_mailer(mailer.clone());
        Rig {
            auth: Arc::new(auth),
            mailer,
        }
    }

    /// One request through the route table. Returns the parsed JSON body
    /// (or `Null`) beside the response so headers stay readable.
    pub async fn call(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value, better_auth_core::http::Response) {
        let mut req = Request::new(method, path);
        if let Some(token) = bearer {
            req = req.with_header("Authorization", &format!("Bearer {token}"));
        }
        if let Some(body) = body {
            req = req.with_json(body);
        }
        let response = routes::route(Arc::clone(&self.auth), req).await;
        let status = response.status;
        let json = parse_body(&response.body);
        (status, json, response)
    }

    /// The `set-auth-token` header of a response.
    pub fn header_token(response: &better_auth_core::http::Response) -> Option<&str> {
        response
            .headers
            .iter()
            .find(|(k, _)| k == "set-auth-token")
            .map(|(_, v)| v.as_str())
    }

    /// `Location` of a redirect.
    pub fn location(response: &better_auth_core::http::Response) -> Option<&str> {
        response
            .headers
            .iter()
            .find(|(k, _)| k == "location")
            .map(|(_, v)| v.as_str())
    }

    /// Every URL in every mail sent so far, oldest first.
    pub fn links(&self) -> Vec<String> {
        self.mailer
            .sent
            .lock()
            .expect("mailer")
            .iter()
            .flat_map(|(_, _, text)| {
                text.split_whitespace()
                    .map(str::to_string)
                    .filter(|w| w.starts_with("http"))
            })
            .collect()
    }

    /// Sign a user up and return `(user, access, refresh)`.
    pub async fn sign_up(&self, email: &str, password: &str) -> (Value, String, String) {
        let (status, json, response) = self
            .call(
                "POST",
                "/api/auth/sign-up/email",
                None,
                Some(obj(&[
                    ("name", Value::str("Ada")),
                    ("email", Value::str(email)),
                    ("password", Value::str(password)),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        (
            json.get("user").cloned().unwrap_or(Value::Null),
            Rig::header_token(&response)
                .expect("token header")
                .to_string(),
            str_of(&json, "refreshToken"),
        )
    }

    /// Sign in with email and password; `(status, json, response)`.
    pub async fn sign_in_raw(
        &self,
        email: &str,
        password: &str,
    ) -> (u16, Value, better_auth_core::http::Response) {
        self.call(
            "POST",
            "/api/auth/sign-in/email",
            None,
            Some(obj(&[
                ("email", Value::str(email)),
                ("password", Value::str(password)),
            ])),
        )
        .await
    }
}

pub fn obj(fields: &[(&str, Value)]) -> Value {
    let mut o = mosql::Object::new();
    for (k, v) in fields {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

pub fn str_of(json: &Value, key: &str) -> String {
    json.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub fn parse_body(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return Value::Null;
    }
    mosql::parse_json(&String::from_utf8_lossy(bytes)).unwrap_or(Value::Null)
}

/// Pull `key=value` out of a URL's query.
pub fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    better_auth_core::http::parse_query(query)
        .into_iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v)
}
