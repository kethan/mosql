//! The route table, and the small helpers every route module shares.
//!
//! `route` is the single entry point: strip the mount prefix, split the
//! path, and dispatch on `(method, segments)`. Unknown paths get a 404 in
//! the usual `{code, message}` shape, so a client never sees a bare
//! transport error from this API.

use std::sync::Arc;

use mosql::Value;

use crate::auth::{Auth, PATH_PREFIX};
use crate::error::{AuthError, AuthResult};
use crate::http::Request;
use crate::middleware::cors;
use crate::models::{now_iso, Record, Session};
use crate::object;
use crate::store::{AuthStoreExt, FindManyQuery, WhereClause};

pub mod admin;
pub mod core;
pub mod email_otp;
pub mod magic_link;
pub mod oauth;
pub mod two_factor;
pub mod username;

pub mod anonymous;

/// Run the route table synchronously on the calling thread, for the
/// `std::net` server whose handler is a plain function.
pub fn respond(auth: Arc<Auth>, req: Request) -> crate::http::Response {
    uflite::db::block_on(route(auth, req))
}

/// Answer one request, CORS headers included.
pub async fn route(auth: Arc<Auth>, req: Request) -> crate::http::Response {
    let response = dispatch(&auth, &req).await;
    cors(&auth, &req, response)
}

async fn dispatch(auth: &Arc<Auth>, req: &Request) -> crate::http::Response {
    let prefix = format!("{}/", PATH_PREFIX.trim_end_matches('/'));
    let Some(rest) = req.path.strip_prefix(&prefix) else {
        return crate::http::Response::error(&not_found(&req.path));
    };
    let segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let method = req.method.as_str();
    let result: AuthResult<crate::http::Response> = match (method, segments.as_slice()) {
        // ---- core -------------------------------------------------------
        ("GET", ["ok"]) => Ok(core::ok()),
        ("GET", ["error"]) => Err(AuthError::internal("Something went wrong")),
        ("GET", ["get-session"]) => core::get_session(auth, req).await,
        ("GET", ["list-sessions"]) => core::list_sessions(auth, req).await,
        ("POST", ["revoke-session"]) => core::revoke_session(auth, req).await,
        ("POST", ["revoke-sessions"]) => core::revoke_sessions(auth, req).await,
        ("POST", ["revoke-other-sessions"]) => core::revoke_other_sessions(auth, req).await,
        ("POST", ["sign-up", "email"]) => core::sign_up_email(auth, req).await,
        ("POST", ["sign-in", "email"]) => core::sign_in_email(auth, req).await,
        ("POST", ["sign-out"]) => core::sign_out(auth, req).await,
        ("POST", ["update-user"]) => core::update_user(auth, req).await,
        ("POST", ["change-password"]) => core::change_password(auth, req).await,
        ("POST", ["set-password"]) => core::set_password(auth, req).await,
        ("POST", ["change-email"]) => core::change_email(auth, req).await,
        ("POST", ["delete-user"]) => core::delete_user(auth, req).await,
        ("GET", ["verify-email"]) => core::verify_email(auth, req).await,
        ("POST", ["send-verification-email"]) => core::send_verification_email(auth, req).await,
        ("POST", ["request-password-reset"] | ["forget-password"]) => {
            core::request_password_reset(auth, req).await
        }
        ("POST", ["reset-password"]) => core::reset_password(auth, req).await,
        ("GET", ["reset-password", token]) => core::reset_password_link(auth, req, token).await,
        // ---- bearer -----------------------------------------------------
        ("POST", ["refresh-token"]) => core::refresh_token(auth, req).await,
        // ---- username ---------------------------------------------------
        ("POST", ["is-username-available"]) => username::is_available(auth, req).await,
        ("POST", ["sign-in", "username"]) => username::sign_in(auth, req).await,
        // ---- anonymous --------------------------------------------------
        ("POST", ["sign-in", "anonymous"]) => anonymous::sign_in(auth, req).await,
        // ---- magic link -------------------------------------------------
        ("POST", ["sign-in", "magic-link"]) => magic_link::send(auth, req).await,
        ("GET", ["magic-link", "verify"]) => magic_link::verify(auth, req).await,
        // ---- email OTP --------------------------------------------------
        ("POST", ["email-otp", "send-verification-otp"]) => email_otp::send(auth, req).await,
        ("POST", ["email-otp", "check-verification-otp"]) => email_otp::check(auth, req).await,
        ("POST", ["email-otp", "verify-email"]) => email_otp::verify_email(auth, req).await,
        ("POST", ["sign-in", "email-otp"]) => email_otp::sign_in(auth, req).await,
        ("POST", ["email-otp", "reset-password"]) => email_otp::reset_password(auth, req).await,
        // ---- OAuth ------------------------------------------------------
        ("POST", ["sign-in", "social"] | ["sign-in", "oauth2"]) => oauth::sign_in(auth, req).await,
        ("POST", ["oauth2", "link"]) => oauth::link(auth, req).await,
        ("GET", ["callback", provider] | ["oauth2", "callback", provider]) => {
            oauth::callback(auth, req, provider).await
        }
        ("POST", ["callback", provider] | ["oauth2", "callback", provider]) => {
            oauth::callback(auth, req, provider).await
        }
        ("GET", ["list-accounts"]) => oauth::list_accounts(auth, req).await,
        ("POST", ["link-social-account"]) => oauth::sign_in(auth, req).await,
        ("POST", ["unlink-account"]) => oauth::unlink_account(auth, req).await,
        // ---- two factor -------------------------------------------------
        ("POST", ["two-factor", "enable"]) => two_factor::enable(auth, req).await,
        ("POST", ["two-factor", "disable"]) => two_factor::disable(auth, req).await,
        ("POST", ["two-factor", "get-totp-uri"]) => two_factor::get_totp_uri(auth, req).await,
        ("POST", ["two-factor", "send-otp"]) => two_factor::send_otp(auth, req).await,
        ("POST", ["two-factor", "verify-otp"]) => two_factor::verify_otp(auth, req).await,
        ("POST", ["two-factor", "verify-totp"]) => two_factor::verify_totp(auth, req).await,
        ("POST", ["two-factor", "verify-backup-code"]) => {
            two_factor::verify_backup_code(auth, req).await
        }
        ("POST", ["two-factor", "generate-backup-codes"]) => {
            two_factor::generate_backup_codes(auth, req).await
        }
        // ---- admin ------------------------------------------------------
        ("POST", ["admin", "create-user"]) => admin::create_user(auth, req).await,
        ("GET", ["admin", "list-users"]) => admin::list_users(auth, req).await,
        ("GET", ["admin", "get-user"]) => admin::get_user(auth, req).await,
        ("POST", ["admin", "update-user"]) => admin::update_user(auth, req).await,
        ("POST", ["admin", "set-role"]) => admin::set_role(auth, req).await,
        ("POST", ["admin", "set-user-password"]) => admin::set_user_password(auth, req).await,
        ("POST", ["admin", "ban-user"]) => admin::ban_user(auth, req).await,
        ("POST", ["admin", "unban-user"]) => admin::unban_user(auth, req).await,
        ("POST", ["admin", "list-user-sessions"]) => admin::list_user_sessions(auth, req).await,
        ("POST", ["admin", "revoke-user-session"]) => admin::revoke_user_session(auth, req).await,
        ("POST", ["admin", "revoke-user-sessions"]) => admin::revoke_user_sessions(auth, req).await,
        ("POST", ["admin", "impersonate-user"]) => admin::impersonate_user(auth, req).await,
        ("POST", ["admin", "stop-impersonating"]) => admin::stop_impersonating(auth, req).await,
        ("POST", ["admin", "remove-user"]) => admin::remove_user(auth, req).await,
        // ---- CORS preflight ---------------------------------------------
        ("OPTIONS", _) => Ok(crate::http::Response::text(204, "")),
        _ => {
            if route_exists(&segments) {
                Err(AuthError::new(
                    405,
                    "METHOD_NOT_ALLOWED",
                    "method not allowed",
                ))
            } else {
                Err(not_found(&req.path))
            }
        }
    };
    result.unwrap_or_else(|e| crate::http::Response::error(&e))
}

