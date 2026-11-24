//! Anonymous plugin: a real user row with no credentials, so a first-time
//! visitor can start working and keep everything once they sign up.
//!
//! The upgrade happens in the sign-up and OAuth routes: a caller whose
//! token says `anon` keeps their user id when they acquire an email or a
//! linked provider.

use std::sync::Arc;

use crate::auth::Auth;
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::optional;
use crate::models::*;
use crate::routes::body;

/// The domain of placeholder addresses for anonymous users.
pub const ANONYMOUS_DOMAIN: &str = "anonymous.local";

/// `POST /sign-in/anonymous` → tokens for a fresh or continuing anonymous
/// user.
pub async fn sign_in(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    if !auth.config.allow_anonymous {
        return Err(AuthError::bad_request(
            "ANONYMOUS_DISABLED",
            "Anonymous sign-in is disabled on this server",
        ));
    }
    let _ = body(req)?;
    // Already anonymous? Continue that user rather than orphaning them.
    if let Some(who) = optional(auth, req).filter(|w| w.is_anonymous()) {
        if let Some(user) = auth.user_by_id(who.user_id()).await? {
            return auth.issue(&user, req, None).await;
        }
    }
    let id = new_id();
    let user = auth
        .save_user(&User {
            id: id.clone(),
            name: "Anonymous".into(),
            email: format!("{id}@{ANONYMOUS_DOMAIN}"),
            email_verified: false,
            image: None,
            username: None,
            display_username: None,
            is_anonymous: true,
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
    auth.issue(&user, req, None).await
}
