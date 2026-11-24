//! Admin plugin: every route here is for `role == "admin"` only.
//!
//! Bans take the user's sessions away at once; an expired ban is lifted
//! lazily on the user's next sign-in. Impersonation issues a real session
//! for the target with `impersonatedBy` set to the admin, and the token
//! carries an `imp` claim, so every request made with it is visibly not
//! the admin's own.

use std::sync::Arc;

use mosql::Value;

use crate::auth::{Auth, TOKEN_HEADER};
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::{authenticate, require_admin, AuthUser};
use crate::models::*;
use crate::object;
use crate::routes::{body, field, json, live_sessions, opt_field};
use crate::store::{AuthStoreExt, FilterOp, FindManyQuery, WhereClause};

fn user_not_found() -> AuthError {
    AuthError::not_found("USER_NOT_FOUND", "User not found")
}

fn target(req: &Request, body: &Value) -> AuthResult<String> {
    field(body, "userId").or_else(|_| {
        req.query_param("userId")
            .map(str::to_string)
            .ok_or_else(|| AuthError::invalid_request("userId is required"))
    })
}

// ------------------------------------------------------------------ routes --

/// `POST /admin/create-user` → `{user}`.
pub async fn create_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let email = check_email(&field(&body, "email")?)?;
    if auth.user_by_email(&email).await?.is_some() {
        return Err(AuthError::bad_request(
            "USER_ALREADY_EXISTS",
            "A user with this email already exists",
        ));
    }
    let password = opt_field(&body, "password");
    if let Some(p) = &password {
        check_password(p)?;
    }
    let role = opt_field(&body, "role").unwrap_or_else(|| "user".into());
    if !matches!(role.as_str(), "user" | "admin") {
        return Err(AuthError::invalid_request("role must be user or admin"));
    }
    let verified = body
        .get("emailVerified")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let name = opt_field(&body, "name")
        .unwrap_or_else(|| email.split('@').next().unwrap_or("user").to_string());
    let user = auth
        .save_user(&User {
            id: new_id(),
            name,
            email,
            email_verified: verified,
            image: opt_field(&body, "image"),
            username: None,
            display_username: None,
            is_anonymous: false,
            two_factor_enabled: false,
            role,
            banned: false,
            ban_reason: None,
            ban_expires: None,
            created_at: now_iso(),
            updated_at: now_iso(),
            extra: mosql::Object::new(),
        })
        .await?;
    if let Some(password) = password {
        auth.set_password(&user.id, &password).await?;
    }
    Ok(json(
        200,
        &Value::Object(object! {"user" => user.to_json()}),
    ))
}

const USER_FIELDS: [&str; 12] = [
    "id",
    "email",
    "name",
    "image",
    "username",
    "role",
    "emailVerified",
    "banned",
    "isAnonymous",
    "twoFactorEnabled",
    "createdAt",
    "updatedAt",
];

fn parse_filter_value(field: &str, text: &str) -> Value {
    match field {
        "emailVerified" | "banned" | "isAnonymous" | "twoFactorEnabled" => match text {
            "true" | "1" => b(true),
            "false" | "0" | "" => b(false),
            _ => s(text),
        },
        _ => s(text),
    }
}

fn parse_op(text: &str) -> FilterOp {
    match text {
        "eq" => FilterOp::Eq,
        "ne" => FilterOp::Ne,
        "lt" => FilterOp::Lt,
        "lte" => FilterOp::Lte,
        "gt" => FilterOp::Gt,
        "gte" => FilterOp::Gte,
        "contains" => FilterOp::Contains,
        "starts_with" => FilterOp::StartsWith,
        "ends_with" => FilterOp::EndsWith,
        _ => FilterOp::Eq,
    }
}

