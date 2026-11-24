//! §7.3 — Email: verification, password reset, password and address
//! changes, and account deletion.
//!
//! Tokens are handed out as opaque strings and stored hashed; the suites
//! below read them out of the recorded mail and use them exactly as a
//! browser would.

mod common;

use better_auth_core::async_test;
use common::*;
use mosql::Value;

/// The token of the last link in the mail box.
fn last_token(rig: &Rig, marker: &str) -> String {
    let links = rig.links();
    let link = links
        .iter()
        .rev()
        .find(|l| l.contains(marker))
        .unwrap_or_else(|| panic!("no link with {marker} in {links:?}"));
    link.split(['?', '&'])
        .find_map(|part| {
            part.strip_prefix("token=")
                .or_else(|| part.strip_prefix("/reset-password/"))
        })
        .expect("a token in the link")
        .to_string()
}

async_test! {
    async fn sign_up_sends_a_verification_link() {
        let rig = Rig::new();
        let (user, _, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        assert_eq!(user.get("emailVerified"), Some(&Value::Bool(false)));
        let links = rig.links();
        assert_eq!(links.len(), 1, "one verification mail: {links:?}");
        assert!(links[0].starts_with("https://app.test/api/auth/verify-email?token="));

        // Verifying marks the address and hands out a session.
        let token = last_token(&rig, "verify-email");
        let (status, json, response) = rig.call("GET", &format!("/api/auth/verify-email?token={token}"), None, None).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("user").unwrap().get("emailVerified"), Some(&Value::Bool(true)));
        let access = Rig::header_token(&response).unwrap();
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(access), None).await;
        assert_eq!(status, 200);
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");

        // The token is spent: a second use is refused.
        let (status, json, _) = rig.call("GET", &format!("/api/auth/verify-email?token={token}"), None, None).await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TOKEN");
    }
}

async_test! {
    async fn verification_can_redirect_to_the_app() {
        let rig = Rig::new();
        rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let token = last_token(&rig, "verify-email");
        let (status, _, response) = rig
            .call(
                "GET",
                &format!("/api/auth/verify-email?token={token}&callbackURL=https://portal.example.com/done"),
                None,
                None,
            )
            .await;
        assert_eq!(status, 302);
        let location = Rig::location(&response).unwrap();
        assert!(location.starts_with("https://portal.example.com/done?"), "{location}");
        assert!(location.contains("token="), "{location}");
        assert!(location.contains("refreshToken="), "{location}");
    }
}

async_test! {
    async fn verification_is_optional_when_the_server_does_not_require_it() {
        let rig = Rig::new();
        // The default server signs in right away.
        let (status, json, _) = rig.sign_in_raw("ada@x.co", "nope").await;
        assert_eq!(status, 401);
        assert_eq!(str_of(&json, "code"), "INVALID_EMAIL_OR_PASSWORD");
        let (user, _, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        assert_eq!(user.get("emailVerified"), Some(&Value::Bool(false)));
        let (status, _, response) = rig.sign_in_raw("ada@x.co", "correct horse battery staple").await;
        assert_eq!(status, 200);
        assert!(Rig::header_token(&response).is_some());
    }
}

async_test! {
    async fn request_password_reset_is_always_the_same_answer() {
        let rig = Rig::new();
        rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let body = obj(&[("email", Value::str("ada@x.co"))]);
        let (status, json, _) = rig.call("POST", "/api/auth/request-password-reset", None, Some(body)).await;
        assert_eq!(status, 200);
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        assert_eq!(
            str_of(&json, "message"),
            "If this email exists in our system, check your email for the reset link"
        );

        // An address that does not exist gets the same answer, and no mail.
        let before = rig.links().len();
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/request-password-reset",
                None,
                Some(obj(&[("email", Value::str("nobody@x.co"))])),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        assert_eq!(rig.links().len(), before, "no mail for an unknown address");

        // The alias behaves the same way.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/forget-password",
                None,
                Some(obj(&[("email", Value::str("ada@x.co"))])),
            )
            .await;
        assert_eq!(status, 200);
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
    }
}

