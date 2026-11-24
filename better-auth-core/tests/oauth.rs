//! §7.3 — OAuth.
//!
//! The provider is never really called: a stub HTTP client answers the
//! token and userinfo requests, so the whole round trip can be checked —
//! the redirect out, the state, the PKCE challenge, the callback in and
//! the account that comes home.

mod common;

use std::sync::{Arc, Mutex};

use better_auth_core::async_test;
use better_auth_core::auth::{Auth, AuthConfig, HttpClient, HttpRequest, HttpResponse};
use better_auth_core::oauth_presets::OAuthRegistry;
use better_auth_core::store::MemoryStore;
use common::*;
use mosql::Value;

/// A stub provider: answers the token endpoint and the userinfo
/// endpoint, and remembers everything it was asked.
struct Stub {
    seen: Mutex<Vec<HttpRequest>>,
    token: String,
    profile: String,
    id_token: Option<String>,
    email_list: Option<String>,
    /// Extra fields for the token answer, as some providers put their
    /// profile in it.
    token_extra: Option<String>,
}

/// A JWT whose payload is the given JSON. Nothing verifies it: the
/// profile is read out of the payload, which is what the provider does.
fn id_token(payload: &str) -> String {
    format!(
        "{}.{}.sig",
        better_auth_core::crypto::b64url_encode(br#"{"alg":"none"}"#),
        better_auth_core::crypto::b64url_encode(payload.as_bytes())
    )
}

impl Stub {
    fn new(token: &str, profile: &str) -> Stub {
        Stub {
            seen: Mutex::new(Vec::new()),
            token: token.into(),
            profile: profile.into(),
            id_token: None,
            email_list: None,
            token_extra: None,
        }
    }

    fn with_token_extra(mut self, extra: &str) -> Stub {
        self.token_extra = Some(extra.into());
        self
    }

    fn with_id_token(mut self, id_token: &str) -> Stub {
        self.id_token = Some(id_token.into());
        self
    }

    fn with_email_list(mut self, body: &str) -> Stub {
        self.email_list = Some(body.into());
        self
    }

    fn seen(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }

    /// The body of the token request.
    fn token_body(&self) -> String {
        let seen = self.seen();
        let request = seen
            .iter()
            .find(|r| r.url.contains("token"))
            .expect("a token request");
        String::from_utf8_lossy(&request.body).into_owned()
    }
}

impl HttpClient for Stub {
    fn request(&self, request: &HttpRequest) -> better_auth_core::error::AuthResult<HttpResponse> {
        self.seen.lock().unwrap().push(request.clone());
        let url = request.url.to_ascii_lowercase();
        if url.contains("token") {
            let mut o = mosql::Object::new();
            o.set("access_token", Value::str(self.token.clone()));
            o.set("token_type", Value::str("Bearer"));
            o.set("expires_in", Value::Int(3600));
            o.set("scope", Value::str("openid profile email"));
            if let Some(id_token) = &self.id_token {
                o.set("id_token", Value::str(id_token.clone()));
            }
            if let Some(extra) = &self.token_extra {
                if let Ok(Value::Object(fields)) = mosql::parse_json(extra) {
                    for (k, v) in fields.iter() {
                        o.set(k.clone(), v.clone());
                    }
                }
            }
            return Ok(HttpResponse {
                status: 200,
                body: mosql::to_json(&Value::Object(o)).into_bytes(),
            });
        }
        if url.contains("emails") {
            let body = self.email_list.clone().unwrap_or_else(|| "[]".into());
            return Ok(HttpResponse {
                status: 200,
                body: body.into_bytes(),
            });
        }
        Ok(HttpResponse {
            status: 200,
            body: self.profile.clone().into_bytes(),
        })
    }
}

/// A rig whose provider is configured from a fixed environment.
fn rig_with(providers: &[(&str, &str, &str)], stub: Arc<Stub>) -> Rig {
    let mut config = AuthConfig::new("https://app.test", SECRET);
    config.pbkdf2_iterations = 1_000;
    config.allowed_origins = vec!["https://portal.example.com".into()];
    let (registry, problems) = OAuthRegistry::from_env_with(|name| {
        for (id, client_id, client_secret) in providers {
            if name == format!("{}_CLIENT_ID", id.to_uppercase()) {
                return Some((*client_id).into());
            }
            if name == format!("{}_CLIENT_SECRET", id.to_uppercase()) {
                return Some((*client_secret).into());
            }
        }
        None
    });
    assert!(problems.is_empty(), "{problems:?}");
    let auth = Auth::new(Arc::new(MemoryStore::new()), config)
        .with_oauth(registry)
        .with_http(stub);
    Rig {
        auth: Arc::new(auth),
        mailer: Arc::new(better_auth_core::auth::MemoryMailer::default()),
    }
}

/// The `state` in an authorization URL.
fn state_of(url: &str) -> String {
    url.split("state=")
        .nth(1)
        .expect("a state")
        .split('&')
        .next()
        .unwrap()
        .to_string()
}

async_test! {
    async fn providers_come_from_the_environment() {
        let (registry, problems) = OAuthRegistry::from_env_with(|name| match name {
            "GOOGLE_CLIENT_ID" => Some("google-id".into()),
            "GOOGLE_CLIENT_SECRET" => Some("google-secret".into()),
            "GITHUB_CLIENT_ID" => Some("github-id".into()),
            _ => None,
        });
        // A half-configured provider is reported, not silently ignored.
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].to_string().contains("GITHUB"), "{problems:?}");
        assert_eq!(registry.ids(), vec!["google"]);
        let google = registry.get("google").unwrap();
        assert_eq!(google.client_id, "google-id");
        assert_eq!(google.client_secret, "google-secret");
    }
}

