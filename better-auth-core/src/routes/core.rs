//! The core routes: email sign-up and sign-in, sessions, the user row,
//! password and email changes, and the verification mails that tie them
//! together.
//!
//! Every response that establishes a session is `{token, refreshToken,
//! user}` with the access token also in the `set-auth-token` header.
//! Browser-completed flows (clicking a mailed link) redirect to the
//! validated callback with `?token=&refreshToken=` so a client without
//! cookies still lands signed in.

use std::sync::Arc;

use mosql::Value;

use crate::auth::{Auth, HttpRequest};
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::{
    authenticate, optional, redirect_with_error, redirect_with_tokens, validate_redirect,
};
use crate::models::*;
use crate::object;
use crate::routes::{
    body, field, json, live_sessions, ok_status, opt_bool, opt_field, redirect_target, session_json,
};
use crate::store::{AuthStoreExt, WhereClause};

// Purposes a verification token can have. The stored identifier is
// `<purpose>:<sha256(token)>`, so a leaked table yields no usable tokens.
pub const V_EMAIL: &str = "email-verification";
pub const V_CHANGE_EMAIL: &str = "change-email";
pub const V_RESET: &str = "password-reset";
pub const V_MAGIC: &str = "magic-link";
pub const V_OTP: &str = "otp";
pub const V_STATE: &str = "oauth-state";
pub const V_DELETE: &str = "delete-user";

/// The identifier a token is stored under.
pub fn token_key(purpose: &str, token: &str) -> String {
    format!("{purpose}:{}", crate::crypto::sha256_hex(token.as_bytes()))
}

/// A fresh token to put in a link.
pub fn new_token() -> String {
    crate::crypto::random_token(32)
}

/// A JSON string literal, for building small documents by hand.
pub fn json_string(text: &str) -> String {
    mosql::to_json(&Value::str(text))
}

pub fn parse_object(text: &str) -> AuthResult<mosql::Object> {
    match mosql::parse_json(text) {
        Ok(Value::Object(o)) => Ok(o),
        _ => Err(AuthError::internal(
            "stored verification value is not an object",
        )),
    }
}

/// A link back to this server, with its query already encoded.
pub fn server_link(auth: &Auth, path_and_query: &str) -> String {
    format!("{}{path_and_query}", auth.config.base_url)
}

pub fn ok() -> Response {
    json(200, &Value::Object(object! {"ok" => Value::Bool(true)}))
}

/// The usual shape for a user who exists but may not sign in yet.
fn unverified(user: &User) -> AuthResult<Response> {
    Ok(json(
        200,
        &Value::Object(object! {
            "token" => Value::Null,
            "refreshToken" => Value::Null,
            "user" => user.to_json(),
        }),
    ))
}

/// The checks every sign-in shares: ban, second factor, verification.
pub async fn sign_in_user(auth: &Auth, req: &Request, user: &User) -> AuthResult<Response> {
    if let Some(reason) = user.ban_in_force(&now_iso()) {
        return Err(AuthError::forbidden("USER_BANNED", reason));
    }
    if user.banned {
        // The ban ran out: lift it rather than treating it as in force
        // on every later request.
        let mut changes = object! {
            "banned" => b(false),
            "banReason" => Value::Null,
            "banExpires" => Value::Null,
        };
        changes.set("updatedAt", s(now_iso()));
        auth.store
            .update(MODEL_USER, &[WhereClause::id(&user.id)], changes)
            .await?;
    }
    if user.two_factor_enabled {
        return Ok(auth.pending_response(&user.id));
    }
    if auth.config.require_email_verification && !user.email_verified && !user.is_anonymous {
        return Err(AuthError::forbidden(
            "EMAIL_NOT_VERIFIED",
            "Email is not verified",
        ));
    }
    auth.issue(user, req, None).await
}

/// Burn the same time a real password check would, so a missing user is
/// not visibly faster than a wrong password.
pub(crate) async fn dummy_password_check(auth: &Auth, password: &str) -> bool {
    let hash = crate::crypto::hash_password("not-the-password", auth.config.pbkdf2_iterations);
    crate::crypto::verify_password(password, &hash)
}

