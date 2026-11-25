//! Username plugin: sign in with a username instead of an email address.
//!
//! `update-user` with a `username` field also lives here, as the rule for
//! what a username may be is one thing wherever it is checked.

use std::sync::Arc;

use mosql::Value;

use crate::auth::Auth;
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::models::*;
use crate::object;
use crate::routes::{body, core, field, json, redirect_target};

fn invalid() -> AuthError {
    AuthError::unauthorized(
        "INVALID_USERNAME_OR_PASSWORD",
        "Invalid username or password",
    )
}

/// `POST /is-username-available` → `{available}`.
pub async fn is_available(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let username = check_username(&field(&body, "username")?)?;
    let taken = auth
        .user_by_username(&username.to_ascii_lowercase())
        .await?
        .is_some();
    Ok(json(
        200,
        &Value::Object(object! {"available" => b(!taken)}),
    ))
}

/// `POST /sign-in/username` → tokens, or the second-factor redirect.
pub async fn sign_in(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let username = check_username(&field(&body, "username")?)?;
    let password = field(&body, "password")?;
    let _ = redirect_target(auth, req, &body, "callbackURL")?;
    let user = auth
        .user_by_username(&username.to_ascii_lowercase())
        .await?;
    let Some(user) = user else {
        core::dummy_password_check(auth, &password).await;
        return Err(invalid());
    };
    if !auth.verify_password(&user.id, &password).await? {
        core::dummy_password_check(auth, &password).await;
        return Err(invalid());
    }
    core::sign_in_user(auth, req, &user).await
}
