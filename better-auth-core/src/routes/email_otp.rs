//! Email OTP plugin: a six-digit code mailed to an address.
//!
//! One row per `(type, address)`, holding the code's SHA-256 and an attempt
//! counter. Sending again replaces the row. A wrong code increments the
//! counter; at five the row is deleted and the caller must request a new
//! code — the lockout — so a code cannot be brute-forced within its five
//! minutes.

use std::sync::Arc;

use mosql::Value;

use crate::auth::Auth;
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::models::*;
use crate::object;
use crate::routes::{body, core, field, json, ok_status};

pub const OTP_TYPES: [&str; 4] = [
    "sign_in",
    "forget_password",
    "email_verification",
    "change_email",
];

fn key(kind: &str, email: &str) -> String {
    format!("{}:{kind}:{}", core::V_OTP, email)
}

fn check_type(kind: &str) -> AuthResult<()> {
    if OTP_TYPES.contains(&kind) {
        Ok(())
    } else {
        Err(AuthError::invalid_request(format!(
            "type must be one of {}",
            OTP_TYPES.join(", ")
        )))
    }
}

/// Store a fresh code for `(kind, email)`.
pub async fn store_code(auth: &Auth, kind: &str, email: &str, code: &str) -> AuthResult<()> {
    let value = format!(
        r#"{{"hash":{},"attempts":0}}"#,
        core::json_string(&crate::crypto::sha256_hex(code.as_bytes()))
    );
    auth.put_verification(&key(kind, email), &value, auth.config.otp_ttl_secs)
        .await
}

/// `pub(crate)`-wide helper: generate, store and mail a code. Returns
/// the code so a server without mail can hand it to the caller instead.
pub(crate) async fn issue_code(
    auth: &Auth,
    kind: &str,
    email: &str,
    subject: &str,
) -> AuthResult<String> {
    issue_code_for(auth, kind, email, email, subject).await
}

/// The same, but the row is keyed by one address and the mail goes to
/// another — which is what the two-factor plugin needs, where the key
/// is the user's id but the code belongs in the user's inbox.
pub(crate) async fn issue_code_for(
    auth: &Auth,
    kind: &str,
    key: &str,
    to: &str,
    subject: &str,
) -> AuthResult<String> {
    let code = crate::crypto::random_digits(6);
    store_code(auth, kind, key, &code).await?;
    if auth.can_mail() {
        auth.send_mail(
            to,
            subject,
            &format!("Your verification code is {code}. It expires in five minutes."),
            None,
        )?;
    }
    Ok(code)
}

fn wrong() -> AuthError {
    AuthError::unauthorized("INVALID_OTP", "Invalid OTP")
}

fn locked_out() -> AuthError {
    AuthError::unauthorized(
        "TOO_MANY_ATTEMPTS",
        "Too many attempts. Request a new code.",
    )
}

/// Compare a code. On success the row is consumed; on failure the
/// counter goes up and the row is dropped when the limit is reached.
pub async fn check_code(
    auth: &Auth,
    kind: &str,
    email: &str,
    code: &str,
    consume: bool,
) -> AuthResult<()> {
    let id = key(kind, email);
    let Some(record) = auth.peek_verification(&id).await? else {
        return Err(wrong());
    };
    if record.expires_at <= now_iso() {
        auth.store
            .delete(
                MODEL_VERIFICATION,
                &[crate::store::WhereClause::id(&record.id)],
            )
            .await?;
        return Err(wrong());
    }
    let data = core::parse_object(&record.value)?;
    let stored = data.get("hash").and_then(Value::as_str).unwrap_or_default();
    let attempts = data.get("attempts").and_then(Value::as_f64).unwrap_or(0.0) as u32;
    if !crate::crypto::ct_eq(
        stored.as_bytes(),
        crate::crypto::sha256_hex(code.as_bytes()).as_bytes(),
    ) {
        let attempts = attempts + 1;
        if attempts >= auth.config.otp_max_attempts {
            auth.store
                .delete(
                    MODEL_VERIFICATION,
                    &[crate::store::WhereClause::id(&record.id)],
                )
                .await?;
            return Err(locked_out());
        }
        let value = format!(
            r#"{{"hash":{},"attempts":{attempts}}}"#,
            core::json_string(stored)
        );
        let update = object! {"value" => s(value), "updatedAt" => s(now_iso())};
        auth.store
            .update(
                MODEL_VERIFICATION,
                &[crate::store::WhereClause::id(&record.id)],
                update,
            )
            .await?;
        return Err(wrong());
    }
    if consume {
        auth.store
            .delete(
                MODEL_VERIFICATION,
                &[crate::store::WhereClause::id(&record.id)],
            )
            .await?;
    }
    Ok(())
}