/// Mail a verification link, if the address is unverified and mail can go out.
pub async fn send_verification(auth: &Auth, user: &User, callback: Option<&str>) -> AuthResult<()> {
    if user.email_verified || user.is_anonymous || !auth.can_mail() {
        return Ok(());
    }
    let token = new_token();
    auth.put_verification(
        &token_key(V_EMAIL, &token),
        &user.id,
        auth.config.email_verification_ttl_secs,
    )
    .await?;
    let mut link = format!(
        "{}{}/verify-email?token={}",
        auth.config.base_url,
        crate::auth::PATH_PREFIX,
        token
    );
    if let Some(callback) = callback {
        link.push_str(&format!(
            "&callbackURL={}",
            crate::http::percent_encode(callback)
        ));
    }
    auth.mail_link(&user.email, "Verify your email", &link)
}

// ------------------------------------------------------------------ routes --

pub async fn get_session(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    // Signed out is not an error here: the client reads `null`.
    let Some(who) = optional(auth, req) else {
        return Ok(json(200, &Value::Null));
    };
    let Some(user) = auth.user_by_id(who.user_id()).await? else {
        return Ok(json(200, &Value::Null));
    };
    let session = auth
        .store
        .find_one_typed::<Session>(&[WhereClause::id(&who.session_id)])
        .await?
        .filter(|s| s.expires_at > now_iso());
    let Some(session) = session else {
        return Ok(json(200, &Value::Null));
    };
    Ok(json(
        200,
        &Value::Object(object! {
            "session" => session_json(&session),
            "user" => user.to_json(),
        }),
    ))
}

pub async fn list_sessions(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let sessions = live_sessions(auth, who.user_id()).await?;
    let items: Vec<Value> = sessions
        .iter()
        .map(|s| {
            let mut o = match session_json(s) {
                Value::Object(o) => o,
                _ => unreachable!(),
            };
            o.remove("token");
            o.set("isCurrent", b(s.id == who.session_id));
            Value::Object(o)
        })
        .collect();
    Ok(json(200, &Value::Array(items)))
}

pub async fn revoke_session(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let token = field(&body, "token")?;
    let digest = crate::crypto::sha256_hex(token.as_bytes());
    let found = auth
        .store
        .find_one_typed::<Session>(&[
            WhereClause::eq("token", s(digest)),
            WhereClause::eq("userId", s(who.user_id())),
        ])
        .await?;
    match found {
        Some(session) => {
            auth.revoke_session(&session.id).await?;
            ok_status()
        }
        None => Ok(json(200, &Value::Object(object! {"status" => b(false)}))),
    }
}

pub async fn revoke_sessions(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    auth.revoke_user_sessions(who.user_id(), None).await?;
    ok_status()
}

pub async fn revoke_other_sessions(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    auth.revoke_user_sessions(who.user_id(), Some(&who.session_id))
        .await?;
    ok_status()
}