/// `GET /admin/list-users` → `{users, total}` with search, filter, sort
/// and paging, all through `FindManyQuery`.
pub async fn list_users(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let mut clauses = Vec::new();
    if let Some(value) = req.query_param("searchValue").filter(|v| !v.is_empty()) {
        let field = req.query_param("searchField").unwrap_or("email");
        let op = req.query_param("searchOperator").unwrap_or("contains");
        if !USER_FIELDS.contains(&field) {
            return Err(AuthError::invalid_request(format!(
                "{field} is not searchable"
            )));
        }
        clauses.push(WhereClause::new(field, parse_op(op), s(value)));
    }
    if let Some(value) = req.query_param("filterValue") {
        let field = req.query_param("filterField").unwrap_or("email");
        let op = req.query_param("filterOperator").unwrap_or("eq");
        if !USER_FIELDS.contains(&field) {
            return Err(AuthError::invalid_request(format!(
                "{field} is not filterable"
            )));
        }
        clauses.push(WhereClause::new(
            field,
            parse_op(op),
            parse_filter_value(field, value),
        ));
    }
    let sort_by = req.query_param("sortBy").unwrap_or("createdAt");
    if !USER_FIELDS.contains(&sort_by) {
        return Err(AuthError::invalid_request(format!(
            "{sort_by} is not sortable"
        )));
    }
    let ascending = !matches!(req.query_param("sortDirection"), Some("desc"));
    let limit: u64 = req
        .query_param("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(500);
    let offset: u64 = req
        .query_param("offset")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let total = auth.store.count(MODEL_USER, &clauses).await?;
    let users = auth
        .store
        .find_many_typed::<User>(
            FindManyQuery::new(clauses)
                .sorted(sort_by, ascending)
                .page(limit, offset),
        )
        .await?;
    Ok(json(
        200,
        &Value::Object(object! {
            "users" => Value::Array(users.iter().map(|u| u.to_json()).collect()),
            "total" => Value::Int(total as i64),
        }),
    ))
}

/// `GET /admin/get-user?userId=|email=` → `{user}`.
pub async fn get_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let user = match req.query_param("email") {
        Some(email) => auth.user_by_email(&email.to_ascii_lowercase()).await?,
        None => {
            auth.user_by_id(req.query_param("userId").unwrap_or_default())
                .await?
        }
    };
    let user = user.ok_or_else(user_not_found)?;
    Ok(json(
        200,
        &Value::Object(object! {"user" => user.to_json()}),
    ))
}

/// `POST /admin/update-user {userId, data}` → `{user}`.
pub async fn update_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    let Value::Object(data) = data else {
        return Err(AuthError::invalid_request("data must be an object"));
    };
    auth.require_user(&user_id).await?;
    let mut changes = mosql::Object::new();
    for (key, value) in data.iter() {
        if !USER_FIELDS.contains(&key.as_str()) || key == "id" {
            continue;
        }
        match (key.as_str(), &value) {
            ("email", Value::Str(email)) => {
                let email = check_email(email)?;
                if let Some(other) = auth.user_by_email(&email).await? {
                    if other.id != user_id {
                        return Err(AuthError::bad_request(
                            "USER_ALREADY_EXISTS",
                            "A user with this email already exists",
                        ));
                    }
                }
                changes.set("email", s(email));
            }
            ("role", Value::Str(role)) if matches!(role.as_str(), "user" | "admin") => {
                changes.set("role", s(role.clone()));
            }
            ("banned", Value::Bool(banned)) => {
                changes.set("banned", b(*banned));
                if !*banned {
                    changes.set("banReason", Value::Null);
                    changes.set("banExpires", Value::Null);
                }
            }
            ("banned", _) => return Err(AuthError::invalid_request("banned must be a boolean")),
            ("role", _) => return Err(AuthError::invalid_request("role must be user or admin")),
            (k, Value::Str(text)) if k == "name" || k == "image" || k == "username" => {
                if k == "username" {
                    let username = check_username(text)?;
                    changes.set("username", s(username.to_ascii_lowercase()));
                    changes.set("displayUsername", s(username));
                } else if text.is_empty() {
                    changes.set(k, Value::Null);
                } else {
                    changes.set(k, s(text.clone()));
                }
            }
            (k, Value::Bool(flag)) if k == "emailVerified" => {
                changes.set(k, b(*flag));
            }
            _ => {}
        }
    }
    if changes.iter().next().is_none() {
        return Err(AuthError::invalid_request("data has nothing to update"));
    }
    let updated = auth.update_user(&user_id, changes).await?;
    Ok(json(
        200,
        &Value::Object(object! {"user" => updated.to_json()}),
    ))
}

/// `POST /admin/set-role {userId, role}` → `{user}`.
pub async fn set_role(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    let role = field(&body, "role")?;
    if !matches!(role.as_str(), "user" | "admin") {
        return Err(AuthError::invalid_request("role must be user or admin"));
    }
    let updated = auth
        .update_user(&user_id, object! {"role" => s(role)})
        .await?;
    Ok(json(
        200,
        &Value::Object(object! {"user" => updated.to_json()}),
    ))
}

/// `POST /admin/set-user-password {userId, newPassword}` → `{status: true}`,
/// and the user's sessions end.
pub async fn set_user_password(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    auth.require_user(&user_id).await?;
    let password = field(&body, "newPassword")?;
    auth.set_password(&user_id, &password).await?;
    auth.revoke_user_sessions(&user_id, None).await?;
    crate::routes::ok_status()
}

