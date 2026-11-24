//! OpenID Connect: discovery, the key set, and checking an ID token.
//!
//! A provider that speaks OIDC publishes what it is at a well-known URL,
//! and its public keys at another. An ID token is a JWT signed with one of
//! those keys; checking it means finding the key, checking the signature
//! against it, and checking the claims say what they should.
//!
//! That is what this module does. Nothing here trusts a token it has not
//! checked: the signature is verified over the exact bytes that were
//! signed, and the issuer, audience and expiry are read from the claims
//! the signature covers.

use mosql::Value;

use crate::auth::{HttpClient, HttpRequest, HttpResponse};
use crate::bigint::BigUint;
use crate::error::{AuthError, AuthResult};

/// How much clock difference is allowed, in seconds.
///
/// A token that expired a moment ago is still valid, and a token that was
/// issued a moment from now is not yet valid; both happen on a network
/// with two clocks.
const LEEWAY: i64 = 60;

/// What a provider publishes at its well-known URL.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Discovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: Option<String>,
    pub jwks_uri: Option<String>,
    pub end_session_endpoint: Option<String>,
    pub scopes_supported: Vec<String>,
}

impl Discovery {
    /// The well-known URL for an issuer.
    ///
    /// The issuer may already carry a path — an issuer of
    /// `https://example.com/tenant` publishes at
    /// `https://example.com/tenant/.well-known/openid-configuration`.
    pub fn well_known_url(issuer: &str) -> String {
        let issuer = issuer.trim_end_matches('/');
        format!("{issuer}/.well-known/openid-configuration")
    }

