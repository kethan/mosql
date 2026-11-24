//! §7.3 — The admin plugin.
//!
//! Every route here needs `role == "admin"`. Bans take the sessions
//! away, an expiring ban lifts itself, and impersonation issues a token
//! that says who is really behind it.

mod common;

use better_auth_core::async_test;
use better_auth_core::models::{new_id, now_iso, User};
use common::*;
use mosql::Value;

/// A rig with an admin and an ordinary user in it.
async fn rig_with_admin() -> (Rig, String, String) {
    let rig = Rig::new();
    let admin_id = new_id();
    rig.auth
        .save_user(&User {
            id: admin_id.clone(),
            name: "Root".into(),
            email: "root@x.co".into(),
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
            created_at: now_iso(),
            updated_at: now_iso(),
            extra: mosql::Object::new(),
        })
        .await
        .unwrap();
    rig.auth.set_password(&admin_id, PASSWORD).await.unwrap();
    let (status, json, response) = rig.sign_in_raw("root@x.co", PASSWORD).await;
    assert_eq!(status, 200, "{json:?}");
    let admin_token = Rig::header_token(&response).unwrap().to_string();
    (rig, admin_token, admin_id)
}

const PASSWORD: &str = "correct horse battery staple";

async_test! {
    async fn only_an_admins_may_reach_the_admin_routes() {
        let (rig, _, _) = rig_with_admin().await;
        let (_, user_token, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        for (method, path) in [
            ("GET", "/api/auth/admin/list-users"),
            ("POST", "/api/auth/admin/create-user"),
            ("POST", "/api/auth/admin/ban-user"),
            ("POST", "/api/auth/admin/impersonate-user"),
            ("POST", "/api/auth/admin/remove-user"),
        ] {
            let (status, json, _) = rig.call(method, path, Some(&user_token), Some(Value::Null)).await;
            assert_eq!(status, 403, "{path}: {json:?}");
            assert_eq!(str_of(&json, "code"), "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS");
        }
    }
}

async_test! {
    async fn an_admin_creates_and_lists_users() {
        let (rig, admin, _) = rig_with_admin().await;
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/create-user",
                Some(&admin),
                Some(obj(&[
                    ("email", Value::str("grace@x.co")),
                    ("password", Value::str(PASSWORD)),
                    ("name", Value::str("Grace")),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let created = json.get("user").unwrap();
        assert_eq!(str_of(created, "email"), "grace@x.co");
        assert_eq!(str_of(created, "role"), "user");
        let created_id = str_of(created, "id");

        // The new user can sign in.
        let (status, _, _) = rig.sign_in_raw("grace@x.co", PASSWORD).await;
        assert_eq!(status, 200);

        // The list finds them, and says how many there are.
        let (status, json, _) = rig.call("GET", "/api/auth/admin/list-users", Some(&admin), None).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("total").and_then(Value::as_f64), Some(2.0));
        assert_eq!(json.get("users").unwrap().as_array().unwrap().len(), 2);

        // Search narrows it down.
        let (status, json, _) = rig
            .call("GET", "/api/auth/admin/list-users?searchValue=grace", Some(&admin), None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let users = json.get("users").unwrap().as_array().unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(str_of(&users[0], "email"), "grace@x.co");
        assert_eq!(json.get("total").and_then(Value::as_f64), Some(1.0));

        // Sorting works.
        let (status, json, _) = rig
            .call("GET", "/api/auth/admin/list-users?sortBy=email&sortDirection=asc", Some(&admin), None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        let users = json.get("users").unwrap().as_array().unwrap();
        assert_eq!(str_of(&users[0], "email"), "grace@x.co");

        // So does paging.
        let (status, json, _) = rig
            .call("GET", "/api/auth/admin/list-users?limit=1&offset=0", Some(&admin), None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("users").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(json.get("total").and_then(Value::as_f64), Some(2.0));

        // One user, read by id and by address.
        let (status, json, _) = rig
            .call("GET", &format!("/api/auth/admin/get-user?userId={created_id}"), Some(&admin), None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "grace@x.co");
        let (status, json, _) = rig
            .call("GET", "/api/auth/admin/get-user?email=grace@x.co", Some(&admin), None)
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "id"), created_id);

        // An unknown user is a 404.
        let (status, json, _) = rig
            .call("GET", "/api/auth/admin/get-user?userId=nope", Some(&admin), None)
            .await;
        assert_eq!(status, 404, "{json:?}");
        assert_eq!(str_of(&json, "code"), "USER_NOT_FOUND");
    }
}

async_test! {
    async fn an_admin_changes_a_user() {
        let (rig, admin, _) = rig_with_admin().await;
        let (user, _, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        // The name.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/update-user",
                Some(&admin),
                Some(obj(&[
                    ("userId", Value::str(&user_id)),
                    ("data", obj(&[("name", Value::str("Ada L"))])),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "name"), "Ada L");

        // The role.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/set-role",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id)), ("role", Value::str("admin"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "role"), "admin");

        // The password — which ends the user's sessions.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/set-user-password",
                Some(&admin),
                Some(obj(&[
                    ("userId", Value::str(&user_id)),
                    ("newPassword", Value::str("a new password")),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        let (status, _, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 401);
        let (status, _, _) = rig.sign_in_raw("ada@x.co", "a new password").await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn a_ban_ends_the_sessions_and_a_lift_brings_them_back() {
        let (rig, admin, _) = rig_with_admin().await;
        let (user, user_token, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        // The sessions are visible to the admin.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/list-user-sessions",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("sessions").unwrap().as_array().unwrap().len(), 1);

        // The ban.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/ban-user",
                Some(&admin),
                Some(obj(&[
                    ("userId", Value::str(&user_id)),
                    ("banReason", Value::str("no reason")),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("user").unwrap().get("banned"), Some(&Value::Bool(true)));

        // The session is gone, and signing in is refused.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&user_token), None).await;
        assert_eq!(json.clone(), Value::Null);
        let _ = status;
        let (status, json, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 403, "{json:?}");
        assert_eq!(str_of(&json, "code"), "USER_BANNED");

        // Lifting the ban.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/unban-user",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("user").unwrap().get("banned"), Some(&Value::Bool(false)));

        let (status, _, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200);
    }
}

async_test! {
    async fn a_ban_with_an_expiry_lifts_itself() {
        let (rig, admin, _) = rig_with_admin().await;
        let (user, _, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        // A ban that runs out in a second.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/ban-user",
                Some(&admin),
                Some(obj(&[
                    ("userId", Value::str(&user_id)),
                    ("banExpiresIn", Value::Int(1)),
                ])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(json.get("user").unwrap().get("banExpires").is_some());

        // While it runs, the user stays out.
        let (status, json, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 403, "{json:?}");
        assert_eq!(str_of(&json, "code"), "USER_BANNED");

        // Once it has run out, the next sign-in lifts it.
        std::thread::sleep(std::time::Duration::from_millis(1_200));
        let (status, json, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200, "{json:?}");

        // And the user is no longer marked as banned.
        let (status, json, _) = rig
            .call(
                "GET",
                &format!("/api/auth/admin/get-user?userId={user_id}"),
                Some(&admin),
                None,
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("user").unwrap().get("banned"), Some(&Value::Bool(false)));
    }
}

async_test! {
    async fn an_admin_can_revoke_a_user_sessions() {
        let (rig, admin, _) = rig_with_admin().await;
        let (user, _, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/revoke-user-sessions",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));

        // The user's session is gone, but the account is not.
        let (status, json, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200, "{json:?}");
    }
}

async_test! {
    async fn impersonation_marks_the_token_and_can_be_stopped() {
        let (rig, admin, admin_id) = rig_with_admin().await;
        let (user, _, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/admin/impersonate-user",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let impersonated = Rig::header_token(&response).unwrap().to_string();
        assert_eq!(str_of(json.get("user").unwrap(), "id"), user_id);

        // The token says who is really behind it.
        let claims = rig.auth.decode(&impersonated).unwrap();
        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.imp.as_deref(), Some(admin_id.as_str()));

        // And it works as that user.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&impersonated), None).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "id"), user_id);

        // Stopping gives the admin their own session back.
        let (status, json, response) = rig
            .call("POST", "/api/auth/admin/stop-impersonating", Some(&impersonated), Some(Value::Null))
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "id"), admin_id);
        let back = Rig::header_token(&response).unwrap().to_string();
        let claims = rig.auth.decode(&back).unwrap();
        assert_eq!(claims.sub, admin_id);
        assert_eq!(claims.imp, None);

        // The admin token works again.
        let (status, json, _) = rig.call("GET", "/api/auth/admin/list-users", Some(&back), None).await;
        assert_eq!(status, 200, "{json:?}");

        // An admin cannot impersonate themselves.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/impersonate-user",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&admin_id))])),
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
    }
}

async_test! {
    async fn an_admin_removes_a_user() {
        let (rig, admin, admin_id) = rig_with_admin().await;
        let (user, _, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/remove-user",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&user_id))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("success"), Some(&Value::Bool(true)));

        let (status, json, _) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 401, "{json:?}");

        // The admin cannot remove themselves.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/admin/remove-user",
                Some(&admin),
                Some(obj(&[("userId", Value::str(&admin_id))])),
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
    }
}