/// `POST /admin/ban-user {userId, banReason?, banExpiresIn?}` → `{user}`.
pub async fn ban_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    auth.require_user(&user_id).await?;
    let reason = opt_field(&body, "banReason").unwrap_or_else(|| "No reason".into());
    let mut changes = object! {
        "banned" => b(true),
        "banReason" => s(reason),
    };
    match body.get("banExpiresIn").and_then(Value::as_f64) {
        Some(secs) if secs > 0.0 => changes.set("banExpires", s(iso(now_secs() + secs as i64))),
        _ => changes.set("banExpires", Value::Null),
    }
    let updated = auth.update_user(&user_id, changes).await?;
    auth.revoke_user_sessions(&user_id, None).await?;
    Ok(json(
        200,
        &Value::Object(object! {"user" => updated.to_json()}),
    ))
}

/// `POST /admin/unban-user {userId}` → `{user}`.
pub async fn unban_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    auth.require_user(&user_id).await?;
    let updated = auth
        .update_user(
            &user_id,
            object! {
                "banned" => b(false),
                "banReason" => Value::Null,
                "banExpires" => Value::Null,
            },
        )
        .await?;
    Ok(json(
        200,
        &Value::Object(object! {"user" => updated.to_json()}),
    ))
}

/// `POST /admin/list-user-sessions {userId}` → `{sessions}`.
pub async fn list_user_sessions(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    auth.require_user(&user_id).await?;
    let sessions = live_sessions(auth, &user_id).await?;
    Ok(json(
        200,
        &Value::Object(
            object! {"sessions" => Value::Array(sessions.iter().map(|s| s.to_json()).collect())},
        ),
    ))
}

/// `POST /admin/revoke-user-session {sessionId}` → `{success: true}`.
pub async fn revoke_user_session(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let session_id = field(&body, "sessionId")?;
    let found = auth
        .store
        .find_one_typed::<Session>(&[WhereClause::id(&session_id)])
        .await?
        .ok_or_else(|| AuthError::not_found("SESSION_NOT_FOUND", "Session not found"))?;
    auth.revoke_session(&found.id).await?;
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}

/// `POST /admin/revoke-user-sessions {userId}` → `{success: true}`.
pub async fn revoke_user_sessions(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    auth.require_user(&user_id).await?;
    auth.revoke_user_sessions(&user_id, None).await?;
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}

/// `POST /admin/impersonate-user {userId}` → `{session, user}` and a
/// token for the target, marked with the admin.
pub async fn impersonate_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let admin: AuthUser = require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    let target_user = auth.require_user(&user_id).await?;
    if target_user.id == admin.user_id() {
        return Err(AuthError::bad_request(
            "CANNOT_IMPERSONATE_SELF",
            "You are already signed in as yourself",
        ));
    }
    if let Some(reason) = target_user.ban_in_force(&now_iso()) {
        return Err(AuthError::forbidden("USER_BANNED", reason));
    }
    let (session, refresh) = auth
        .create_session(&target_user, req, Some(admin.user_id()))
        .await?;
    let access = auth.sign_access(&target_user, &session.id, Some(admin.user_id()));
    Ok(Response::json(
        200,
        &Value::Object(object! {
            "session" => session.to_json(),
            "user" => target_user.to_json(),
            "refreshToken" => s(refresh),
        }),
    )
    .with_header(TOKEN_HEADER, &access))
}

/// `POST /admin/stop-impersonating` → the admin's own fresh `{session, user}`.
pub async fn stop_impersonating(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    // The caller holds the impersonated token; the admin is named in `imp`.
    // An admin who is not impersonating simply gets a fresh session.
    let who = authenticate(auth, req)?;
    let admin_id = who
        .impersonated_by()
        .unwrap_or_else(|| who.user_id())
        .to_string();
    let admin_user = auth.require_user(&admin_id).await?;
    if admin_user.role != "admin" {
        return Err(AuthError::forbidden(
            "FORBIDDEN",
            "Only administrators may do this",
        ));
    }
    let (session, refresh) = auth.create_session(&admin_user, req, None).await?;
    let access = auth.sign_access(&admin_user, &session.id, None);
    Ok(Response::json(
        200,
        &Value::Object(object! {
            "session" => session.to_json(),
            "user" => admin_user.to_json(),
            "refreshToken" => s(refresh),
        }),
    )
    .with_header(TOKEN_HEADER, &access))
}

/// `POST /admin/remove-user {userId}` → `{success: true}`.
pub async fn remove_user(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let admin = require_admin(auth, req)?;
    let body = body(req)?;
    let user_id = target(req, &body)?;
    if user_id == admin.user_id() {
        return Err(AuthError::bad_request(
            "CANNOT_REMOVE_SELF",
            "You cannot remove your own account here",
        ));
    }
    auth.require_user(&user_id).await?;
    auth.delete_user(&user_id).await?;
    Ok(json(200, &Value::Object(object! {"success" => b(true)})))
}