async_test! {
    async fn the_redirect_out_carries_the_state_and_the_challenge() {
        let stub = Arc::new(Stub::new("token-1", r#"{"id":"1","email":"ada@x.co","name":"Ada"}"#));
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("google"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("redirect"), Some(&Value::Bool(true)));
        let url = str_of(&json, "url");
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth"), "{url}");
        assert!(url.contains("client_id=gid"), "{url}");
        assert!(url.contains("response_type=code"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains("code_challenge="), "{url}");
        assert!(url.contains("state="), "{url}");
        assert!(url.contains("redirect_uri="), "{url}");
        // The redirect URI is ours.
        assert!(url.contains("https%3A%2F%2Fapp.test%2Fapi%2Fauth%2Foauth2%2Fcallback%2Fgoogle"), "{url}");
    }
}

async_test! {
    async fn the_callback_signs_the_user_in() {
        // Google reads the profile out of the id_token.
        let stub = Arc::new(
            Stub::new("token-1", "{}").with_id_token(&id_token(
                r#"{"sub":"1","email":"ada@x.co","name":"Ada","email_verified":true}"#,
            )),
        );
        let rig = rig_with(&[("google", "gid", "gsecret")], stub.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("google"))])),
            )
            .await;
        let url = str_of(&json, "url");
        let state = state_of(&url);

        let (status, json, response) = rig
            .call(
                "GET",
                &format!("/api/auth/oauth2/callback/google?code=abc&state={state}"),
                None,
                None,
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(json.get("token").is_some());
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
        assert_eq!(json.get("user").unwrap().get("emailVerified"), Some(&Value::Bool(true)));
        let access = Rig::header_token(&response).unwrap().to_string();

        // The token request carried the code, the redirect URI and the
        // PKCE verifier.
        let seen = stub.seen();
        let token_request = seen.iter().find(|r| r.url.contains("token")).expect("a token request");
        let body = String::from_utf8_lossy(&token_request.body).into_owned();
        assert!(body.contains("code=abc"), "{body}");
        assert!(body.contains("client_id=gid"), "{body}");
        assert!(body.contains("code_verifier="), "{body}");

        // The account is linked and listed.
        let (status, json, _) = rig.call("GET", "/api/auth/list-accounts", Some(&access), None).await;
        assert_eq!(status, 200, "{json:?}");
        let accounts = json.get("accounts").unwrap().as_array().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(str_of(&accounts[0], "providerId"), "google");
        assert_eq!(str_of(&accounts[0], "accountId"), "1");
        assert!(accounts[0].get("accessToken").is_none(), "never leak the provider token");
    }
}

async_test! {
    async fn the_state_is_single_use() {
        let stub = Arc::new(
            Stub::new("token-1", "{}").with_id_token(&id_token(r#"{"sub":"1","email":"ada@x.co"}"#)),
        );
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("google"))])),
            )
            .await;
        let state = state_of(&str_of(&json, "url"));

        let (status, _, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/google?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 200);
        // The same state again is refused.
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/google?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 400, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_STATE");
    }
}

async_test! {
    async fn an_unknown_provider_is_refused() {
        let stub = Arc::new(Stub::new("token-1", "{}"));
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("myspace"))])),
            )
            .await;
        assert_eq!(status, 404, "{json:?}");
        assert_eq!(str_of(&json, "code"), "PROVIDER_NOT_FOUND");
    }
}

