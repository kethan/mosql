//! §7.3 — Tokens and sessions.
//!
//! Access tokens are HS256 JWTs carrying identity; refresh tokens are
//! opaque and rotated. This suite checks that a token means what it says,
//! expires when it says, cannot be altered, cannot carry a pending second
//! factor onto a normal route, and that a refresh token works exactly
//! once.

mod common;

use better_auth_core::async_test;
use better_auth_core::auth::AuthConfig;
use better_auth_core::crypto;
use better_auth_core::http::Request;
use better_auth_core::middleware;
use better_auth_core::store::MemoryStore;
use common::*;
use mosql::Value;
use std::sync::Arc;

#[test]
fn access_token_claims_and_signature() {
    let rig = Rig::new();
    let access = rig.auth.sign_access(
        &better_auth_core::models::User {
            id: "u1".into(),
            name: "Ada".into(),
            email: "ada@x.co".into(),
            email_verified: true,
            image: None,
            username: None,
            display_username: None,
            is_anonymous: false,
            two_factor_enabled: false,
            role: "admin".into(),
            banned: false,
            ban_reason: None,
            ban_expires: None,
            created_at: String::new(),
            updated_at: String::new(),
            extra: mosql::Object::new(),
        },
        "sess-1",
        Some("admin-9"),
    );
    // Three base64url parts; the payload decodes to exactly the claims
    // the spec names.
    let parts: Vec<&str> = access.split('.').collect();
    assert_eq!(parts.len(), 3);
    let claims = crypto::jwt_payload(&access).unwrap();
    for key in ["sub", "sid", "email", "role", "anon", "imp", "iat", "exp"] {
        assert!(claims.get(key).is_some(), "missing claim {key}: {claims:?}");
    }
    assert_eq!(claims.get("sub"), Some(&Value::str("u1")));
    assert_eq!(claims.get("sid"), Some(&Value::str("sess-1")));
    assert_eq!(claims.get("imp"), Some(&Value::str("admin-9")));
    assert_eq!(claims.get("purpose"), None);
    let iat = claims.get("iat").and_then(Value::as_f64).unwrap();
    let exp = claims.get("exp").and_then(Value::as_f64).unwrap();
    assert_eq!(exp - iat, 900.0);

    let decoded = rig.auth.decode(&access).unwrap();
    assert_eq!(decoded.role.as_deref(), Some("admin"));

    // Tampering with any part fails, with a 401.
    let mut tampered = access.clone();
    tampered.replace_range(tampered.len() - 2.., "xx");
    let err = rig.auth.decode(&tampered).unwrap_err();
    assert_eq!((err.status, err.code), (401, "INVALID_TOKEN"));
    let swapped = format!("{}.{}.{}", parts[0], parts[1], parts[0]);
    assert_eq!(rig.auth.decode(&swapped).unwrap_err().status, 401);

    // Another secret cannot verify it.
    let other = AuthConfig::new("https://app.test", "another-secret-another-secret-another");
    let other_auth = better_auth_core::auth::Auth::new(Arc::new(MemoryStore::new()), other);
    assert_eq!(other_auth.decode(&access).unwrap_err().status, 401);
}

#[test]
fn access_token_expiry_is_enforced() {
    let mut config = AuthConfig::new("https://app.test", SECRET);
    config.access_ttl_secs = 1;
    config.pbkdf2_iterations = 1_000;
    let auth = better_auth_core::auth::Auth::new(Arc::new(MemoryStore::new()), config);
    let user = better_auth_core::models::User {
        id: "u1".into(),
        name: String::new(),
        email: "e@x.co".into(),
        email_verified: true,
        image: None,
        username: None,
        display_username: None,
        is_anonymous: false,
        two_factor_enabled: false,
        role: "user".into(),
        banned: false,
        ban_reason: None,
        ban_expires: None,
        created_at: String::new(),
        updated_at: String::new(),
        extra: mosql::Object::new(),
    };
    let token = auth.sign_access(&user, "s", None);
    assert!(auth.decode(&token).is_ok());
    let later = better_auth_core::crypto::jwt_decode(&token, &auth.config.secret, auth.now() + 2)
        .unwrap_err();
    assert_eq!((later.status, later.code), (401, "EXPIRED_TOKEN"));
    // And through the middleware, which is what routes see.
    let req =
        Request::new("GET", "/api/auth/get-session").with_header("Authorization", "Bearer nope");
    assert_eq!(
        middleware::authenticate(&auth, &req).unwrap_err().status,
        401
    );
}

