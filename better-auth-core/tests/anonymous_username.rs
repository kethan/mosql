//! §7.3 — Anonymous sign-in, and the upgrade to a real account.
//!
//! An anonymous visitor keeps one user id for their whole life: signing
//! up with an email, a one-time code or a magic link turns that same row
//! into a real account, so nothing they did is lost.

mod common;

use better_auth_core::async_test;
use common::*;
use mosql::Value;
use std::sync::Arc;

const PASSWORD: &str = "correct horse battery staple";

async_test! {
    async fn anonymous_sign_in_creates_a_placeholder_account() {
        let rig = Rig::new();
        let (status, json, response) = rig.call("POST", "/api/auth/sign-in/anonymous", None, Some(Value::Null)).await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(user.get("isAnonymous"), Some(&Value::Bool(true)));
        let email = str_of(user, "email");
        assert!(email.ends_with("@anonymous.local"), "{email}");
        assert!(Rig::header_token(&response).is_some());

        // The token says so too.
        let access = Rig::header_token(&response).unwrap();
        let claims = rig.auth.decode(access).unwrap();
        assert!(claims.anon);

        // A second anonymous sign-in continues the same user.
        let (status, json, _) = rig.call("POST", "/api/auth/sign-in/anonymous", Some(access), Some(Value::Null)).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "id"), str_of(user, "id"));
    }
}

async_test! {
    async fn signing_up_keeps_the_anonymous_id() {
        let rig = Rig::new();
        let (_, json, response) = rig.call("POST", "/api/auth/sign-in/anonymous", None, Some(Value::Null)).await;
        let anon_id = str_of(json.get("user").unwrap(), "id");
        let access = Rig::header_token(&response).unwrap().to_string();

        // Signing up with an email while anonymous.
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/sign-up/email",
                Some(&access),
                Some(obj(&[
                    ("name", Value::str("Ada")),
                    ("email", Value::str("ada@x.co")),
                    ("password", Value::str(PASSWORD)),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "id"), anon_id, "the same user id, upgraded");
        assert_eq!(user.get("isAnonymous"), Some(&Value::Bool(false)));
        assert_eq!(str_of(user, "email"), "ada@x.co");

        // The token handed back with the upgrade says so; the old one
        // still says what it said when it was minted.
        let claims = rig.auth.decode(&access).unwrap();
        assert!(claims.anon, "the old token is untouched");
        let fresh = rig.auth.decode(Rig::header_token(&response).unwrap()).unwrap();
        assert!(!fresh.anon, "the new token is not anonymous");

        // And the account signs in normally.
        let (status, _, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn a_one_time_code_also_upgrades_in_place() {
        let rig = Rig::new();
        let (_, json, response) = rig.call("POST", "/api/auth/sign-in/anonymous", None, Some(Value::Null)).await;
        let anon_id = str_of(json.get("user").unwrap(), "id");
        let access = Rig::header_token(&response).unwrap().to_string();

        rig.call(
            "POST",
            "/api/auth/email-otp/send-verification-otp",
            None,
            Some(obj(&[("email", Value::str("ada@x.co")), ("type", Value::str("sign_in"))])),
        )
        .await;
        let code = {
            let mails = rig.mailer.sent.lock().unwrap();
            let last = mails.last().unwrap();
            last.2
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()))
                .rfind(|w| w.len() == 6 && !w.is_empty())
                .unwrap()
                .to_string()
        };

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                Some(&access),
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "id"), anon_id, "the same user id, upgraded");
        assert_eq!(user.get("isAnonymous"), Some(&Value::Bool(false)));
        assert_eq!(str_of(user, "email"), "ada@x.co");
    }
}

async_test! {
    async fn a_magic_link_also_upgrades_in_place() {
        let rig = Rig::new();
        let (_, json, response) = rig.call("POST", "/api/auth/sign-in/anonymous", None, Some(Value::Null)).await;
        let anon_id = str_of(json.get("user").unwrap(), "id");
        let access = Rig::header_token(&response).unwrap().to_string();

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/magic-link",
                Some(&access),
                Some(obj(&[("email", Value::str("ada@x.co"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let token = rig.links()[0].split("token=").nth(1).unwrap().to_string();

        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/magic-link/verify?token={token}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "id"), anon_id, "the same user id, upgraded");
        assert_eq!(user.get("isAnonymous"), Some(&Value::Bool(false)));
    }
}

async_test! {
    async fn username_sign_in() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/update-user",
                Some(&access),
                Some(obj(&[("username", Value::str("ada.l"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");

        // The username is free.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/is-username-available",
                None,
                Some(obj(&[("username", Value::str("grace.h"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("available"), Some(&Value::Bool(true)));

        // This one is not.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/is-username-available",
                None,
                Some(obj(&[("username", Value::str("ada.l"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("available"), Some(&Value::Bool(false)));

        // Signing in with it.
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/sign-in/username",
                None,
                Some(obj(&[
                    ("username", Value::str("ada.l")),
                    ("password", Value::str(PASSWORD)),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(Rig::header_token(&response).is_some());
        assert_eq!(str_of(json.get("user").unwrap(), "username"), "ada.l");

        // A wrong password is refused, with the username wording.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/username",
                None,
                Some(obj(&[
                    ("username", Value::str("ada.l")),
                    ("password", Value::str("wrong")),
                ])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_USERNAME_OR_PASSWORD");

        // An unknown username is refused the same way.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/username",
                None,
                Some(obj(&[
                    ("username", Value::str("nobody")),
                    ("password", Value::str(PASSWORD)),
                ])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_USERNAME_OR_PASSWORD");
    }
}

async_test! {
    async fn anonymous_sign_in_can_be_switched_off() {
        let mut config = better_auth_core::auth::AuthConfig::new("https://app.test", SECRET);
        config.pbkdf2_iterations = 1_000;
        config.allow_anonymous = false;
        let auth = Arc::new(better_auth_core::auth::Auth::new(
            Arc::new(better_auth_core::store::MemoryStore::new()),
            config,
        ));
        let mut req = better_auth_core::http::Request::new("POST", "/api/auth/sign-in/anonymous");
        req = req.with_json(Value::Null);
        let response = better_auth_core::routes::route(auth, req).await;
        assert_eq!(response.status, 400);
        let body = mosql::parse_json(&String::from_utf8_lossy(&response.body)).unwrap();
        assert_eq!(str_of(&body, "code"), "ANONYMOUS_DISABLED");
    }
}
