//! Two-factor plugin: TOTP (RFC 6238), emailed one-time codes, and
//! single-use backup codes.
//!
//! The shared secret is sealed with AES-256-GCM under a key derived from
//! the server secret; only the seal is stored. Backup codes are stored as
//! SHA-256 digests and removed as they are used. Enabling is two steps —
//! `enable` returns the QR URI and the codes, `verify-totp` proves the
//! authenticator works and switches the factor on — so a mistyped secret
//! cannot lock anyone out.
//!
//! Verification routes accept either a pending token (completing a
//! sign-in) or a session (enabling, or a fresh check), and return a full
//! token pair either way.

use std::sync::Arc;

use mosql::Value;

use crate::auth::{Auth, TOKEN_HEADER};
use crate::error::{AuthError, AuthResult};
use crate::http::{Request, Response};
use crate::middleware::authenticate_for_second_factor;
use crate::models::*;
use crate::object;
use crate::routes::{body, email_otp, field, json};
use crate::store::AuthStoreExt;

const OTP_KIND: &str = "2fa";

fn not_enabled() -> AuthError {
    AuthError::bad_request(
        "TWO_FACTOR_NOT_ENABLED",
        "Two-factor authentication is not enabled",
    )
}

fn invalid_code() -> AuthError {
    AuthError::unauthorized("INVALID_TWO_FACTOR_CODE", "Invalid two-factor code")
}

fn seal_key(auth: &Auth) -> [u8; 32] {
    crate::crypto::two_factor_key(&auth.config.secret)
}

fn seal(auth: &Auth, secret: &[u8]) -> String {
    crate::crypto::aead_encrypt(&seal_key(auth), secret)
}

fn unseal(auth: &Auth, packed: &str) -> AuthResult<Vec<u8>> {
    crate::crypto::aead_decrypt(&seal_key(auth), packed)
        .ok_or_else(|| AuthError::internal("cannot open the stored two-factor secret"))
}

async fn row(auth: &Auth, user_id: &str) -> AuthResult<Option<TwoFactor>> {
    auth.store
        .find_one_typed::<TwoFactor>(&[crate::store::WhereClause::eq("userId", s(user_id))])
        .await
}

/// Ten codes of ten hex-free characters in two groups, `xxxxx-xxxxx`,
/// shown once. Drawn from the base32 alphabet so they are unambiguous
/// when typed.
pub fn new_backup_codes() -> Vec<String> {
    (0..10)
        .map(|_| {
            let bytes = crate::crypto::totp_secret();
            let text = crate::crypto::base32_encode(&bytes);
            let clean: String = text
                .chars()
                .filter(|c| *c != '=' && *c != '-')
                .take(10)
                .collect();
            format!("{}-{}", &clean[..5], &clean[5..])
        })
        .collect()
}

fn hash_codes(codes: &[String]) -> String {
    let hashes: Vec<Value> = codes
        .iter()
        .map(|c| {
            Value::str(crate::crypto::sha256_hex(
                c.trim().to_ascii_uppercase().as_bytes(),
            ))
        })
        .collect();
    mosql::to_json(&Value::Array(hashes))
}

