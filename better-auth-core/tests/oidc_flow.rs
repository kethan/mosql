//! §7.3 — OpenID Connect, end to end.
//!
//! A provider that speaks OIDC publishes what it is and the keys it signs
//! with. This drives the whole round trip against a stub provider that
//! does exactly that: the redirect goes to the discovered endpoint, the
//! callback checks the ID token against the discovered key set, and a
//! token that does not check out never becomes a user.
//!
//! The key is a real one, and the stub signs with it, so the check is the
//! real check.

mod common;

use std::sync::{Arc, Mutex};

use better_auth_core::async_test;
use better_auth_core::auth::{
    Auth, AuthConfig, HttpClient, HttpRequest, HttpResponse, SyncHttpClient,
};
use better_auth_core::bigint::BigUint;
use better_auth_core::oauth_presets::{CustomProvider, OAuthRegistry, ProviderConfig};
use better_auth_core::store::MemoryStore;
use common::*;
use mosql::Value;

/// A 1024-bit test key. Public in the provider's key set, private here.
const N_B64URL: &str = "u3-EXJ7Xlg5gPR9xzzvzebIWrInOkoHfqLlJXcDockNb9yVGtKIDYG2BFyKukesK3oYZHC4a9YSEfDSCY10vRF5CsRPdnn_3FQqJ6bu6GU_AsXSvJoayVabUjFOuLrseBmKPbeRYSurK00SXDR2ZzER2lJrdP3mXC85VJDvrrG0";
const D_B64URL: &str = "W2hJURYnKAKOLH4977fcdQG7SBZO3OHv9cOO3uQC7mGkXhKQK9OoLcR04KCJ1p1tyRo696AKiJM4O40l2-c7-RR6UbKX30_ikfi5ojM8itYuC4r4Gk1h7RcztECTRYb2qY286CCsD90mVqyRXUkwE8GlG12ODqlzX98zV87b3EE";
const E_B64URL: &str = "AQAB";

