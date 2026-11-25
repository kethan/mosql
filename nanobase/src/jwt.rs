//! Session tokens: JWT with HMAC-SHA256.
//!
//! A token is the session. There is no server-side session row to look up,
//! which is what lets a request be answered without touching a database — the
//! token says who the caller is and the signature proves the platform said so.
//!
//! What goes in the token matters as much as the signature. It carries the
//! caller's identity and the app they are acting on, **never** a database URL,
//! a storage key or a rule set. A JWT is signed but not encrypted: anything in
//! it is readable by whoever holds it, and a token is handed to a browser.
//! Per-app configuration is resolved server-side from the app id instead.
//!
//! ```text
//! eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ1YWJjIn0.signature
//!   header (base64url)   claims (base64url)   HMAC-SHA256 over both
//! ```

use mosql::{parse_json, Object, Value};
use uflite::error::{Error, Result};

use crate::crypto::{b64url_decode, b64url_encode, ct_eq, hmac_sha256_parts};

/// How the token is signed. Only HS256 exists; the header is written and
/// checked so a token claiming another algorithm is refused rather than
/// silently accepted with a different verification path.
const ALG: &str = "HS256";

/// Who issued the token, and who is expected to be checking it.
pub const ISSUER: &str = "nanobase";

/// A token's contents.
#[derive(Debug, Clone, PartialEq)]
pub struct Claims {
    /// The user's id.
    pub sub: String,
    /// The user's email, so a route can show it without a lookup.
    pub email: String,
    /// `owner` today. Kept as a string so adding a role is not a breaking
    /// change to tokens already issued.
    pub role: String,
    /// Issued at, in seconds since the Unix epoch.
    pub iat: i64,
    /// Expires at, in seconds since the Unix epoch.
    pub exp: i64,
}

impl Claims {
    /// Claims for a user who has just signed in.
    pub fn new(user_id: &str, email: &str, role: &str, now: i64, ttl_secs: i64) -> Self {
        Claims {
            sub: user_id.to_string(),
            email: email.to_string(),
            role: role.to_string(),
            iat: now,
            exp: now + ttl_secs,
        }
    }

    /// Whether the token is still valid at `now`.
    pub fn is_expired(&self, now: i64) -> bool {
        now >= self.exp
    }

    fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("sub", Value::str(self.sub.clone()));
        o.set("email", Value::str(self.email.clone()));
        o.set("role", Value::str(self.role.clone()));
        o.set("iss", Value::str(ISSUER));
        o.set("iat", Value::Int(self.iat));
        o.set("exp", Value::Int(self.exp));
        Value::Object(o)
    }

    fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(Claims {
            sub: o.get("sub")?.as_str()?.to_string(),
            email: o.get("email")?.as_str()?.to_string(),
            role: o.get("role")?.as_str()?.to_string(),
            iat: match o.get("iat")? {
                Value::Int(i) => *i,
                _ => return None,
            },
            exp: match o.get("exp")? {
                Value::Int(i) => *i,
                _ => return None,
            },
        })
    }
}

/// Sign `claims` with `secret` and return the token.
pub fn encode(claims: &Claims, secret: &[u8]) -> String {
    let header = format!(r#"{{"alg":"{ALG}","typ":"JWT"}}"#);
    let signing_input = format!(
        "{}.{}",
        b64url_encode(header.as_bytes()),
        b64url_encode(mosql::to_json(&claims.to_value()).as_bytes())
    );
    let sig = hmac_sha256_parts(secret, &[signing_input.as_bytes()]);
    format!("{signing_input}.{}", b64url_encode(&sig))
}

/// Check a token's signature, issuer and expiry, and return its claims.
///
/// Every failure is the same error to the caller: "not authenticated". Which
/// check failed is not information to give someone presenting a forged token.
pub fn decode(token: &str, secret: &[u8], now: i64) -> Result<Claims> {
    let unauthenticated = || Error::unauthorized("Not authenticated");

    let mut parts = token.split('.');
    let (Some(header_b64), Some(claims_b64), Some(sig_b64)) =
        (parts.next(), parts.next(), parts.next())
    else {
        return Err(unauthenticated());
    };
    if parts.next().is_some() {
        return Err(unauthenticated());
    }

    let header_json = b64url_decode(header_b64).ok_or_else(unauthenticated)?;
    let header =
        parse_json(&String::from_utf8_lossy(&header_json)).map_err(|_| unauthenticated())?;
    match header.get("alg").and_then(Value::as_str) {
        // Refuse anything but HS256 rather than guessing: the classic JWT
        // mistake is honouring `alg: none` or an attacker-chosen algorithm.
        Some(a) if a == ALG => {}
        _ => return Err(unauthenticated()),
    }

    let signing_input = format!("{header_b64}.{claims_b64}");
    let expected = hmac_sha256_parts(secret, &[signing_input.as_bytes()]);
    let presented = b64url_decode(sig_b64).ok_or_else(unauthenticated)?;
    if !ct_eq(&expected, &presented) {
        return Err(unauthenticated());
    }

    let claims_json = b64url_decode(claims_b64).ok_or_else(unauthenticated)?;
    let claims_value =
        parse_json(&String::from_utf8_lossy(&claims_json)).map_err(|_| unauthenticated())?;
    let claims = Claims::from_value(&claims_value).ok_or_else(unauthenticated)?;
    match claims_value.get("iss").and_then(Value::as_str) {
        Some(ISSUER) => {}
        _ => return Err(unauthenticated()),
    }
    if claims.is_expired(now) {
        return Err(unauthenticated());
    }
    Ok(claims)
}