async_test! {
    async fn apple_answers_over_post() {
        let stub = Arc::new(
            Stub::new("token-1", "{}")
                .with_id_token(&id_token(r#"{"sub":"apple-1","email":"ada@icloud.com"}"#)),
        );
        let rig = rig_with(&[("apple", "aid", "asecret")], stub.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("apple"))])),
            )
            .await;
        let url = str_of(&json, "url");
        assert!(url.contains("response_mode=form_post"), "{url}");
        let state = state_of(&url);

        // Apple posts the answer back as a form.
        let mut req = better_auth_core::http::Request::new("POST", "/api/auth/oauth2/callback/apple");
        req = req.with_header("content-type", "application/x-www-form-urlencoded");
        req.body = format!("code=abc&state={state}&user={}", "%7B%22name%22%3A%7B%22firstName%22%3A%22Ada%22%7D%7D").into_bytes();
        let response = better_auth_core::routes::route(Arc::clone(&rig.auth), req).await;
        assert_eq!(response.status, 200, "{}", String::from_utf8_lossy(&response.body));
        let body = mosql::parse_json(&String::from_utf8_lossy(&response.body)).unwrap();
        assert_eq!(str_of(body.get("user").unwrap(), "email"), "ada@icloud.com");
        assert_eq!(str_of(body.get("user").unwrap(), "name"), "Ada");
    }
}

async_test! {
    async fn tiktok_sends_its_client_key() {
        let stub = Arc::new(Stub::new(
            "token-1",
            r#"{"data":{"user":{"open_id":"tt-1","email":"ada@x.co","display_name":"Ada"}}}"#,
        ));
        let rig = rig_with(&[("tiktok", "tid", "tsecret")], stub.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("tiktok"))])),
            )
            .await;
        let state = state_of(&str_of(&json, "url"));
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/tiktok?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let body = stub.token_body();
        assert!(body.contains("client_key=tid"), "{body}");
        assert!(!body.contains("client_id="), "{body}");
    }
}

async_test! {
    async fn notion_answers_with_json_and_its_own_profile() {
        // Notion puts the installing user in the token answer itself.
        let stub = Arc::new(
            Stub::new("token-1", r#"{"id":"n-1","name":"Ada"}"#).with_token_extra(
                r#"{"owner":{"user":{"id":"n-1","name":"Ada","person":{"email":"ada@x.co"}}}}"#,
            ),
        );
        let rig = rig_with(&[("notion", "nid", "nsecret")], stub.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("notion"))])),
            )
            .await;
        let state = state_of(&str_of(&json, "url"));
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/notion?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "email"), "ada@x.co");
        assert_eq!(str_of(user, "name"), "Ada");

        // The token request asked for JSON.
        let seen = stub.seen();
        let token_request = seen.iter().find(|r| r.url.contains("token")).unwrap();
        let headers = &token_request.headers;
        assert!(headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("accept") && v.contains("json")), "{headers:?}");
        assert!(headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v.starts_with("Basic ")), "{headers:?}");
    }
}

async_test! {
    async fn github_falls_back_to_the_email_list() {
        let stub = Arc::new(
            Stub::new("token-1", r#"{"id":7,"login":"ada","name":"Ada"}"#)
                .with_email_list(r#"[{"email":"ada@x.co","primary":true,"verified":true}]"#),
        );
        let rig = rig_with(&[("github", "hid", "hsecret")], stub);
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("github"))])),
            )
            .await;
        let state = state_of(&str_of(&json, "url"));
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/github?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
    }
}

async_test! {
    async fn an_anonymous_visitor_keeps_their_id_through_oauth() {
        let stub = Arc::new(
            Stub::new("token-1", "{}").with_id_token(&id_token(r#"{"sub":"1","email":"ada@x.co","name":"Ada"}"#)),
        );
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (_, json, response) = rig.call("POST", "/api/auth/sign-in/anonymous", None, Some(Value::Null)).await;
        let anon_id = str_of(json.get("user").unwrap(), "id");
        let access = Rig::header_token(&response).unwrap().to_string();

        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                Some(&access),
                Some(obj(&[("provider", Value::str("google"))])),
            )
            .await;
        let state = state_of(&str_of(&json, "url"));
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/oauth2/callback/google?code=abc&state={state}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "id"), anon_id, "the same user id, upgraded");
        assert_eq!(user.get("isAnonymous"), Some(&Value::Bool(false)));
        assert_eq!(str_of(user, "email"), "ada@x.co");
    }
}

async_test! {
    async fn linking_needs_a_signed_in_user() {
        let stub = Arc::new(Stub::new("token-1", r#"{"id":"1","email":"ada@x.co"}"#));
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/oauth2/link",
                None,
                Some(obj(&[("provider", Value::str("google"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "UNAUTHORIZED");
    }
}

async_test! {
    async fn a_provider_error_comes_back_as_an_error() {
        let stub = Arc::new(Stub::new("token-1", "{}"));
        let rig = rig_with(&[("google", "gid", "gsecret")], stub);
        let (status, json, _) = rig
            .call(
                "GET",
                "/api/auth/oauth2/callback/google?error=access_denied&error_description=No+thanks",
                None,
                None,
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
        assert_eq!(str_of(&json, "code"), "OAUTH_ACCESS_DENIED");
        assert_eq!(str_of(&json, "message"), "No thanks");
    }
}