fn not_found(path: &str) -> AuthError {
    AuthError::not_found("NOT_FOUND", format!("{path} is not a route on this server"))
}

/// Whether some method serves this path — to answer 405 rather than 404.
fn route_exists(segments: &[&str]) -> bool {
    let joined = segments.join("/");
    [
        "ok",
        "error",
        "get-session",
        "list-sessions",
        "revoke-session",
        "revoke-sessions",
        "revoke-other-sessions",
        "sign-up/email",
        "sign-in/email",
        "sign-out",
        "update-user",
        "change-password",
        "set-password",
        "change-email",
        "delete-user",
        "verify-email",
        "send-verification-email",
        "request-password-reset",
        "forget-password",
        "reset-password",
        "refresh-token",
        "is-username-available",
        "sign-in/username",
        "sign-in/anonymous",
        "sign-in/magic-link",
        "magic-link/verify",
        "email-otp/send-verification-otp",
        "email-otp/check-verification-otp",
        "email-otp/verify-email",
        "sign-in/email-otp",
        "email-otp/reset-password",
        "sign-in/social",
        "sign-in/oauth2",
        "oauth2/link",
        "list-accounts",
        "link-social-account",
        "unlink-account",
        "two-factor/enable",
        "two-factor/disable",
        "two-factor/get-totp-uri",
        "two-factor/send-otp",
        "two-factor/verify-otp",
        "two-factor/verify-totp",
        "two-factor/verify-backup-code",
        "two-factor/generate-backup-codes",
        "admin/create-user",
        "admin/list-users",
        "admin/get-user",
        "admin/update-user",
        "admin/set-role",
        "admin/set-user-password",
        "admin/ban-user",
        "admin/unban-user",
        "admin/list-user-sessions",
        "admin/revoke-user-session",
        "admin/revoke-user-sessions",
        "admin/impersonate-user",
        "admin/stop-impersonating",
        "admin/remove-user",
    ]
    .contains(&joined.as_str())
        || segments.len() >= 2
            && (segments[0] == "callback"
                || (segments[0] == "oauth2" && segments[1] == "callback")
                || segments[0] == "reset-password")
}

