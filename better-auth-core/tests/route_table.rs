//! The route table, walked end to end.
//!
//! Every path the specification names must be served, and nothing else
//! may be: a typo in a path should be a 404 that says so, not a silent
//! 405.

mod common;

use better_auth_core::async_test;
use common::*;
use mosql::Value;

/// Every route, in the order the specification lists them.
fn routes() -> Vec<(&'static str, &'static str)> {
    vec![
        // ---- core -------------------------------------------------------
        ("GET", "/ok"),
        ("GET", "/error"),
        ("GET", "/get-session"),
        ("GET", "/list-sessions"),
        ("POST", "/revoke-session"),
        ("POST", "/revoke-sessions"),
        ("POST", "/revoke-other-sessions"),
        ("POST", "/sign-up/email"),
        ("POST", "/sign-in/email"),
        ("POST", "/sign-out"),
        ("POST", "/update-user"),
        ("POST", "/change-password"),
        ("POST", "/set-password"),
        ("POST", "/change-email"),
        ("POST", "/delete-user"),
        ("GET", "/verify-email"),
        ("POST", "/send-verification-email"),
        ("POST", "/request-password-reset"),
        ("POST", "/forget-password"),
        ("POST", "/reset-password"),
        ("GET", "/reset-password/some-token"),
        // ---- bearer -----------------------------------------------------
        ("POST", "/refresh-token"),
        // ---- username ---------------------------------------------------
        ("POST", "/sign-in/username"),
        ("POST", "/is-username-available"),
        // ---- anonymous --------------------------------------------------
        ("POST", "/sign-in/anonymous"),
        // ---- magic link -------------------------------------------------
        ("POST", "/sign-in/magic-link"),
        ("GET", "/magic-link/verify?token=x"),
        // ---- email OTP --------------------------------------------------
        ("POST", "/email-otp/send-verification-otp"),
        ("POST", "/email-otp/check-verification-otp"),
        ("POST", "/email-otp/verify-email"),
        ("POST", "/sign-in/email-otp"),
        ("POST", "/email-otp/reset-password"),
        // ---- OAuth ------------------------------------------------------
        ("POST", "/sign-in/social"),
        ("POST", "/sign-in/oauth2"),
        ("POST", "/oauth2/link"),
        ("GET", "/callback/google"),
        ("POST", "/callback/google"),
        ("GET", "/oauth2/callback/google"),
        ("POST", "/oauth2/callback/google"),
        ("GET", "/list-accounts"),
        ("POST", "/link-social-account"),
        ("POST", "/unlink-account"),
        // ---- two factor -------------------------------------------------
        ("POST", "/two-factor/enable"),
        ("POST", "/two-factor/disable"),
        ("POST", "/two-factor/get-totp-uri"),
        ("POST", "/two-factor/send-otp"),
        ("POST", "/two-factor/verify-otp"),
        ("POST", "/two-factor/verify-totp"),
        ("POST", "/two-factor/verify-backup-code"),
        ("POST", "/two-factor/generate-backup-codes"),
        // ---- admin ------------------------------------------------------
        ("POST", "/admin/create-user"),
        ("GET", "/admin/list-users"),
        ("GET", "/admin/get-user"),
        ("POST", "/admin/update-user"),
        ("POST", "/admin/set-role"),
        ("POST", "/admin/set-user-password"),
        ("POST", "/admin/ban-user"),
        ("POST", "/admin/unban-user"),
        ("POST", "/admin/list-user-sessions"),
        ("POST", "/admin/revoke-user-session"),
        ("POST", "/admin/revoke-user-sessions"),
        ("POST", "/admin/impersonate-user"),
        ("POST", "/admin/stop-impersonating"),
        ("POST", "/admin/remove-user"),
    ]
}

async_test! {
    async fn every_route_in_the_specification_is_served() {
        let rig = Rig::new();
        for (method, path) in routes() {
            let (status, json, _) = rig.call(method, &format!("/api/auth{path}"), None, Some(Value::Null)).await;
            // Not "this path does not exist": the route is there. (A
            // route may still answer 404 for its own reasons — an
            // unknown provider, say.)
            assert_ne!(
                str_of(&json, "code"),
                "NOT_FOUND",
                "{method} {path} is not routed: {json:?}"
            );
            let _ = status;
        }
    }
}

async_test! {
    async fn a_path_that_is_not_in_the_list_is_a_404() {
        let rig = Rig::new();
        for path in [
            "/api/auth/nope",
            "/api/auth/sign-in/nope",
            "/api/auth/admin/nope",
            "/api/nope",
            "/",
        ] {
            let (status, json, _) = rig.call("GET", path, None, None).await;
            assert_eq!(status, 404, "{path}: {json:?}");
            assert_eq!(str_of(&json, "code"), "NOT_FOUND");
        }
    }
}

async_test! {
    async fn the_wrong_method_is_a_405() {
        let rig = Rig::new();
        // The path is there, the method is not.
        let (status, json, _) = rig.call("DELETE", "/api/auth/sign-up/email", None, Some(Value::Null)).await;
        assert_eq!(status, 405, "{json:?}");
        assert_eq!(str_of(&json, "code"), "METHOD_NOT_ALLOWED");
    }
}

async_test! {
    async fn the_ok_route_says_ok() {
        let rig = Rig::new();
        let (status, json, _) = rig.call("GET", "/api/auth/ok", None, None).await;
        assert_eq!(status, 200);
        assert_eq!(json.get("ok"), Some(&Value::Bool(true)));
    }
}
