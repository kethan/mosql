//! §7.3 — One-time codes and magic links.
//!
//! A code is six digits and lives five minutes; five wrong tries lock the
//! address out until a new code is requested. A magic link is 32 random
//! bytes, stored hashed, and works exactly once.

mod common;

use better_auth_core::async_test;
use common::*;
use mosql::Value;

const PASSWORD: &str = "correct horse battery staple";

/// The six-digit code in the last mail.
fn last_code(rig: &Rig) -> String {
    let mails = rig.mailer.sent.lock().unwrap();
    let last = mails.last().expect("a mail");
    last.2
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()))
        .rfind(|w| w.len() == 6 && !w.is_empty())
        .expect("a six digit code")
        .to_string()
}

async_test! {
    async fn email_otp_signs_in_and_creates_the_user() {
        let rig = Rig::new();
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/email-otp/send-verification-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("type", Value::str("sign_in"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));

        let code = last_code(&rig);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));

        // The check route does not consume the code.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/email-otp/check-verification-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code)), ("type", Value::str("sign_in"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));

        // Signing in with the code creates the account and returns tokens.
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let access = Rig::header_token(&response).unwrap().to_string();
        assert!(json.get("refreshToken").is_some());
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
        assert_eq!(json.get("user").unwrap().get("emailVerified"), Some(&Value::Bool(true)));

        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!(status, 200);
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");

        // The code is spent.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_OTP");
    }
}

async_test! {
    async fn email_otp_locks_out_after_five_tries() {
        let rig = Rig::new();
        rig.call(
            "POST",
            "/api/auth/email-otp/send-verification-otp",
            None,
            Some(obj(&[("email", Value::str("ada@x.co")), ("type", Value::str("sign_in"))])),
        )
        .await;
        let code = last_code(&rig);

        // Four wrong tries: still just wrong.
        for _ in 0..4 {
            let (status, json, _) = rig
                .call(
                    "POST",
                    "/api/auth/sign-in/email-otp",
                    None,
                    Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str("000000"))])),
                )
                .await;
            assert_eq!(status, 401, "{json:?}");
            assert_eq!(str_of(&json, "code"), "INVALID_OTP");
        }
        // The fifth wrong try locks the address out.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str("000000"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "TOO_MANY_ATTEMPTS");

        // Even the right code does not work now.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_OTP");

        // A fresh code opens the door again.
        rig.call(
            "POST",
            "/api/auth/email-otp/send-verification-otp",
            None,
            Some(obj(&[("email", Value::str("ada@x.co")), ("type", Value::str("sign_in"))])),
        )
        .await;
        let fresh = last_code(&rig);
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/email-otp",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&fresh))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
    }
}

async_test! {
    async fn email_otp_verifies_an_address() {
        let rig = Rig::new();
        rig.sign_up("ada@x.co", PASSWORD).await;
        rig.call(
            "POST",
            "/api/auth/email-otp/send-verification-otp",
            None,
            Some(obj(&[("email", Value::str("ada@x.co"))])),
        )
        .await;
        let code = last_code(&rig);
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/email-otp/verify-email",
                None,
                Some(obj(&[("email", Value::str("ada@x.co")), ("otp", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        assert!(json.get("token").is_some());
        assert_eq!(json.get("user").unwrap().get("emailVerified"), Some(&Value::Bool(true)));
        let _ = response;
    }
}

async_test! {
    async fn email_otp_resets_a_password() {
        let rig = Rig::new();
        rig.sign_up("ada@x.co", PASSWORD).await;
        rig.call(
            "POST",
            "/api/auth/email-otp/send-verification-otp",
            None,
            Some(obj(&[("email", Value::str("ada@x.co")), ("type", Value::str("forget_password"))])),
        )
        .await;
        let code = last_code(&rig);
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/email-otp/reset-password",
                None,
                Some(obj(&[
                    ("email", Value::str("ada@x.co")),
                    ("otp", Value::str(&code)),
                    ("password", Value::str("a fresh password")),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));

        let (status, _, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "a fresh password").await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn magic_link_signs_in_once() {
        let rig = Rig::new();
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/magic-link",
                None,
                Some(obj(&[("email", Value::str("ada@x.co"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));

        let links = rig.links();
        assert_eq!(links.len(), 1);
        assert!(links[0].starts_with("https://app.test/api/auth/magic-link/verify?token="));
        let token = links[0].split("token=").nth(1).unwrap().to_string();
        assert!(token.len() >= 32, "the token is long: {}", token.len());

        // Following the link signs the reader in.
        let (status, json, response) = rig
            .call("GET", &format!("/api/auth/magic-link/verify?token={token}"), None, None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(json.get("token").is_some());
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
        assert_eq!(json.get("user").unwrap().get("emailVerified"), Some(&Value::Bool(true)));
        assert!(Rig::header_token(&response).is_some());

        // A second follow is refused: the link is spent.
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/magic-link/verify?token={token}"), None, None)
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TOKEN");
    }
}

async_test! {
    async fn magic_link_redirects_to_the_app() {
        let rig = Rig::new();
        rig.call(
            "POST",
            "/api/auth/sign-in/magic-link",
            None,
            Some(obj(&[
                ("email", Value::str("ada@x.co")),
                ("callbackURL", Value::str("https://portal.example.com/after")),
            ])),
        )
        .await;
        let token = rig.links()[0].split("token=").nth(1).unwrap().to_string();
        let (status, _, response) = rig
            .call("GET", &format!("/api/auth/magic-link/verify?token={token}"), None, None)
            .await;
        assert_eq!(status, 302);
        let location = Rig::location(&response).unwrap();
        assert!(location.starts_with("https://portal.example.com/after?"), "{location}");
        assert!(location.contains("token="), "{location}");
        assert!(location.contains("refreshToken="), "{location}");
    }
}

async_test! {
    async fn an_unknown_magic_link_is_refused() {
        let rig = Rig::new();
        let (status, json, _) = rig
            .call("GET", "/api/auth/magic-link/verify?token=nope", None, None)
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TOKEN");
    }
}