pub async fn sign_up_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let password = field(&body, "password")?;
    check_password(&password)?;
    let name = opt_field(&body, "name")
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());
    let image = opt_field(&body, "image");
    let callback = redirect_target(auth, req, &body, "callbackURL")?;

    // An anonymous caller becomes this user instead of leaving an orphan
    // behind: same id, so everything they did stays theirs.
    if let Some(who) = optional(auth, req).filter(|w| w.is_anonymous()) {
        let mut changes = object! {
            "email" => s(email.clone()),
            "name" => s(name),
            "isAnonymous" => b(false),
            "emailVerified" => b(false),
        };
        if let Some(image) = image {
            changes.set("image", s(image));
        }
        let updated = auth.update_user(who.user_id(), changes).await?;
        auth.set_password(&updated.id, &password).await?;
        send_verification(auth, &updated, callback.as_deref()).await?;
        if auth.config.require_email_verification {
            return unverified(&updated);
        }
        return auth.issue(&updated, req, None).await;
    }

    if auth.user_by_email(&email).await?.is_some() {
        return Err(AuthError::bad_request(
            "USER_ALREADY_EXISTS",
            "A user with this email already exists. Try signing in.",
        ));
    }
    let user = auth
        .save_user(&User {
            id: new_id(),
            name,
            email,
            email_verified: false,
            image,
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
    auth.set_password(&user.id, &password).await?;
    send_verification(auth, &user, callback.as_deref()).await?;
    if auth.config.require_email_verification {
        return unverified(&user);
    }
    auth.issue(&user, req, None).await
}

pub async fn sign_in_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let password = field(&body, "password")?;
    let user = auth.user_by_email(&email).await?;
    let Some(user) = user else {
        dummy_password_check(auth, &password).await;
        return Err(invalid_credentials());
    };
    if !auth.verify_password(&user.id, &password).await? {
        dummy_password_check(auth, &password).await;
        return Err(invalid_credentials());
    }
    sign_in_user(auth, req, &user).await
}

fn invalid_credentials() -> AuthError {
    AuthError::unauthorized("INVALID_EMAIL_OR_PASSWORD", "Invalid email or password")
}

pub async fn sign_out(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    if let Some(who) = optional(auth, req) {
        auth.revoke_session(&who.session_id).await?;
    }
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}

pub async fn update_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let mut changes = mosql::Object::new();
    if let Some(name) = opt_field(&body, "name") {
        changes.set("name", s(name));
    }
    if let Some(image) = opt_field(&body, "image") {
        changes.set(
            "image",
            if image.is_empty() {
                Value::Null
            } else {
                s(image)
            },
        );
    }
    if let Some(username) = opt_field(&body, "username") {
        let username = check_username(&username)?;
        if let Some(taken) = auth.user_by_username(&username).await? {
            if taken.id != who.user_id() {
                return Err(AuthError::bad_request(
                    "USERNAME_TAKEN",
                    "That username is taken",
                ));
            }
        }
        changes.set("username", s(username.to_ascii_lowercase()));
        changes.set("displayUsername", s(username));
    }
    if changes.iter().next().is_none() {
        return Err(AuthError::invalid_request(
            "nothing to update: name, image or username",
        ));
    }
    let updated = auth.update_user(who.user_id(), changes).await?;
    Ok(json(
        200,
        &Value::Object(object! {
            "status" => b(true),
            "user" => updated.to_json(),
        }),
    ))
}

pub async fn change_password(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let current = field(&body, "currentPassword")?;
    let new_password = field(&body, "newPassword")?;
    if !auth.verify_password(who.user_id(), &current).await? {
        return Err(AuthError::unauthorized(
            "INVALID_PASSWORD",
            "The current password is wrong",
        ));
    }
    check_password(&new_password)?;
    let user = who.user(auth).await?;
    auth.set_password(&user.id, &new_password).await?;
    if opt_bool(&body, "revokeOtherSessions") == Some(true) {
        auth.revoke_user_sessions(&user.id, Some(&who.session_id))
            .await?;
    }
    auth.issue(&user, req, None).await
}

pub async fn set_password(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let new_password = field(&body, "newPassword")?;
    if auth
        .credential_account(who.user_id())
        .await?
        .is_some_and(|a| a.password.is_some())
    {
        return Err(AuthError::bad_request(
            "PASSWORD_ALREADY_SET",
            "This account already has a password; use change-password",
        ));
    }
    auth.set_password(who.user_id(), &new_password).await?;
    ok_status()
}