fn stored_codes(row: &TwoFactor) -> Vec<String> {
    match mosql::parse_json(&row.backup_codes) {
        Ok(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// Create or replace the row with a fresh secret and codes.
async fn write_row(auth: &Auth, user_id: &str, secret: &[u8], codes: &[String]) -> AuthResult<()> {
    auth.store
        .delete_many(
            MODEL_TWO_FACTOR,
            &[crate::store::WhereClause::eq("userId", s(user_id))],
        )
        .await?;
    let record = TwoFactor {
        id: new_id(),
        user_id: user_id.to_string(),
        secret: seal(auth, secret),
        backup_codes: hash_codes(codes),
        extra: mosql::Object::new(),
    };
    auth.store.create_typed(&record).await?;
    Ok(())
}

async fn set_enabled(auth: &Auth, user_id: &str, enabled: bool) -> AuthResult<()> {
    auth.update_user(user_id, object! {"twoFactorEnabled" => b(enabled)})
        .await?;
    Ok(())
}

/// A password check for the sensitive operations, when there is one.
async fn require_password(auth: &Auth, user_id: &str, password: &str) -> AuthResult<()> {
    if auth
        .credential_account(user_id)
        .await?
        .is_some_and(|a| a.password.is_some())
        && !auth.verify_password(user_id, password).await?
    {
        return Err(AuthError::unauthorized(
            "INVALID_PASSWORD",
            "Invalid password",
        ));
    }
    Ok(())
}

/// After a good code: switch the factor on if this was the enabling
/// step, and sign the user in.
async fn complete(auth: &Auth, req: &Request, user_id: &str) -> AuthResult<Response> {
    let user = auth.require_user(user_id).await?;
    if let Some(reason) = user.ban_in_force(&now_iso()) {
        return Err(AuthError::forbidden("USER_BANNED", reason));
    }
    let user = if user.two_factor_enabled {
        user
    } else {
        set_enabled(auth, &user.id, true).await?;
        auth.require_user(&user.id).await?
    };
    let (session, refresh) = auth.create_session(&user, req, None).await?;
    let access = auth.sign_access(&user, &session.id, None);
    let mut o = match auth.token_json(&access, &refresh, &user) {
        Value::Object(o) => o,
        _ => unreachable!(),
    };
    o.set("status", b(true));
    Ok(Response::json(200, &Value::Object(o)).with_header(TOKEN_HEADER, &access))
}

// ------------------------------------------------------------------ routes --

/// `POST /two-factor/enable {password}` → `{totpURI, backupCodes}`.
pub async fn enable(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = crate::middleware::authenticate(auth, req)?;
    let body = body(req)?;
    require_password(auth, who.user_id(), &field(&body, "password")?).await?;
    let user = who.user(auth).await?;
    if user.two_factor_enabled {
        return Err(AuthError::bad_request(
            "TWO_FACTOR_ALREADY_ENABLED",
            "Two-factor authentication is already enabled",
        ));
    }
    let secret = crate::crypto::totp_secret();
    let codes = new_backup_codes();
    write_row(auth, &user.id, &secret, &codes).await?;
    let totp_uri = crate::crypto::totp_uri(&secret, &user.email, &auth.config.issuer);
    Ok(json(
        200,
        &Value::Object(object! {
            "totpURI" => s(totp_uri),
            "backupCodes" => Value::Array(codes.into_iter().map(s).collect()),
        }),
    ))
}

/// `POST /two-factor/disable {password}` → `{status: true}`.
pub async fn disable(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = crate::middleware::authenticate(auth, req)?;
    let body = body(req)?;
    require_password(auth, who.user_id(), &field(&body, "password")?).await?;
    if row(auth, who.user_id()).await?.is_none() {
        return Err(not_enabled());
    }
    auth.store
        .delete_many(
            MODEL_TWO_FACTOR,
            &[crate::store::WhereClause::eq("userId", s(who.user_id()))],
        )
        .await?;
    set_enabled(auth, who.user_id(), false).await?;
    crate::routes::ok_status()
}

/// `POST /two-factor/get-totp-uri {password}` → `{totpURI}`.
pub async fn get_totp_uri(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = crate::middleware::authenticate(auth, req)?;
    let body = body(req)?;
    require_password(auth, who.user_id(), &field(&body, "password")?).await?;
    let row = row(auth, who.user_id()).await?.ok_or_else(not_enabled)?;
    let user = who.user(auth).await?;
    let secret = unseal(auth, &row.secret)?;
    Ok(json(
        200,
        &Value::Object(
            object! {"totpURI" => s(crate::crypto::totp_uri(&secret, &user.email, &auth.config.issuer))},
        ),
    ))
}

/// `POST /two-factor/send-otp` → `{status: true}` and a mail.
pub async fn send_otp(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let _ = body(req)?;
    let (user_id, _) = authenticate_for_second_factor(auth, req)?;
    auth.require_user(&user_id).await?;
    if row(auth, &user_id).await?.is_none() {
        return Err(not_enabled());
    }
    let email = auth.require_user(&user_id).await?.email;
    email_otp::issue_code_for(auth, OTP_KIND, &user_id, &email, "Your two-factor code").await?;
    crate::routes::ok_status()
}

/// `POST /two-factor/verify-otp {code}` → tokens.
pub async fn verify_otp(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let (user_id, _) = authenticate_for_second_factor(auth, req)?;
    let code = field(&body, "code").or_else(|_| field(&body, "otp"))?;
    email_otp::check_code(auth, OTP_KIND, &user_id, &code, true).await?;
    complete(auth, req, &user_id).await
}

/// `POST /two-factor/verify-totp {code}` → tokens.
pub async fn verify_totp(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let (user_id, _) = authenticate_for_second_factor(auth, req)?;
    let code = field(&body, "code").or_else(|_| field(&body, "totpCode"))?;
    let row = row(auth, &user_id).await?.ok_or_else(not_enabled)?;
    let secret = unseal(auth, &row.secret)?;
    let now = now_secs();
    // One step of clock drift either side, as the RFC suggests.
    let ok = [-1, 0, 1].iter().any(|step| {
        crate::crypto::totp_verify(&secret, &code, now + step * crate::crypto::TOTP_STEP_SECS)
    });
    if !ok {
        return Err(invalid_code());
    }
    complete(auth, req, &user_id).await
}

/// `POST /two-factor/verify-backup-code {code}` → tokens, and the code is
/// spent.
pub async fn verify_backup_code(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let body = body(req)?;
    let (user_id, _) = authenticate_for_second_factor(auth, req)?;
    let code = field(&body, "code").or_else(|_| field(&body, "backupCode"))?;
    let row = row(auth, &user_id).await?.ok_or_else(not_enabled)?;
    let digest = crate::crypto::sha256_hex(code.trim().to_ascii_uppercase().as_bytes());
    let codes = stored_codes(&row);
    let Some(index) = codes
        .iter()
        .position(|c| crate::crypto::ct_eq(c.as_bytes(), digest.as_bytes()))
    else {
        return Err(invalid_code());
    };
    let remaining: Vec<String> = codes
        .into_iter()
        .enumerate()
        .filter(|(i, _)| *i != index)
        .map(|(_, c)| c)
        .collect();
    auth.store
        .update(
            MODEL_TWO_FACTOR,
            &[crate::store::WhereClause::id(&row.id)],
            object! {"backupCodes" => s(hash_codes_from(&remaining))},
        )
        .await?;
    complete(auth, req, &user_id).await
}

/// Serialise already-hashed codes back to the stored form.
fn hash_codes_from(digests: &[String]) -> String {
    mosql::to_json(&Value::Array(
        digests.iter().map(|d| Value::str(d.clone())).collect(),
    ))
}

/// `POST /two-factor/generate-backup-codes {password}` → new codes.
pub async fn generate_backup_codes(auth: &Arc<Auth>, req: &Request) -> AuthResult<Response> {
    let who = crate::middleware::authenticate(auth, req)?;
    let body = body(req)?;
    require_password(auth, who.user_id(), &field(&body, "password")?).await?;
    let row = row(auth, who.user_id()).await?.ok_or_else(not_enabled)?;
    let codes = new_backup_codes();
    auth.store
        .update(
            MODEL_TWO_FACTOR,
            &[crate::store::WhereClause::id(&row.id)],
            object! {"backupCodes" => s(hash_codes(&codes))},
        )
        .await?;
    Ok(json(
        200,
        &Value::Object(object! {
            "status" => b(true),
            "backupCodes" => Value::Array(codes.into_iter().map(s).collect()),
        }),
    ))
}