async_test! {
    async fn reset_password_with_the_mailed_token() {
        let rig = Rig::new();
        rig.sign_up("ada@x.co", "correct horse battery staple").await;
        rig.call(
            "POST",
            "/api/auth/request-password-reset",
            None,
            Some(obj(&[("email", Value::str("ada@x.co"))])),
        )
        .await;
        let links = rig.links();
        let reset_link = links
            .iter()
            .rev()
            .find(|l| l.contains("/reset-password/"))
            .expect("a reset link")
            .clone();
        let token = reset_link
            .rsplit('/')
            .next()
            .expect("the token at the end of the link")
            .to_string();

        // The GET form redirects to the app with the token in the query.
        let (status, _, response) = rig.call("GET", &format!("/api/auth/reset-password/{token}"), None, None).await;
        assert_eq!(status, 302);
        let location = Rig::location(&response).unwrap();
        assert!(location.contains(&format!("token={token}")), "{location}");

        // The POST form sets the new password.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/reset-password",
                None,
                Some(obj(&[("newPassword", Value::str("a brand new password")), ("token", Value::str(token.clone()))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));

        // The old password is gone; the new one works.
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "correct horse battery staple").await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "a brand new password").await;
        assert_eq!(status, 200);

        // The token is single use.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/reset-password",
                None,
                Some(obj(&[("newPassword", Value::str("another new password")), ("token", Value::str(token))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
    }
}

async_test! {
    async fn change_password_rotates_the_session() {
        let rig = Rig::new();
        let (user, access, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/change-password",
                Some(&access),
                Some(obj(&[
                    ("currentPassword", Value::str("correct horse battery staple")),
                    ("newPassword", Value::str("a newer password")),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        // A fresh pair comes back.
        let new_access = Rig::header_token(&response).unwrap().to_string();
        assert!(!new_access.is_empty());
        assert!(json.get("refreshToken").is_some());
        assert_eq!(str_of(json.get("user").unwrap(), "id"), str_of(&user, "id"));

        // The wrong current password is refused.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/change-password",
                Some(&access),
                Some(obj(&[
                    ("currentPassword", Value::str("wrong")),
                    ("newPassword", Value::str("yet another password")),
                ])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_PASSWORD");

        // A short password is refused.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/change-password",
                Some(&access),
                Some(obj(&[
                    ("currentPassword", Value::str("a newer password")),
                    ("newPassword", Value::str("short")),
                ])),
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
    }
}

async_test! {
    async fn change_email_needs_a_confirmation() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/change-email",
                Some(&access),
                Some(obj(&[("newEmail", Value::str("ada2@x.co"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));

        // Nothing changed yet.
        let (_, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");

        // The mailed link confirms it.
        let token = last_token(&rig, "verify-email");
        let (status, json, _) = rig.call("GET", &format!("/api/auth/verify-email?token={token}"), None, None).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada2@x.co");

        // The old address no longer signs in.
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "correct horse battery staple").await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.sign_in_raw("ada2@x.co", "correct horse battery staple").await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn delete_user_needs_the_password() {
        let rig = Rig::new();
        let (user, access, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let (status, json, _) = rig.call("POST", "/api/auth/delete-user", Some(&access), Some(Value::Null)).await;
        assert_eq!(status, 400, "{json:?}");

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/delete-user",
                Some(&access),
                Some(obj(&[("password", Value::str("wrong"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_PASSWORD");

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/delete-user",
                Some(&access),
                Some(obj(&[("password", Value::str("correct horse battery staple"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));
        assert_eq!(str_of(&json, "message"), "User deleted");

        // The account is gone.
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "correct horse battery staple").await;
        assert_eq!(status, 401);
        let _ = user;
    }
}

async_test! {
    async fn update_user_changes_the_profile() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", "correct horse battery staple").await;
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/update-user",
                Some(&access),
                Some(obj(&[("name", Value::str("Ada L")), ("image", Value::str("https://x.co/a.png"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        let user = json.get("user").unwrap();
        assert_eq!(str_of(user, "name"), "Ada L");
        assert_eq!(str_of(user, "image"), "https://x.co/a.png");

        // A username is taken into account.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/update-user",
                Some(&access),
                Some(obj(&[("username", Value::str("ada.l"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "username"), "ada.l");

        // An invalid username is refused.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/update-user",
                Some(&access),
                Some(obj(&[("username", Value::str("no spaces here"))])),
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
    }
}