// --------------------------------------------------------------- helpers ---

/// The body as JSON. Form bodies are read as their fields, for Apple's
/// callback.
pub fn body(req: &Request) -> AuthResult<Value> {
    if req.is_form() {
        let mut o = mosql::Object::new();
        for (k, v) in req.form() {
            o.set(k, Value::str(v));
        }
        return Ok(Value::Object(o));
    }
    req.json()
}

/// A required string field.
pub fn field(body: &Value, name: &str) -> AuthResult<String> {
    let value = body
        .get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AuthError::invalid_request(format!("{name} is required")))?;
    Ok(value.to_string())
}

/// An optional string field; blank counts as absent.
pub fn opt_field(body: &Value, name: &str) -> Option<String> {
    body.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// An optional boolean field, accepting JSON booleans and the strings
/// `"true"`/`"false"`.
pub fn opt_bool(body: &Value, name: &str) -> Option<bool> {
    match body.get(name) {
        Some(Value::Bool(b)) => Some(*b),
        Some(Value::Str(s)) => match s.trim() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        Some(Value::Int(i)) => Some(*i != 0),
        _ => None,
    }
}

/// A redirect target from a body or query field, validated against the
/// trusted origins. `None` when the field is absent.
pub fn redirect_target(
    auth: &Auth,
    req: &Request,
    body: &Value,
    name: &str,
) -> AuthResult<Option<String>> {
    let target = opt_field(body, name).or_else(|| req.query_param(name).map(str::to_string));
    target
        .map(|t| crate::middleware::validate_redirect(auth, &t))
        .transpose()
}

/// `{"status": true}` — the commonest acknowledgement.
pub fn ok_status() -> AuthResult<crate::http::Response> {
    Ok(crate::http::Response::json(
        200,
        &Value::Object(object! {"status" => Value::Bool(true)}),
    ))
}

pub fn json(status: u16, value: &Value) -> crate::http::Response {
    crate::http::Response::json(status, value)
}

/// The sessions of a user that have not run out, newest first, without
/// their token digests.
pub async fn live_sessions(auth: &Auth, user_id: &str) -> AuthResult<Vec<Session>> {
    let sessions = auth
        .store
        .find_many_typed::<Session>(
            FindManyQuery::new(vec![WhereClause::eq("userId", Value::str(user_id))])
                .sorted("createdAt", false),
        )
        .await?;
    let now = now_iso();
    Ok(sessions
        .into_iter()
        .filter(|s| s.expires_at > now)
        .collect())
}

/// A session as clients see it.
pub fn session_json(session: &Session) -> Value {
    session.to_json()
}

/// The record for an unknown-model error never happens here, but the
/// trait needs `T: Record`; this keeps imports honest.
#[allow(dead_code)]
fn _assert_record<T: Record>() {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Response;

    fn auth() -> Arc<Auth> {
        Arc::new(Auth::new(
            Arc::new(crate::store::MemoryStore::new()),
            crate::auth::AuthConfig::new("https://app.test", "0123456789abcdef0123456789abcdef"),
        ))
    }

    use crate::async_test;

    async_test! {
    async fn unknown_paths_and_methods() {
        let auth = auth();
        let r = route(auth.clone(), Request::new("GET", "/api/auth/nope")).await;
        assert_eq!(r.status, 404);
        assert!(String::from_utf8(r.body).unwrap().contains("NOT_FOUND"));
        let r = route(auth.clone(), Request::new("GET", "/api/auth/sign-in/email")).await;
        assert_eq!(r.status, 405);
        let r = route(auth.clone(), Request::new("GET", "/elsewhere")).await;
        assert_eq!(r.status, 404);
        let r = route(auth, Request::new("OPTIONS", "/api/auth/get-session")).await;
        assert_eq!(r.status, 204);
        let _ = (Response::text(200, ""), live_sessions, session_json);
    }
    }

    #[test]
    fn fields() {
        let v = mosql::parse_json(r#"{"a":"x","b":"","c":" true ","d":1,"e":false}"#).unwrap();
        assert_eq!(field(&v, "a").unwrap(), "x");
        assert!(field(&v, "b").is_err());
        assert!(field(&v, "zz").is_err());
        assert_eq!(opt_field(&v, "c").as_deref(), Some("true"));
        assert_eq!(opt_bool(&v, "c"), Some(true));
        assert_eq!(opt_bool(&v, "d"), Some(true));
        assert_eq!(opt_bool(&v, "e"), Some(false));
        assert_eq!(opt_bool(&v, "q"), None);
    }
}