async_test! {
    async fn sign_up_sign_in_and_get_session() {
        let rig = Rig::new();
        let (user, access, refresh) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        assert_eq!(str_of(&user, "email"), "ada@x.co");
        assert_eq!(str_of(&user, "role"), "user");
        assert_eq!(user.get("emailVerified"), Some(&Value::Bool(false)));
        assert_eq!(user.get("password"), None, "never leak a hash");
        assert!(!refresh.is_empty() && refresh.len() >= 40);
        assert_ne!(access, refresh);

        // get-session: the shape the client plugin reads.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!(status, 200);
        assert_eq!(str_of(json.get("user").unwrap(), "id"), str_of(&user, "id"));
        let session = json.get("session").unwrap();
        assert!(session.get("token").is_none(), "no refresh digest in the session");
        assert!(str_of(session, "expiresAt").len() >= 20);
        assert!(str_of(session, "userAgent").is_empty());

        // Signed out: null, not an error.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", None, None).await;
        assert_eq!((status, json.clone()), (200, Value::Null));

        // Sign-in issues a second, independent session.
        let (status, _, response) = rig.sign_in_raw("ada@x.co", "correct horse battery staple").await;
        assert_eq!(status, 200);
        let access2 = Rig::header_token(&response).unwrap();
        assert_ne!(access2, access);

        // list-sessions shows both, marking the current one.
        let (status, json, _) = rig.call("GET", "/api/auth/list-sessions", Some(access2), None).await;
        assert_eq!(status, 200);
        let items = json.as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items.iter().filter(|s| s.get("isCurrent") == Some(&Value::Bool(true))).count(), 1);
        assert!(items.iter().all(|s| s.get("token").is_none()));

        // revoke-other-sessions leaves the caller one session.
        let (status, _, _) = rig.call("POST", "/api/auth/revoke-other-sessions", Some(access2), Some(Value::Null)).await;
        assert_eq!(status, 200);
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!((status, json.clone()), (200, Value::Null), "the other access token's session is gone");

        // sign-out ends the current session; repeating is harmless.
        let (status, json, _) = rig.call("POST", "/api/auth/sign-out", Some(access2), None).await;
        assert_eq!(status, 200);
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(access2), None).await;
        assert_eq!(json.clone(), Value::Null);
        let _ = status;
        let (status, _, _) = rig.call("POST", "/api/auth/sign-out", None, None).await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn refresh_rotates_and_rejects_replay() {
        let rig = Rig::new();
        let (_, _, refresh) = rig.sign_up("ada@x.co", "correct horse battery staple").await;

        // The refresh token is the bearer on /refresh-token.
        let (status, json, response) = rig.call("POST", "/api/auth/refresh-token", Some(&refresh), None).await;
        assert_eq!(status, 200, "{json:?}");
        let access = Rig::header_token(&response).unwrap().to_string();
        let refresh2 = str_of(&json, "refreshToken");
        assert_ne!(refresh2, refresh, "a refresh must rotate the token");
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
        assert!(!json.get("token").unwrap().as_str().unwrap().is_empty());

        // The new pair works.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!(status, 200);
        assert_ne!(json, Value::Null);

        // Replaying the old refresh token is refused, and did not kill
        // the live one.
        let (status, json, _) = rig.call("POST", "/api/auth/refresh-token", Some(&refresh), None).await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TOKEN");
        let (status, _, _) = rig.call("POST", "/api/auth/refresh-token", Some(&refresh2), None).await;
        assert_eq!(status, 200);

        // Garbage, and using an access token where a refresh is expected.
        let (status, _, _) = rig.call("POST", "/api/auth/refresh-token", Some("garbage"), None).await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.call("POST", "/api/auth/refresh-token", Some(&access), None).await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.call("POST", "/api/auth/refresh-token", None, None).await;
        assert_eq!(status, 401);

        // An access token is not usable as a refresh token, and a refresh
        // token is not an access token.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&refresh2), None).await;
        assert_eq!(json, Value::Null);
        let _ = status;
    }
}

async_test! {
    async fn revoked_sessions_stop_refreshing() {
        let rig = Rig::new();
        let (_, access, refresh) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let (status, _, _) = rig.call("POST", "/api/auth/sign-out", Some(&access), None).await;
        assert_eq!(status, 200);
        let (status, _, _) = rig.call("POST", "/api/auth/refresh-token", Some(&refresh), None).await;
        assert_eq!(status, 401, "sign-out must also end the refresh token");
    }
}

async_test! {
    async fn a_pending_second_factor_token_is_not_a_session() {
        let rig = Rig::new();
        let (user, _, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let pending = rig.auth.sign_pending(&str_of(&user, "id"));
        // Decodes, and says what it is.
        let claims = rig.auth.decode(&pending).unwrap();
        assert!(claims.is_two_factor_pending());
        assert_eq!(claims.sid, None);
        // Every normal route refuses it with the pending code; get-session
        // answers `null`, as it does for any caller without a session.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&pending), None).await;
        assert_eq!((status, json.clone()), (200, Value::Null));
        for (method, path) in [
            ("GET", "/api/auth/list-sessions"),
            ("POST", "/api/auth/update-user"),
            ("POST", "/api/auth/change-password"),
            ("GET", "/api/auth/admin/list-users"),
        ] {
            let req = Request::new(method, path)
                .with_header("Authorization", &format!("Bearer {pending}"))
                .with_json(obj(&[("name", Value::str("x"))]));
            let response = better_auth_core::routes::route(Arc::clone(&rig.auth), req).await;
            assert_eq!(response.status, 401, "{path}");
            assert!(String::from_utf8_lossy(&response.body).contains("TWO_FACTOR_PENDING"), "{path}");
        }
        // The pending token expires in five minutes.
        let claims = rig.auth.decode(&pending).unwrap();
        assert_eq!(claims.exp - claims.iat, 300);
    }
}

async_test! {
    async fn protected_routes_need_a_bearer() {
        let rig = Rig::new();
        for (method, path) in [
            ("GET", "/api/auth/list-sessions"),
            ("POST", "/api/auth/update-user"),
            ("POST", "/api/auth/change-password"),
            ("POST", "/api/auth/delete-user"),
            ("POST", "/api/auth/two-factor/enable"),
            ("GET", "/api/auth/admin/list-users"),
            ("GET", "/api/auth/list-accounts"),
        ] {
            let (status, json, _) = rig.call(method, path, None, Some(Value::Null)).await;
            assert_eq!(status, 401, "{path}: {json:?}");
            assert_eq!(str_of(&json, "code"), "UNAUTHORIZED");
            assert_eq!(str_of(&json, "message"), "missing bearer token");
        }
    }
}