pub async fn change_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let new_email = check_email(&field(&body, "newEmail")?)?;
    if !auth.can_mail() {
        return Err(AuthError::bad_request(
            "MAIL_NOT_CONFIGURED",
            "Mail is not configured on this server",
        ));
    }
    if auth.user_by_email(&new_email).await?.is_some() {
        return Err(AuthError::bad_request(
            "USER_ALREADY_EXISTS",
            "A user with this email already exists",
        ));
    }
    let token = new_token();
    let value = format!(
        r#"{{"userId":{},"newEmail":{}}}"#,
        json_string(who.user_id()),
        json_string(&new_email)
    );
    auth.put_verification(
        &token_key(V_CHANGE_EMAIL, &token),
        &value,
        auth.config.email_verification_ttl_secs,
    )
    .await?;
    let link = format!(
        "{}{}/verify-email?token={}",
        auth.config.base_url,
        crate::auth::PATH_PREFIX,
        token
    );
    auth.mail_link(&new_email, "Confirm your new email", &link)?;
    ok_status()
}

pub async fn delete_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = authenticate(auth, req)?;
    let body = body(req)?;
    let has_password = auth
        .credential_account(who.user_id())
        .await?
        .is_some_and(|a| a.password.is_some());
    if has_password {
        let password = field(&body, "password")?;
        if !auth.verify_password(who.user_id(), &password).await? {
            return Err(AuthError::unauthorized(
                "INVALID_PASSWORD",
                "Password is required to delete the account",
            ));
        }
    } else if let Some(token) = opt_field(&body, "token") {
        let confirmed = auth.take_verification(&token_key(V_DELETE, &token)).await?;
        if confirmed.as_deref() != Some(who.user_id()) {
            return Err(AuthError::invalid_token("invalid confirmation token"));
        }
    }
    auth.delete_user(who.user_id()).await?;
    Ok(json(
        200,
        &Value::Object(object! {
            "success" => b(true),
            "message" => s("User deleted"),
        }),
    ))
}

pub async fn verify_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let token = field(&body, "token").or_else(|_| {
        req.query_param("token")
            .map(str::to_string)
            .ok_or_else(|| AuthError::invalid_request("token is required"))
    })?;
    let callback = redirect_target(auth, req, &body, "callbackURL")?;

    let outcome: AuthResult<User> = async {
        if let Some(user_id) = auth.take_verification(&token_key(V_EMAIL, &token)).await? {
            let user = auth.require_user(&user_id).await?;
            return auth
                .update_user(&user.id, object! {"emailVerified" => b(true)})
                .await;
        }
        if let Some(value) = auth
            .take_verification(&token_key(V_CHANGE_EMAIL, &token))
            .await?
        {
            let data = parse_object(&value)?;
            let user_id = data
                .get("userId")
                .and_then(Value::as_str)
                .ok_or_else(|| AuthError::internal("bad change-email record"))?
                .to_string();
            let new_email = data
                .get("newEmail")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if auth.user_by_email(&new_email).await?.is_some() {
                return Err(AuthError::bad_request(
                    "USER_ALREADY_EXISTS",
                    "A user with this email already exists",
                ));
            }
            return auth
                .update_user(
                    &user_id,
                    object! {
                        "email" => s(new_email),
                        "emailVerified" => b(true),
                    },
                )
                .await;
        }
        Err(AuthError::invalid_token("invalid or expired token"))
    }
    .await;

    match outcome {
        Ok(user) => {
            let (session, refresh) = auth.create_session(&user, req, None).await?;
            let access = auth.sign_access(&user, &session.id, None);
            match callback {
                Some(callback) => Ok(redirect_with_tokens(&callback, &access, &refresh)),
                None => Ok(auth.token_response(&access, &refresh, &user, 200)),
            }
        }
        Err(e) => match callback {
            Some(callback) => Ok(redirect_with_error(&callback, &e)),
            None => Err(e),
        },
    }
}

pub async fn send_verification_email(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    // Address in the body, or the signed-in caller.
    let user = match opt_field(&body, "email") {
        Some(email) => auth.user_by_email(&email.to_ascii_lowercase()).await?,
        None => match optional(auth, req) {
            Some(who) => auth.user_by_id(who.user_id()).await?,
            None => None,
        },
    };
    if let Some(user) = user {
        send_verification(auth, &user, None).await?;
    }
    // Whether or not the address exists: the answer is the same.
    ok_status()
}

