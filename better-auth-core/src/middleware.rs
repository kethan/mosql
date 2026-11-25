//! Getting a caller from a request, and where a browser may be sent
//! afterwards.
//!
//! Authentication here is deliberately cheap: decode the JWT in the
//! `Authorization: Bearer` header and hand back its claims. No database
//! read, no session lookup — a fifteen-minute token is believed for
//! fifteen minutes, and revocation shows up at the next refresh. Routes
//! that show or change a user row fetch it themselves.
//!
//! A token minted for a pending second factor proves only the password.
//! It opens the second-factor routes and nothing else.

use mosql::Value;

use crate::auth::Auth;
use crate::crypto::Claims;
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::models::{now_iso, User};
use crate::object;

/// A caller vouched for by an access token.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthUser {
    pub claims: Claims,
    /// The session row the refresh token lives on.
    pub session_id: String,
}

impl AuthUser {
    pub fn user_id(&self) -> &str {
        &self.claims.sub
    }

    pub fn email(&self) -> &str {
        self.claims.email.as_deref().unwrap_or("")
    }

    pub fn role(&self) -> &str {
        self.claims.role.as_deref().unwrap_or("user")
    }

    pub fn is_admin(&self) -> bool {
        self.role() == "admin"
    }

    pub fn is_anonymous(&self) -> bool {
        self.claims.anon
    }

    pub fn impersonated_by(&self) -> Option<&str> {
        self.claims.imp.as_deref()
    }

    /// The full user row. One read; call it only when the row's other
    /// fields are needed.
    pub async fn user(&self, auth: &Auth) -> AuthResult<User> {
        auth.require_user(self.user_id()).await
    }
}

/// The error a missing or unusable token produces.
pub fn unauthorized(message: &str) -> AuthError {
    AuthError::unauthorized("UNAUTHORIZED", message)
}

/// Decode a bearer token without judging what it is for.
fn decode_bearer(auth: &Auth, req: &Request) -> AuthResult<Claims> {
    let token = req
        .bearer_token()
        .ok_or_else(|| unauthorized("missing bearer token"))?;
    auth.decode(token)
}

/// The signed-in caller. Second-factor-pending tokens are refused here:
/// they are not sessions.
pub fn authenticate(auth: &Auth, req: &Request) -> AuthResult<AuthUser> {
    let claims = decode_bearer(auth, req)?;
    if claims.is_two_factor_pending() {
        return Err(AuthError::unauthorized(
            "TWO_FACTOR_PENDING",
            "Second factor required before this request is allowed",
        ));
    }
    let session_id = claims
        .sid
        .clone()
        .ok_or_else(|| AuthError::invalid_token("token has no session"))?;
    Ok(AuthUser { claims, session_id })
}

/// The caller if there is a usable token, `None` otherwise. For
/// `/get-session`, which answers `null` rather than 401 when signed out.
pub fn optional(auth: &Auth, req: &Request) -> Option<AuthUser> {
    authenticate(auth, req).ok()
}

/// An admin, or 403.
pub fn require_admin(auth: &Auth, req: &Request) -> AuthResult<AuthUser> {
    let user = authenticate(auth, req)?;
    if !user.is_admin() {
        return Err(AuthError::forbidden(
            "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
            "Only administrators may do this",
        ));
    }
    Ok(user)
}

/// A pending second-factor token: the password step is done, the second
/// step is not. Anything else is refused.
pub fn authenticate_pending(auth: &Auth, req: &Request) -> AuthResult<String> {
    let claims = decode_bearer(auth, req)?;
    if !claims.is_two_factor_pending() {
        return Err(AuthError::unauthorized(
            "TWO_FACTOR_PENDING",
            "Sign in with your password first to start second-factor verification",
        ));
    }
    Ok(claims.sub)
}

/// For the second-factor verification routes, which serve two flows: a
/// pending token completing a sign-in, or a signed-in user enabling or
/// re-checking a factor. Returns `(user id, pending)`.
pub fn authenticate_for_second_factor(auth: &Auth, req: &Request) -> AuthResult<(String, bool)> {
    let claims = decode_bearer(auth, req)?;
    if claims.is_two_factor_pending() {
        return Ok((claims.sub, true));
    }
    let session_id = claims
        .sid
        .clone()
        .ok_or_else(|| AuthError::invalid_token("token has no session"))?;
    let _ = session_id;
    Ok((claims.sub, false))
}

/// A 401 as a response, with the standard body.
pub fn unauthorized_response(error: &AuthError) -> Response {
    Response::error(error)
}

// ------------------------------------------------------------- redirects ----