    /// The discovery document as JSON.
    pub fn from_value(value: &Value) -> AuthResult<Discovery> {
        let issuer = value
            .get("issuer")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let authorization_endpoint = value
            .get("authorization_endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let token_endpoint = value
            .get("token_endpoint")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if token_endpoint.is_empty() {
            return Err(AuthError::internal(
                "the discovery document has no token endpoint",
            ));
        }
        Ok(Discovery {
            issuer,
            authorization_endpoint,
            token_endpoint,
            userinfo_endpoint: value
                .get("userinfo_endpoint")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            jwks_uri: value
                .get("jwks_uri")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            end_session_endpoint: value
                .get("end_session_endpoint")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            scopes_supported: value
                .get("scopes_supported")
                .and_then(|v| v.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// Fetch the discovery document.
    pub fn fetch(url: &str, client: &dyn HttpClient) -> AuthResult<Discovery> {
        let response = client.request(&HttpRequest {
            method: "GET".into(),
            url: url.to_string(),
            headers: vec![("accept".into(), "application/json".into())],
            body: Vec::new(),
        })?;
        Discovery::from_response(url, response)
    }

    /// Read a discovery document out of a reply.
    pub fn from_response(url: &str, response: HttpResponse) -> AuthResult<Discovery> {
        if response.status >= 400 {
            return Err(AuthError::internal(format!(
                "the discovery document at {url} answered {}",
                response.status
            )));
        }
        Discovery::from_value(&response.json()?)
    }
}

/// One public key.
#[derive(Debug, Clone, PartialEq)]
pub struct Jwk {
    /// The key type. Only `RSA` is used.
    pub kty: String,
    /// Which key this is, when the provider says.
    pub kid: Option<String>,
    /// The algorithm the key is for.
    pub alg: Option<String>,
    /// The modulus.
    pub n: Vec<u8>,
    /// The public exponent.
    pub e: Vec<u8>,
}

impl Jwk {
    /// The key as JSON.
    pub fn from_value(value: &Value) -> AuthResult<Jwk> {
        let kty = value
            .get("kty")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let n = value
            .get("n")
            .and_then(|v| v.as_str())
            .and_then(crate::crypto::b64url_decode)
            .ok_or_else(|| AuthError::internal("a key has no modulus"))?;
        let e = value
            .get("e")
            .and_then(|v| v.as_str())
            .and_then(crate::crypto::b64url_decode)
            .ok_or_else(|| AuthError::internal("a key has no exponent"))?;
        Ok(Jwk {
            kty,
            kid: value
                .get("kid")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            alg: value
                .get("alg")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            n,
            e,
        })
    }

    /// The modulus as a number.
    pub fn modulus(&self) -> BigUint {
        BigUint::from_bytes_be(&self.n)
    }

    /// The public exponent as a number.
    pub fn exponent(&self) -> BigUint {
        BigUint::from_bytes_be(&self.e)
    }
}

/// A provider's public keys.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

impl Jwks {
    /// The key set as JSON.
    pub fn from_value(value: &Value) -> AuthResult<Jwks> {
        let items = value
            .get("keys")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AuthError::internal("the key set has no keys"))?;
        let mut keys = Vec::with_capacity(items.len());
        for item in items {
            keys.push(Jwk::from_value(item)?);
        }
        Ok(Jwks { keys })
    }

    /// Fetch the key set.
    pub fn fetch(url: &str, client: &dyn HttpClient) -> AuthResult<Jwks> {
        let response = client.request(&HttpRequest {
            method: "GET".into(),
            url: url.to_string(),
            headers: vec![("accept".into(), "application/json".into())],
            body: Vec::new(),
        })?;
        Jwks::from_response(url, response)
    }

    /// Read a key set out of a reply.
    pub fn from_response(url: &str, response: HttpResponse) -> AuthResult<Jwks> {
        if response.status >= 400 {
            return Err(AuthError::internal(format!(
                "the key set at {url} answered {}",
                response.status
            )));
        }
        Jwks::from_value(&response.json()?)
    }

    /// The key to use for a token: the one with this `kid`, or the only one
    /// there is.
    pub fn key(&self, kid: Option<&str>) -> Option<&Jwk> {
        match kid {
            Some(kid) => self.keys.iter().find(|k| k.kid.as_deref() == Some(kid)),
            None => {
                if self.keys.len() == 1 {
                    self.keys.first()
                } else {
                    None
                }
            }
        }
    }
}

/// A provider's endpoints, from the document it publishes.
pub fn apply(
    config: &crate::oauth_presets::ProviderConfig,
    discovery: &Discovery,
) -> AuthResult<crate::oauth_presets::ProviderConfig> {
    let mut resolved = config.clone();
    let url = config.discovery_url.clone().unwrap_or_default();
    if !discovery.authorization_endpoint.is_empty() {
        resolved.authorization_url = discovery.authorization_endpoint.clone();
    }
    if !discovery.token_endpoint.is_empty() {
        resolved.token_url = discovery.token_endpoint.clone();
    }
    if resolved.userinfo_url.is_none() {
        resolved.userinfo_url = discovery.userinfo_endpoint.clone();
    }
    resolved.jwks_uri = discovery.jwks_uri.clone();
    if resolved.issuer.is_none() {
        resolved.issuer = Some(discovery.issuer.clone());
    }
    if config.require_id_token_verification
        && (resolved.issuer.as_deref().unwrap_or_default().is_empty()
            || resolved.jwks_uri.is_none())
    {
        return Err(AuthError::internal(format!(
            "{}: the discovery document at {url} does not name an issuer and a key set",
            config.id
        )));
    }
    Ok(resolved)
}

/// Check an ID token against a provider's key set, and return the claims.
///
/// The claims are only returned when they check out: the signature over
/// the exact bytes that were signed, the issuer, the audience and the
/// token's life.
pub fn verify_for(
    config: &crate::oauth_presets::ProviderConfig,
    client: &dyn HttpClient,
    id_token: &str,
    nonce: Option<&str>,
    now: i64,
) -> AuthResult<Value> {
    let jwks_uri = config.jwks_uri.as_deref().ok_or_else(|| {
        AuthError::internal(format!("{}: the provider has no key set", config.id))
    })?;
    let jwks = Jwks::fetch(jwks_uri, client)?;
    let issuer = config.issuer.as_deref().unwrap_or_default();
    let claims = verify_id_token(id_token, &jwks, issuer, &config.client_id, now)?;
    if !config.disable_id_token_nonce_binding {
        let claimed = claims.get("nonce").and_then(|v| v.as_str());
        match (nonce, claimed) {
            (Some(expected), Some(actual)) if expected == actual => {}
            (Some(_), None) => {
                return Err(AuthError::internal(
                    "the token does not carry the nonce the request asked for",
                ))
            }
            _ => {
                return Err(AuthError::internal(
                    "the token's nonce does not match the request",
                ))
            }
        }
    }
    Ok(claims)
}

/// The parts of an ID token.
pub struct ParsedToken {
    pub header: Value,
    pub claims: Value,
    /// The bytes that were signed: the header and the claims, joined by a
    /// dot, exactly as they were sent.
    pub signing_input: Vec<u8>,
    pub signature: Vec<u8>,
}

/// Split an ID token and read its header and claims.
///
/// The signature is not checked here; [`verify_id_token`] does that.
pub fn parse_token(token: &str) -> AuthResult<ParsedToken> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(AuthError::internal("the token is not a JWT"));
    }
    let header_bytes = nanobase::crypto::b64url_decode(parts[0])
        .ok_or_else(|| AuthError::internal("the token's header is not base64url"))?;
    let claims_bytes = nanobase::crypto::b64url_decode(parts[1])
        .ok_or_else(|| AuthError::internal("the token's claims are not base64url"))?;
    let signature = nanobase::crypto::b64url_decode(parts[2])
        .ok_or_else(|| AuthError::internal("the token's signature is not base64url"))?;
    let header = mosql::parse_json(&String::from_utf8_lossy(&header_bytes))
        .map_err(|e| AuthError::internal(format!("the token's header is not JSON: {e}")))?;
    let claims = mosql::parse_json(&String::from_utf8_lossy(&claims_bytes))
        .map_err(|e| AuthError::internal(format!("the token's claims are not JSON: {e}")))?;
    Ok(ParsedToken {
        header,
        claims,
        signing_input: format!("{}.{}", parts[0], parts[1]).into_bytes(),
        signature,
    })
}

/// Check an ID token: the signature, then the claims.
///
/// The issuer and the audience have to be the ones the caller expects, and
/// the token has to be within its life. `now` is the current time in
/// seconds since the Unix epoch.
pub fn verify_id_token(
    token: &str,
    jwks: &Jwks,
    issuer: &str,
    audience: &str,
    now: i64,
) -> AuthResult<Value> {
    let parsed = parse_token(token)?;
    let alg = parsed
        .header
        .get("alg")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if alg != "RS256" {
        return Err(AuthError::internal(format!(
            "the token is signed with {alg}, and only RS256 is checked"
        )));
    }
    let kid = parsed.header.get("kid").and_then(|v| v.as_str());
    let key = jwks
        .key(kid)
        .ok_or_else(|| AuthError::internal("the provider's key set has no key for the token"))?;
    if key.kty != "RSA" {
        return Err(AuthError::internal("the token's key is not an RSA key"));
    }
    verify_rs256(&parsed.signing_input, &parsed.signature, key)?;

    // The claims the signature covers.
    let claim = |name: &str| parsed.claims.get(name).and_then(|v| v.as_str());
    let token_issuer = claim("iss").unwrap_or_default();
    if !issuer.is_empty() && token_issuer != issuer {
        return Err(AuthError::internal(format!(
            "the token was issued by {token_issuer:?}, not {issuer:?}"
        )));
    }
    let audience_ok = match parsed.claims.get("aud") {
        Some(Value::Str(aud)) => aud == audience,
        Some(Value::Array(auds)) => auds.iter().any(|a| a.as_str() == Some(audience)),
        _ => false,
    };
    if !audience_ok {
        return Err(AuthError::internal(
            "the token was not issued for this application",
        ));
    }
    if let Some(exp) = parsed.claims.get("exp").and_then(|v| v.as_f64()) {
        if (exp as i64) + LEEWAY < now {
            return Err(AuthError::internal("the token has expired"));
        }
    }
    if let Some(nbf) = parsed.claims.get("nbf").and_then(|v| v.as_f64()) {
        if (nbf as i64) - LEEWAY > now {
            return Err(AuthError::internal("the token is not valid yet"));
        }
    }
    if let Some(iat) = parsed.claims.get("iat").and_then(|v| v.as_f64()) {
        if (iat as i64) - LEEWAY > now {
            return Err(AuthError::internal("the token was issued in the future"));
        }
    }
    Ok(parsed.claims)
}

/// Check an RS256 signature over `message` with `key`.
///
/// The signature is `s`; the message's digest is padded the way PKCS#1
/// v1.5 says, and the check is that `s^e mod n` is exactly that block.
pub fn verify_rs256(message: &[u8], signature: &[u8], key: &Jwk) -> AuthResult<()> {
    let digest = crate::crypto::sha256(message);
    let modulus = key.modulus();
    if modulus.is_zero() {
        return Err(AuthError::internal("the key's modulus is empty"));
    }
    let size = key.n.len();
    let mut expected = Vec::with_capacity(size);
    expected.push(0x00);
    expected.push(0x01);
    // The padding: at least eight 0xff bytes.
    let padding = size.saturating_sub(digest.len() + 3 + DIGEST_INFO.len());
    if padding < 8 {
        return Err(AuthError::internal("the key is too small for a signature"));
    }
    expected.extend(std::iter::repeat_n(0xff, padding));
    expected.push(0x00);
    expected.extend_from_slice(DIGEST_INFO);
    expected.extend_from_slice(&digest);

    let signature_number = BigUint::from_bytes_be(signature);
    if signature_number.cmp(&modulus) != std::cmp::Ordering::Less {
        return Err(AuthError::internal(
            "the signature is not smaller than the modulus",
        ));
    }
    let recovered = BigUint::mod_exp(&signature_number, &key.exponent(), &modulus);
    let block = recovered
        .to_bytes_be_padded(size)
        .ok_or_else(|| AuthError::internal("the signature does not recover a block"))?;
    if block == expected {
        Ok(())
    } else {
        Err(AuthError::internal(
            "the token's signature does not check out",
        ))
    }
}

/// The DER prefix of a SHA-256 digest, as PKCS#1 v1.5 wants it.
const DIGEST_INFO: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

#[cfg(test)]
mod tests {
    use super::*;
    use mosql::Object;

    fn obj(pairs: &[(&str, Value)]) -> Value {
        let mut o = Object::new();
        for (k, v) in pairs {
            o.set(*k, v.clone());
        }
        Value::Object(o)
    }

    #[test]
    fn the_well_known_url_keeps_the_issuer_path() {
        assert_eq!(
            Discovery::well_known_url("https://example.com"),
            "https://example.com/.well-known/openid-configuration"
        );
        assert_eq!(
            Discovery::well_known_url("https://example.com/tenant/"),
            "https://example.com/tenant/.well-known/openid-configuration"
        );
    }

    #[test]
    fn a_discovery_document_is_read() {
        let value = obj(&[
            ("issuer", Value::str("https://example.com")),
            (
                "authorization_endpoint",
                Value::str("https://example.com/auth"),
            ),
            ("token_endpoint", Value::str("https://example.com/token")),
            ("userinfo_endpoint", Value::str("https://example.com/me")),
            ("jwks_uri", Value::str("https://example.com/jwks")),
            (
                "scopes_supported",
                Value::Array(vec![Value::str("openid"), Value::str("email")]),
            ),
        ]);
        let discovery = Discovery::from_value(&value).expect("read");
        assert_eq!(discovery.issuer, "https://example.com");
        assert_eq!(discovery.token_endpoint, "https://example.com/token");
        assert_eq!(
            discovery.userinfo_endpoint.as_deref(),
            Some("https://example.com/me")
        );
        assert_eq!(discovery.scopes_supported.len(), 2);
    }

    #[test]
    fn a_discovery_document_without_a_token_endpoint_is_refused() {
        let value = obj(&[("issuer", Value::str("https://example.com"))]);
        assert!(Discovery::from_value(&value).is_err());
    }

    #[test]
    fn a_key_is_chosen_by_its_kid() {
        let mut jwks = Jwks::default();
        jwks.keys.push(Jwk {
            kty: "RSA".into(),
            kid: Some("one".into()),
            alg: Some("RS256".into()),
            n: vec![1, 2, 3],
            e: vec![1, 0, 1],
        });
        jwks.keys.push(Jwk {
            kty: "RSA".into(),
            kid: Some("two".into()),
            alg: Some("RS256".into()),
            n: vec![4, 5, 6],
            e: vec![1, 0, 1],
        });
        assert_eq!(
            jwks.key(Some("two")).map(|k| k.n.clone()),
            Some(vec![4, 5, 6])
        );
        assert!(jwks.key(Some("three")).is_none());
        // With more than one key, a token that names none cannot be checked.
        assert!(jwks.key(None).is_none());
    }

    #[test]
    fn a_token_is_split_into_its_parts() {
        // {"alg":"RS256","kid":"one"}.{"iss":"https://example.com"}.c2ln
        let token =
            "eyJhbGciOiJSUzI1NiIsImtpZCI6Im9uZSJ9.eyJpc3MiOiJodHRwczovL2V4YW1wbGUuY29tIn0.c2ln";
        let parsed = parse_token(token).expect("parsed");
        assert_eq!(
            parsed.header.get("alg").and_then(|v| v.as_str()),
            Some("RS256")
        );
        assert_eq!(
            parsed.claims.get("iss").and_then(|v| v.as_str()),
            Some("https://example.com")
        );
        assert_eq!(parsed.signature, b"sig".to_vec());
        assert!(parsed.signing_input.starts_with(b"eyJ"));
    }

    #[test]
    fn a_token_that_is_not_a_jwt_is_refused() {
        assert!(parse_token("nonsense").is_err());
    }
}