pub async fn request_password_reset(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    let redirect_to = redirect_target(auth, req, &body, "redirectTo")?;
    if auth.can_mail() {
        if let Some(user) = auth.user_by_email(&email).await? {
            if auth
                .credential_account(&user.id)
                .await?
                .is_some_and(|a| a.password.is_some())
            {
                let token = new_token();
                let value = format!(
                    r#"{{"userId":{},"redirectTo":{}}}"#,
                    json_string(&user.id),
                    redirect_to
                        .as_deref()
                        .map(json_string)
                        .unwrap_or_else(|| "null".into())
                );
                auth.put_verification(
                    &token_key(V_RESET, &token),
                    &value,
                    auth.config.password_reset_ttl_secs,
                )
                .await?;
                let link = format!(
                    "{}{}/reset-password/{token}",
                    auth.config.base_url,
                    crate::auth::PATH_PREFIX
                );
                auth.mail_link(&user.email, "Reset your password", &link)?;
            }
        }
    }
    Ok(json(
        200,
        &Value::Object(object! {
            "status" => b(true),
            "message" => s("If this email exists in our system, check your email for the reset link"),
        }),
    ))
}

pub async fn reset_password(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let new_password = field(&body, "newPassword")?;
    let user_id = match opt_field(&body, "token") {
        Some(token) => {
            let value = auth
                .take_verification(&token_key(V_RESET, &token))
                .await?
                .ok_or_else(|| AuthError::invalid_token("invalid or expired token"))?;
            parse_object(&value)?
                .get("userId")
                .and_then(Value::as_str)
                .ok_or_else(|| AuthError::internal("bad reset record"))?
                .to_string()
        }
        None => authenticate(auth, req)?.user_id().to_string(),
    };
    auth.set_password(&user_id, &new_password).await?;
    auth.revoke_user_sessions(&user_id, None).await?;
    ok_status()
}

/// The mailed link: validate without consuming (the POST consumes), and
/// hand the browser to the app's reset page with the token.
pub async fn reset_password_link(
    auth: &Arc<Auth>,
    req: &Request,
    token: &str,
) -> AuthResult<Response> {
    let fallback = format!("{}/reset-password", auth.config.base_url);
    let callback = match req
        .query_param("callbackURL")
        .map(|t| validate_redirect(auth, t))
        .transpose()?
    {
        Some(c) => c,
        None => match auth.peek_verification(&token_key(V_RESET, token)).await? {
            Some(record) => {
                let data = parse_object(&record.value).unwrap_or_default();
                data.get("redirectTo")
                    .and_then(Value::as_str)
                    .map(|t| validate_redirect(auth, t))
                    .transpose()?
                    .unwrap_or(fallback)
            }
            None => fallback,
        },
    };
    let valid = auth
        .peek_verification(&token_key(V_RESET, token))
        .await?
        .is_some_and(|v| v.expires_at > now_iso());
    if !valid {
        return Ok(redirect_with_error(
            &callback,
            &AuthError::invalid_token("invalid or expired token"),
        ));
    }
    let separator = if callback.contains('?') { '&' } else { '?' };
    Ok(Response::redirect(&format!(
        "{callback}{separator}token={}",
        crate::http::percent_encode(token)
    )))
}

/// `POST /refresh-token` with the refresh token as the bearer.
pub async fn refresh_token(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let refresh = req.bearer_token().ok_or_else(|| {
        AuthError::unauthorized("UNAUTHORIZED", "Send the refresh token as a bearer token")
    })?;
    let (user, session, new_refresh) = auth.refresh(refresh, req).await?;
    let access = auth.sign_access(&user, &session.id, session.impersonated_by.as_deref());
    Ok(auth.token_response(&access, &new_refresh, &user, 200))
}

/// For hosts that want the OAuth token exchange through this module's
/// conventions; kept here so `HttpRequest` has one import site in routes.
pub fn http_request(
    method: &str,
    url: &str,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
) -> HttpRequest {
    HttpRequest {
        method: method.to_string(),
        url: url.to_string(),
        headers,
        body,
    }
}