/// Where a browser may be sent after a flow: a path on this origin, or an
/// absolute URL whose origin is this server's or one the host allowed.
/// Anything else is an open redirect and is refused.
pub fn validate_redirect(auth: &Auth, target: &str) -> AuthResult<String> {
    let target = target.trim();
    if target.is_empty() {
        return Err(AuthError::invalid_request("redirect target is empty"));
    }
    if target.starts_with('/') && !target.starts_with("//") {
        return Ok(format!("{}{}", auth.config.base_url, target));
    }
    let (scheme, rest) = target.split_once("://").ok_or_else(|| {
        AuthError::invalid_request("redirect target must be a path or an absolute URL")
    })?;
    if scheme != "https" && scheme != "http" {
        return Err(AuthError::invalid_request(
            "redirect target must use http or https",
        ));
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() {
        return Err(AuthError::invalid_request("redirect target has no host"));
    }
    let base_host = auth
        .config
        .base_url
        .split_once("://")
        .map(|(_, r)| r.split(['/', '?', '#']).next().unwrap_or(""))
        .unwrap_or("");
    let origin = format!("{scheme}://{host}");
    let permitted = origin.eq_ignore_ascii_case(&auth.config.base_url)
        || host.eq_ignore_ascii_case(base_host)
        || auth
            .config
            .allowed_origins
            .iter()
            .any(|a| a.eq_ignore_ascii_case(&origin));
    if !permitted {
        return Err(AuthError::invalid_request(format!(
            "redirect target {origin} is not a trusted origin"
        )));
    }
    Ok(target.to_string())
}

/// `callback?token=…&refreshToken=…`, whether the callback already has a
/// query string or not.
pub fn redirect_with_tokens(callback: &str, access: &str, refresh: &str) -> Response {
    let separator = if callback.contains('?') { '&' } else { '?' };
    let location = format!(
        "{callback}{separator}token={}&refreshToken={}",
        crate::http::percent_encode(access),
        crate::http::percent_encode(refresh)
    );
    Response::redirect(&location)
}

/// `callback?error=CODE&error_description=…` for a failed browser flow.
pub fn redirect_with_error(callback: &str, error: &AuthError) -> Response {
    let separator = if callback.contains('?') { '&' } else { '?' };
    Response::redirect(&format!(
        "{callback}{separator}error={}&error_description={}",
        crate::http::percent_encode(error.code),
        crate::http::percent_encode(&error.message)
    ))
}

/// Answer a browser from another origin when that origin is trusted.
pub fn cors(auth: &Auth, req: &Request, mut response: Response) -> Response {
    if let Some(origin) = req.header("origin") {
        let trusted = origin.eq_ignore_ascii_case(&auth.config.base_url)
            || auth
                .config
                .allowed_origins
                .iter()
                .any(|a| a.eq_ignore_ascii_case(origin));
        if trusted {
            response = response
                .with_header("access-control-allow-origin", origin)
                .with_header("access-control-allow-credentials", "true")
                .with_header("vary", "Origin");
        }
    }
    response
}

/// The user object a client gets from `/get-session`, built from claims
/// when the row is not needed.
pub fn user_from_claims(claims: &Claims) -> Value {
    let mut o = object! {
        "id" => Value::str(claims.sub.clone()),
        "email" => Value::str(claims.email.clone().unwrap_or_default()),
        "emailVerified" => Value::Bool(false),
        "name" => Value::str(""),
        "createdAt" => Value::str(now_iso()),
        "updatedAt" => Value::str(now_iso()),
        "role" => Value::str(claims.role.clone().unwrap_or_else(|| "user".into())),
        "banned" => Value::Bool(false),
        "isAnonymous" => Value::Bool(claims.anon),
        "twoFactorEnabled" => Value::Bool(false),
    };
    if let Some(imp) = &claims.imp {
        o.set("impersonatedBy", Value::str(imp.clone()));
    }
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_test;
    use crate::auth::AuthConfig;
    use crate::models::User;
    use crate::store::MemoryStore;
    use std::sync::Arc;

    fn auth() -> Auth {
        let mut config = AuthConfig::new("https://app.test", "0123456789abcdef0123456789abcdef");
        config.allowed_origins = vec!["https://portal.example.com".into()];
        Auth::new(Arc::new(MemoryStore::new()), config)
    }

    fn user() -> User {
        User {
            id: "u1".into(),
            name: "Ada".into(),
            email: "ada@x.co".into(),
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
        }
    }

    #[test]
    fn bearer_tokens_are_decoded_without_a_store_read() {
        let auth = auth();
        let ada = user();
        let access = auth.sign_access(&ada, "sess1", None);
        let req = Request::new("GET", "/api/auth/get-session")
            .with_header("Authorization", &format!("Bearer {access}"));
        let who = authenticate(&auth, &req).unwrap();
        assert_eq!(who.user_id(), "u1");
        assert_eq!(who.session_id, "sess1");
        assert!(who.is_admin());
        assert!(require_admin(&auth, &req).is_ok());

        // No header, bad scheme, garbage token.
        assert_eq!(
            authenticate(&auth, &Request::new("GET", "/x"))
                .unwrap_err()
                .status,
            401
        );
        assert_eq!(
            authenticate(
                &auth,
                &Request::new("GET", "/x").with_header("Authorization", "Basic abc")
            )
            .unwrap_err()
            .code,
            "UNAUTHORIZED"
        );
        assert_eq!(
            authenticate(
                &auth,
                &Request::new("GET", "/x").with_header("Authorization", "Bearer nope")
            )
            .unwrap_err()
            .code,
            "INVALID_TOKEN"
        );
        assert!(optional(&auth, &Request::new("GET", "/x")).is_none());
    }

    #[test]
    fn pending_tokens_only_open_second_factor_routes() {
        let auth = auth();
        let pending = auth.sign_pending("u1");
        let req = Request::new("POST", "/api/auth/two-factor/verify-totp")
            .with_header("Authorization", &format!("Bearer {pending}"));
        let err = authenticate(&auth, &req).unwrap_err();
        assert_eq!(err.status, 401);
        assert_eq!(err.code, "TWO_FACTOR_PENDING");
        assert_eq!(authenticate_pending(&auth, &req).unwrap(), "u1");
        assert_eq!(
            authenticate_for_second_factor(&auth, &req).unwrap(),
            ("u1".into(), true)
        );
        let ada = user();
        let access = auth.sign_access(&ada, "s", None);
        let req =
            Request::new("POST", "/x").with_header("Authorization", &format!("Bearer {access}"));
        assert_eq!(
            authenticate_pending(&auth, &req).unwrap_err().code,
            "TWO_FACTOR_PENDING"
        );
        assert_eq!(
            authenticate_for_second_factor(&auth, &req).unwrap(),
            ("u1".into(), false)
        );
    }

    #[test]
    fn non_admins_are_refused() {
        let auth = auth();
        let mut ada = user();
        ada.role = "user".into();
        let access = auth.sign_access(&ada, "s", None);
        let req =
            Request::new("POST", "/x").with_header("Authorization", &format!("Bearer {access}"));
        let err = require_admin(&auth, &req).unwrap_err();
        assert_eq!(err.status, 403);
        assert_eq!(err.code, "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS");
    }

    #[test]
    fn redirect_targets_are_checked() {
        let auth = auth();
        assert_eq!(
            validate_redirect(&auth, "/dash?x=1").unwrap(),
            "https://app.test/dash?x=1"
        );
        assert_eq!(
            validate_redirect(&auth, "https://app.test/cb").unwrap(),
            "https://app.test/cb"
        );
        assert_eq!(
            validate_redirect(&auth, "https://portal.example.com/x").unwrap(),
            "https://portal.example.com/x"
        );
        for bad in [
            "",
            "//evil.test",
            "https://evil.test",
            "javascript:alert(1)",
            "ftp://app.test",
            "app.test/x",
            "https://app.test.evil.com",
        ] {
            assert!(validate_redirect(&auth, bad).is_err(), "{bad}");
        }
        let r = redirect_with_tokens("https://app.test/cb?a=1", "t k", "r");
        assert_eq!(r.status, 302);
        assert_eq!(
            r.headers[0].1,
            "https://app.test/cb?a=1&token=t%20k&refreshToken=r"
        );
        let r = redirect_with_error("https://app.test/cb", &AuthError::invalid_request("nope"));
        assert!(r.headers[0].1.contains("error=INVALID_REQUEST"));
        assert!(r.headers[0].1.contains("error_description=nope"));
    }

    #[test]
    fn cors_headers_follow_the_origin() {
        let auth = auth();
        let req = Request::new("GET", "/x").with_header("Origin", "https://app.test");
        let r = cors(&auth, &req, Response::json(200, &Value::Null));
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "access-control-allow-origin" && v == "https://app.test"));
        let req = Request::new("GET", "/x").with_header("Origin", "https://portal.example.com");
        assert!(cors(&auth, &req, Response::json(200, &Value::Null))
            .headers
            .iter()
            .any(|(k, _)| k == "access-control-allow-origin"));
        let req = Request::new("GET", "/x").with_header("Origin", "https://other.test");
        let r = cors(&auth, &req, Response::json(200, &Value::Null));
        assert!(!r
            .headers
            .iter()
            .any(|(k, _)| k == "access-control-allow-origin"));
        let u = user_from_claims(&auth.decode(&auth.sign_access(&user(), "s", None)).unwrap());
        assert_eq!(u.get("role"), Some(&Value::str("admin")));
    }

    async_test! {
        async fn the_full_row_is_one_read_away() {
            let a = auth();
            let ada = user();
            a.save_user(&ada).await.unwrap();
            let access = a.sign_access(&ada, "s", None);
            let req = Request::new("GET", "/x").with_header("Authorization", &format!("Bearer {access}"));
            let who = authenticate(&a, &req).unwrap();
            assert_eq!(who.user(&a).await.unwrap().name, "Ada");
        }
    }
}