/// `POST /email-otp/send-verification-otp` → `{success: true}`.
pub async fn send(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let kind = field(&body, "type").unwrap_or_else(|_| "email_verification".into());
    check_type(&kind)?;
    if !auth.can_mail() {
        // Development without a mailer: tell the caller the code, so the
        // flow can still be exercised end to end.
        let code = issue_code(auth, &kind, &email, "Your verification code").await?;
        return Ok(json(
            200,
            &Value::Object(object! {"success" => b(true), "otp" => s(code)}),
        ));
    }
    issue_code(auth, &kind, &email, "Your verification code").await?;
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}

/// `POST /email-otp/check-verification-otp` → `{success: true}` without
/// consuming the code.
pub async fn check(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let otp = field(&body, "otp")?;
    let kind = field(&body, "type").unwrap_or_else(|_| "email_verification".into());
    check_type(&kind)?;
    check_code(auth, &kind, &email, &otp, false).await?;
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}

/// `POST /email-otp/verify-email` → verified, signed in.
pub async fn verify_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let otp = field(&body, "otp")?;
    check_code(auth, "email_verification", &email, &otp, true).await?;
    let user = auth
        .user_by_email(&email)
        .await?
        .ok_or_else(|| AuthError::not_found("USER_NOT_FOUND", "User not found"))?;
    let user = if user.email_verified {
        user
    } else {
        auth.update_user(&user.id, object! {"emailVerified" => b(true)})
            .await?
    };
    let response = auth.issue(&user, req, None).await?;
    // The same shape as the other token answers, plus the flag.
    let mut o = match mosql::parse_json(&String::from_utf8_lossy(&response.body)) {
        Ok(Value::Object(o)) => o,
        _ => mosql::Object::new(),
    };
    o.set("status", b(true));
    Ok(Response::json(200, &Value::Object(o)))
}

/// `POST /sign-in/email-otp` → tokens for the address, creating the user
/// if this is their first time.
pub async fn sign_in(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let otp = field(&body, "otp")?;
    // An anonymous caller is upgraded rather than replaced.
    let anon = crate::middleware::optional(auth, req)
        .filter(|w| w.is_anonymous())
        .map(|w| w.user_id().to_string());
    check_code(auth, "sign_in", &email, &otp, true).await?;
    let user = if let Some(id) = anon {
        let user = auth.require_user(&id).await?;
        auth.update_user(
            &user.id,
            object! {
                "email" => s(email),
                "emailVerified" => b(true),
                "isAnonymous" => b(false),
            },
        )
        .await?
    } else {
        match auth.user_by_email(&email).await? {
            Some(user) => user,
            None => {
                auth.save_user(&User {
                    id: new_id(),
                    name: email.split('@').next().unwrap_or("user").to_string(),
                    email: email.clone(),
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
                    created_at: now_iso(),
                    updated_at: now_iso(),
                    extra: mosql::Object::new(),
                })
                .await?
            }
        }
    };
    core::sign_in_user(auth, req, &user).await
}

/// `POST /email-otp/reset-password` → `{success: true}`.
pub async fn reset_password(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let otp = field(&body, "otp")?;
    let password = field(&body, "password")?;
    check_code(auth, "forget_password", &email, &otp, true).await?;
    let user = auth
        .user_by_email(&email)
        .await?
        .ok_or_else(|| AuthError::not_found("USER_NOT_FOUND", "User not found"))?;
    auth.set_password(&user.id, &password).await?;
    auth.revoke_user_sessions(&user.id, None).await?;
    ok_status()
}