/// The prefix of a SHA-256 digest, as PKCS#1 v1.5 wants it.
const DIGEST_INFO: &[u8] = &[
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

fn b64url(text: &str) -> Vec<u8> {
    better_auth_core::crypto::b64url_decode(text).expect("base64url")
}

/// Sign a message the way PKCS#1 v1.5 does, with the test key.
fn sign(message: &[u8]) -> Vec<u8> {
    let modulus = BigUint::from_bytes_be(&b64url(N_B64URL));
    let exponent = BigUint::from_bytes_be(&b64url(D_B64URL));
    let digest = better_auth_core::crypto::sha256(message);
    let size = b64url(N_B64URL).len();
    let mut block = vec![0x00, 0x01];
    let padding = size - digest.len() - 3 - DIGEST_INFO.len();
    block.extend(std::iter::repeat_n(0xff, padding));
    block.push(0x00);
    block.extend_from_slice(DIGEST_INFO);
    block.extend_from_slice(&digest);
    let number = BigUint::mod_exp(&BigUint::from_bytes_be(&block), &exponent, &modulus);
    number
        .to_bytes_be_padded(size)
        .expect("the block fits the modulus")
}

/// A token signed with the test key.
fn signed_token(claims: &str, kid: &str) -> String {
    let header = format!(r#"{{"alg":"RS256","typ":"JWT","kid":"{kid}"}}"#);
    let signing_input = format!(
        "{}.{}",
        better_auth_core::crypto::b64url_encode(header.as_bytes()),
        better_auth_core::crypto::b64url_encode(claims.as_bytes())
    );
    let signature = better_auth_core::crypto::b64url_encode(&sign(signing_input.as_bytes()));
    format!("{signing_input}.{signature}")
}

/// A stub provider that speaks OIDC.
struct Provider {
    seen: Mutex<Vec<HttpRequest>>,
    issuer: String,
    /// Whether the ID token is signed by the key the key set names.
    good_signature: bool,
    /// Whether the discovery document names a key set.
    with_key_set: bool,
    /// The nonce the authorization request asked for, once the test has
    /// seen the URL.
    nonce: Mutex<Option<String>>,
}

impl Provider {
    fn new(issuer: &str) -> Provider {
        Provider {
            seen: Mutex::new(Vec::new()),
            issuer: issuer.to_string(),
            good_signature: true,
            with_key_set: true,
            nonce: Mutex::new(None),
        }
    }

    /// The provider publishes no key set.
    fn without_key_set(mut self) -> Provider {
        self.with_key_set = false;
        self
    }

    /// Remember the nonce the request asked for.
    fn set_nonce(&self, nonce: &str) {
        *self.nonce.lock().unwrap() = Some(nonce.to_string());
    }

    fn broken_signature(mut self) -> Provider {
        self.good_signature = false;
        self
    }

    fn seen(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }
}

impl HttpClient for Provider {
    fn request(&self, request: &HttpRequest) -> better_auth_core::error::AuthResult<HttpResponse> {
        self.seen.lock().unwrap().push(request.clone());
        let url = request.url.to_ascii_lowercase();
        if url.contains("openid-configuration") {
            let jwks = if self.with_key_set {
                format!(r#","jwks_uri":"{issuer}/jwks""#, issuer = self.issuer)
            } else {
                String::new()
            };
            let body = format!(
                r#"{{"issuer":"{issuer}","authorization_endpoint":"{issuer}/authorize","token_endpoint":"{issuer}/token","userinfo_endpoint":"{issuer}/userinfo"{jwks},"scopes_supported":["openid","email","profile"]}}"#,
                issuer = self.issuer,
                jwks = jwks
            );
            return Ok(HttpResponse::new(200, body.into_bytes()));
        }
        if url.contains("/jwks") {
            let body = format!(
                r#"{{"keys":[{{"kty":"RSA","kid":"key-1","alg":"RS256","n":"{N_B64URL}","e":"{E_B64URL}"}}]}}"#
            );
            return Ok(HttpResponse::new(200, body.into_bytes()));
        }
        if url.contains("/token") {
            // The nonce the authorization request asked for, so the token
            // can echo it back.
            let nonce = self.nonce.lock().unwrap().clone().unwrap_or_default();
            let nonce_claim = if nonce.is_empty() {
                String::new()
            } else {
                format!(r#","nonce":"{nonce}""#)
            };
            let claims = format!(
                r#"{{"iss":"{issuer}","aud":"client-1","sub":"user-1","email":"ada@x.co","name":"Ada","email_verified":true{nonce_claim}}}"#,
                issuer = self.issuer,
                nonce_claim = nonce_claim
            );
            let id_token = if self.good_signature {
                signed_token(&claims, "key-1")
            } else {
                // The same claims, signed by nothing.
                format!(
                    "{}.{}",
                    better_auth_core::crypto::b64url_encode(
                        br#"{"alg":"RS256","typ":"JWT","kid":"key-1"}"#
                    ),
                    better_auth_core::crypto::b64url_encode(claims.as_bytes())
                )
            };
            let body = format!(
                r#"{{"access_token":"at-1","token_type":"Bearer","expires_in":3600,"id_token":"{id_token}"}}"#
            );
            return Ok(HttpResponse::new(200, body.into_bytes()));
        }
        Ok(HttpResponse::new(
            200,
            br#"{"sub":"user-1","email":"ada@x.co"}"#.to_vec(),
        ))
    }
}

/// A rig with one provider, described by a discovery document.
fn rig(provider: Arc<Provider>) -> Rig {
    rig_with_headers(provider, Vec::new(), Vec::new())
}

/// A rig whose provider asks for headers of its own.
fn rig_with_headers(
    provider: Arc<Provider>,
    discovery_headers: Vec<(String, String)>,
    authorization_headers: Vec<(String, String)>,
) -> Rig {
    let custom = CustomProvider {
        id: "corp".into(),
        display_name: "Corp".into(),
        client_id: "client-1".into(),
        client_secret: "secret".into(),
        discovery_url: Some(format!(
            "{}/.well-known/openid-configuration",
            provider.issuer
        )),
        issuer: None,
        require_id_token_verification: true,
        disable_id_token_nonce_binding: false,
        discovery_headers,
        authorization_headers,
        ..CustomProvider::default()
    };
    let config = ProviderConfig::from_custom(&custom);
    let mut registry = OAuthRegistry::new();
    registry.insert(config);
    let mut auth_config = AuthConfig::new("https://app.test", SECRET);
    auth_config.password_scrypt_n = 1024;
    let auth = Auth::new(Arc::new(MemoryStore::new()), auth_config)
        .with_oauth(registry)
        .with_http(std::sync::Arc::new(SyncHttpClient(provider)));
    Rig {
        auth: Arc::new(auth),
        mailer: Arc::new(better_auth_core::auth::MemoryMailer::default()),
    }
}

/// The `state` in an authorization URL.
fn state_of(url: &str) -> String {
    url.split("state=")
        .nth(1)
        .expect("a state")
        .split('&')
        .next()
        .unwrap()
        .to_string()
}

/// The `nonce` in an authorization URL.
fn nonce_of(url: &str) -> Option<String> {
    let tail = url.split("nonce=").nth(1)?;
    Some(tail.split('&').next().unwrap().to_string())
}

async_test! {
    async fn the_redirect_goes_to_the_discovered_endpoint_with_a_nonce() {
        let provider = Arc::new(Provider::new("https://corp.example.com"));
        let rig = rig(provider.clone());
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let url = str_of(&json, "url");
        // The endpoint came from the discovery document, not from the
        // configuration.
        assert!(url.starts_with("https://corp.example.com/authorize?"), "{url}");
        assert!(url.contains("client_id=client-1"), "{url}");
        // A provider that publishes a key set binds the ID token to this
        // request.
        let nonce = nonce_of(&url).expect("a nonce");
        assert!(!nonce.is_empty());
    }
}

async_test! {
    async fn a_signed_id_token_becomes_a_user() {
        let provider = Arc::new(Provider::new("https://corp.example.com"));
        let rig = rig(provider.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        let url = str_of(&json, "url");
        let state = state_of(&url);
        // The provider echoes the nonce the request asked for.
        provider.set_nonce(&nonce_of(&url).expect("a nonce"));

        let (status, json, _) = rig
            .call(
                "GET",
                &format!("/api/auth/oauth2/callback/corp?code=abc&state={state}"),
                None,
                None,
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(json.get("token").is_some(), "{json:?}");
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
        // The token endpoint was the discovered one.
        assert!(provider
            .seen()
            .iter()
            .any(|r| r.url == "https://corp.example.com/token"));
    }
}

async_test! {
    async fn the_headers_a_provider_asks_for_are_sent() {
        let provider = Arc::new(Provider::new("https://corp.example.com"));
        let rig = rig_with_headers(
            provider.clone(),
            vec![("x-discovery".into(), "discovery-key".into())],
            vec![("x-gateway".into(), "token-key".into())],
        );
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        let url = str_of(&json, "url");
        let state = state_of(&url);
        provider.set_nonce(&nonce_of(&url).expect("a nonce"));
        let (status, json, _) = rig
            .call(
                "GET",
                &format!("/api/auth/oauth2/callback/corp?code=abc&state={state}"),
                None,
                None,
            )
            .await;
        assert_eq!(status, 200, "{json:?}");

        let seen = provider.seen();
        // The discovery request carried the header the provider asked for.
        let discovery = seen
            .iter()
            .find(|r| r.url.contains("openid-configuration"))
            .expect("a discovery request");
        assert!(
            discovery
                .headers
                .iter()
                .any(|(name, value)| name == "x-discovery" && value == "discovery-key"),
            "{:?}",
            discovery.headers
        );
        // And so did the token request.
        let token = seen
            .iter()
            .find(|r| r.url.ends_with("/token"))
            .expect("a token request");
        assert!(
            token
                .headers
                .iter()
                .any(|(name, value)| name == "x-gateway" && value == "token-key"),
            "{:?}",
            token.headers
        );
    }
}

async_test! {
    async fn an_id_token_that_does_not_check_out_is_refused() {
        let provider = Arc::new(Provider::new("https://corp.example.com").broken_signature());
        let rig = rig(provider.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        let url = str_of(&json, "url");
        let state = state_of(&url);

        let (status, json, _) = rig
            .call(
                "GET",
                &format!("/api/auth/oauth2/callback/corp?code=abc&state={state}"),
                None,
                None,
            )
            .await;
        assert_ne!(status, 200, "{json:?}");
        assert!(json.get("token").is_none(), "{json:?}");
    }
}

async_test! {
    async fn a_token_without_the_nonce_is_refused() {
        // The provider does not echo the nonce back.
        let provider = Arc::new(Provider::new("https://corp.example.com"));
        let rig = rig(provider.clone());
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        let url = str_of(&json, "url");
        let state = state_of(&url);

        let (status, json, _) = rig
            .call(
                "GET",
                &format!("/api/auth/oauth2/callback/corp?code=abc&state={state}"),
                None,
                None,
            )
            .await;
        assert_ne!(status, 200, "{json:?}");
        assert!(json.get("token").is_none(), "{json:?}");
    }
}

async_test! {
    async fn a_provider_with_no_key_set_is_refused_when_it_must_be_checked() {
        // The discovery document names no key set, and the provider was
        // configured to insist on a checked token: the sign-in never
        // starts.
        let provider = Arc::new(Provider::new("https://corp.example.com").without_key_set());
        let rig = rig(provider.clone());
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/sign-in/social",
                None,
                Some(obj(&[("provider", Value::str("corp"))])),
            )
            .await;
        assert_ne!(status, 200, "{json:?}");
        assert!(
            mosql::to_json(&json).contains("key set"),
            "{json:?}"
        );
    }
}
