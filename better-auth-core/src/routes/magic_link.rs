//! Magic link plugin: a mailed URL that signs the reader in.
//!
//! The token in the link is 32 random bytes; the row stores its SHA-256
//! and expires after five minutes. Reading it consumes it, so a link
//! works once.

use std::sync::Arc;

use mosql::Value;

use crate::auth::Auth;
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::{optional, redirect_with_error, redirect_with_tokens, validate_redirect};
use crate::models::*;
use crate::object;
use crate::routes::{body, core, field, json, redirect_target};

fn invalid() -> AuthError {
    AuthError::invalid_token("invalid or expired link")
}

/// `POST /sign-in/magic-link` → `{status: true}`, and a mail.
pub async fn send(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let callback = redirect_target(auth, req, &body, "callbackURL")?;
    let new_user_callback = redirect_target(auth, req, &body, "newUserCallbackURL")?;
    let error_callback = redirect_target(auth, req, &body, "errorCallbackURL")?;
    if !auth.can_mail() {
        return Err(AuthError::bad_request(
            "MAIL_NOT_CONFIGURED",
            "Mail is not configured on this server",
        ));
    }
    let token = core::new_token();
    let value = format!(
        r#"{{"email":{},"callbackURL":{},"newUserCallbackURL":{},"errorCallbackURL":{},"linkUserId":{}}}"#,
        core::json_string(&email),
        callback
            .as_deref()
            .map(core::json_string)
            .unwrap_or_else(|| "null".into()),
        new_user_callback
            .as_deref()
            .map(core::json_string)
            .unwrap_or_else(|| "null".into()),
        error_callback
            .as_deref()
            .map(core::json_string)
            .unwrap_or_else(|| "null".into()),
        optional(auth, req)
            .filter(|w| w.is_anonymous())
            .map(|w| core::json_string(w.user_id()))
            .unwrap_or_else(|| "null".into())
    );
    auth.put_verification(
        &core::token_key(core::V_MAGIC, &token),
        &value,
        auth.config.magic_link_ttl_secs,
    )
    .await?;
    let link = format!(
        "{}{}/magic-link/verify?token={token}",
        auth.config.base_url,
        crate::auth::PATH_PREFIX
    );
    auth.mail_link(&email, "Your sign-in link", &link)?;
    Ok(json(
        200,
        &Value::Object(object! {"status" => Value::Bool(true)}),
    ))
}

/// `GET /magic-link/verify?token=` → redirect with tokens, or JSON.
pub async fn verify(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let token = req
        .query_param("token")
        .ok_or_else(|| AuthError::invalid_request("token is required"))?
        .to_string();
    let value = auth
        .take_verification(&core::token_key(core::V_MAGIC, &token))
        .await?;
    let Some(value) = value else {
        return Err(invalid());
    };
    let data = core::parse_object(&value)?;
    let get = |k: &str| data.get(k).and_then(Value::as_str).map(str::to_string);
    let email = get("email").ok_or_else(|| AuthError::internal("bad magic-link record"))?;
    // Without a callback the answer is the tokens themselves.
    let fallback_callback = get("callbackURL");
    let callback = match req.query_param("callbackURL") {
        Some(c) => Some(validate_redirect(auth, c)?),
        None => fallback_callback,
    };

    let outcome = finish(auth, &email, get("linkUserId").as_deref()).await;
    match outcome {
        Ok((user, is_new)) => {
            let target = if is_new {
                get("newUserCallbackURL")
            } else {
                None
            }
            .or(callback);
            let (session, refresh) = auth.create_session(&user, req, None).await?;
            let access = auth.sign_access(&user, &session.id, None);
            match target {
                Some(target) => Ok(redirect_with_tokens(&target, &access, &refresh)),
                None => Ok(auth.token_response(&access, &refresh, &user, 200)),
            }
        }
        Err(e) => {
            let target = get("errorCallbackURL").or(callback);
            match target {
                Some(target) => Ok(redirect_with_error(&target, &e)),
                None => Err(e),
            }
        }
    }
}

/// Find or create the user a link belongs to, marking the address
/// verified. An anonymous `link_user_id` is upgraded in place.
async fn finish(auth: &Auth, email: &str, link_user_id: Option<&str>) -> AuthResult<(User, bool)> {
    if let Some(id) = link_user_id {
        let user = auth.require_user(id).await?;
        let name = if user.name.is_empty() || user.is_anonymous {
            email.split('@').next().unwrap_or("user").to_string()
        } else {
            user.name.clone()
        };
        let updated = auth
            .update_user(
                &user.id,
                object! {
                    "email" => s(email),
                    "emailVerified" => b(true),
                    "isAnonymous" => b(false),
                    "name" => s(name),
                },
            )
            .await?;
        return Ok((updated, false));
    }
    let Some(user) = auth.user_by_email(email).await? else {
        let user = auth
            .save_user(&User {
                id: new_id(),
                name: email.split('@').next().unwrap_or("user").to_string(),
                email: email.to_string(),
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
            .await?;
        return Ok((user, true));
    };
    let user = if user.email_verified {
        user
    } else {
        auth.update_user(&user.id, object! {"emailVerified" => b(true)})
            .await?
    };
    Ok((user, false))
}
