//! OpenID Connect, end to end.
//!
//! A tiny provider answers a discovery document and a key set; the engine
//! reads them and checks an ID token against the key. The signature in the
//! test was made by OpenSSL with a real RSA key, so a check that passes
//! here is a check that works.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use better_auth_core::auth::{PlainHttpClient, SyncHttpClient};
use better_auth_core::oidc::{verify_id_token, Discovery, Jwk, Jwks};
use mosql::Value;

/// A 2048-bit RSA public key, and a token it signed.
const N_B64URL: &str = "wz85VXA4RVJc0ES4j8duGdTeWsSuDtJVrszrCmPp43Avkk7MVxBbtavNl6YtvqNt33vTq3ICFg6zY7gaYNEjoXD_bzt-StCbnAAHJHRwXHOEfTFTYMLqyIUKivhvk28BbSoBzE86UQsVz9TAdtNkcm8qtBCvlF1OT6QK6BtGGR4lm-w6xO5SVJFjLgesMZIAyvRLfbHKX5dSMX3HeSek6NYTVdREtbK_nygXy7vdi3IUnahSsq1fCp81OD_vTjidxgLIkftn5ZYyYyRVNby2JujRgOQImLOacjysSrBWXih6cFsmfBnXdOMSWOzPD1UXglbb8GCa_OjPBiRCGMM23w";
const E_B64URL: &str = "AQAB";
const HEADER_B64URL: &str = "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCIsImtpZCI6InRlc3Qta2V5In0";
const CLAIMS_B64URL: &str = "eyJpc3MiOiJodHRwczovL2lzc3Vlci5leGFtcGxlLmNvbSIsImF1ZCI6ImNsaWVudC0xMjMiLCJzdWIiOiJ1c2VyLTEiLCJleHAiOjQxMDI0NDQ4MDAsImlhdCI6MTcwMDAwMDAwMCwiZW1haWwiOiJhQGIuYyJ9";
const SIGNATURE_B64URL: &str = "X-ErE4_OudXCfOlWrwomueXikP4FbytKe4yTmrGa5q9LPVeLP_TDNoJsvEiCqXAIss99WUGvJ9HIgTnZj90g1t2D6tbEkxV0Garlai_hKSpl7qZhJdKPjR6nzskzYWZ_qh2TSP9vqkQwbfbAXeZdOJ8TltT8dxDatBCpQ0y2yzBwfS18cNl57jGEiDD5c882_wAMLzFS37jpig5qul8z5B74FKuobfArPNCOHe_72dRL9VihMqOJPNER5LSLpQOnoq2NKEzjqogYjFr7nQ_5cCNpbHMRl-pjOXeGrJfinYihuj0YFfH2KEfxm0HMA-muNE1GThgvRyUtJgrV_jW9cg";

/// The token the test key signed.
fn token() -> String {
    format!("{HEADER_B64URL}.{CLAIMS_B64URL}.{SIGNATURE_B64URL}")
}

/// The key set the provider publishes.
fn jwks() -> Jwks {
    Jwks {
        keys: vec![Jwk {
            kty: "RSA".into(),
            kid: Some("test-key".into()),
            alg: Some("RS256".into()),
            n: b64url(N_B64URL),
            e: b64url(E_B64URL),
        }],
    }
}

fn b64url(text: &str) -> Vec<u8> {
    better_auth_core::crypto::b64url_decode(text).expect("base64url")
}

/// A provider that answers a discovery document and a key set, and nothing
/// else.
struct Provider {
    port: u16,
}

impl Provider {
    fn start() -> Provider {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                std::thread::spawn(move || {
                    serve(&mut stream, port);
                });
            }
        });
        Provider { port }
    }

    /// The issuer the provider says it is.
    fn issuer(&self) -> String {
        "https://issuer.example.com".to_string()
    }

    /// Where the discovery document is published.
    fn discovery_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/.well-known/openid-configuration",
            self.port
        )
    }

    /// Where the key set is published.
    fn jwks_url(&self) -> String {
        format!("http://127.0.0.1:{}/jwks", self.port)
    }
}

fn serve(stream: &mut TcpStream, port: u16) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request_line = String::new();
    reader.read_line(&mut request_line).expect("request");
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header == "\r\n" || header.is_empty() {
            break;
        }
        let lower = header.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
    }
    if length > 0 {
        let mut body = vec![0u8; length];
        let _ = reader.read_exact(&mut body);
    }
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");
    let body = if path.starts_with("/.well-known/openid-configuration") {
        format!(
            r#"{{"issuer":"https://issuer.example.com","authorization_endpoint":"http://127.0.0.1:{port}/authorize","token_endpoint":"http://127.0.0.1:{port}/token","userinfo_endpoint":"http://127.0.0.1:{port}/userinfo","jwks_uri":"http://127.0.0.1:{port}/jwks","scopes_supported":["openid","email"]}}"#
        )
    } else if path.starts_with("/jwks") {
        format!(
            r#"{{"keys":[{{"kty":"RSA","kid":"test-key","alg":"RS256","n":"{N_B64URL}","e":"{E_B64URL}"}}]}}"#
        )
    } else {
        "{}".to_string()
    };
    let reply = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
}

#[test]
fn a_discovery_document_says_where_everything_is() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let discovery = uflite::db::block_on(Discovery::fetch(&provider.discovery_url(), &[], &client))
        .expect("discovery");
    assert_eq!(discovery.issuer, provider.issuer());
    assert!(discovery.token_endpoint.ends_with("/token"));
    assert_eq!(
        discovery.jwks_uri.as_deref(),
        Some(provider.jwks_url().as_str())
    );
    assert_eq!(discovery.scopes_supported, vec!["openid", "email"]);
}

#[test]
fn the_key_set_is_read() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let jwks = uflite::db::block_on(Jwks::fetch(&provider.jwks_url(), &[], &client)).expect("jwks");
    assert_eq!(jwks.keys.len(), 1);
    assert_eq!(jwks.keys[0].kid.as_deref(), Some("test-key"));
    assert_eq!(jwks.keys[0].n.len(), 256);
    assert_eq!(jwks.keys[0].e, vec![0x01, 0x00, 0x01]);
}

#[test]
fn an_id_token_signed_by_the_provider_checks_out() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let issuer = provider.issuer();
    let discovery = uflite::db::block_on(Discovery::fetch(&provider.discovery_url(), &[], &client))
        .expect("discovery");
    let jwks = uflite::db::block_on(Jwks::fetch(
        discovery.jwks_uri.as_deref().expect("jwks uri"),
        &[],
        &client,
    ))
    .expect("jwks");
    let claims = verify_id_token(&token(), &jwks, &issuer, "client-123", 1_800_000_000)
        .expect("the token checks out");
    assert_eq!(claims.get("sub").and_then(|v| v.as_str()), Some("user-1"));
    assert_eq!(claims.get("email").and_then(|v| v.as_str()), Some("a@b.c"));
}

#[test]
fn a_token_from_somewhere_else_is_refused() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let jwks = uflite::db::block_on(Jwks::fetch(&provider.jwks_url(), &[], &client)).expect("jwks");
    // The right key, but a token made for another issuer.
    let error = verify_id_token(
        &token(),
        &jwks,
        "https://other.example.com",
        "client-123",
        1_800_000_000,
    )
    .expect_err("refused");
    assert!(error.to_string().contains("issued by"));
}

#[test]
fn a_token_for_another_application_is_refused() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let issuer = provider.issuer();
    let jwks = uflite::db::block_on(Jwks::fetch(&provider.jwks_url(), &[], &client)).expect("jwks");
    let error = verify_id_token(&token(), &jwks, &issuer, "another-client", 1_800_000_000)
        .expect_err("refused");
    assert!(error
        .to_string()
        .contains("not issued for this application"));
}

#[test]
fn an_expired_token_is_refused() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let issuer = provider.issuer();
    let jwks = uflite::db::block_on(Jwks::fetch(&provider.jwks_url(), &[], &client)).expect("jwks");
    // The token expires in 2100; a time far past that is refused.
    let _ = &client;
    let error = verify_id_token(&token(), &jwks, &issuer, "client-123", 4_102_444_900)
        .expect_err("refused");
    assert!(error.to_string().contains("expired"));
}

#[test]
fn a_tampered_token_is_refused() {
    let provider = Provider::start();
    let client = SyncHttpClient(PlainHttpClient);
    let issuer = provider.issuer();
    let jwks = uflite::db::block_on(Jwks::fetch(&provider.jwks_url(), &[], &client)).expect("jwks");
    // One character of the signature changed.
    let mut parts: Vec<String> = token().split('.').map(str::to_string).collect();
    let signature = parts[2].clone();
    let flipped = if signature.starts_with('X') { 'Y' } else { 'X' };
    parts[2] = format!("{flipped}{}", &signature[1..]);
    let error = verify_id_token(
        &parts.join("."),
        &jwks,
        &issuer,
        "client-123",
        1_800_000_000,
    )
    .expect_err("refused");
    assert!(error.to_string().contains("signature"));
}

#[test]
fn a_token_signed_with_another_key_is_refused() {
    let provider = Provider::start();
    let issuer = provider.issuer();
    // The provider's key set, but a token that names a key it does not have.
    let mut jwks = jwks();
    jwks.keys[0].kid = Some("some-other-key".into());
    let error = verify_id_token(&token(), &jwks, &issuer, "client-123", 1_800_000_000)
        .expect_err("refused");
    assert!(error.to_string().contains("no key for the token"));
}

#[test]
fn a_token_that_is_not_signed_with_rsa_is_refused() {
    let provider = Provider::start();
    let issuer = provider.issuer();
    let header = better_auth_core::crypto::b64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let claims = better_auth_core::crypto::b64url_encode(br#"{"iss":"x","aud":"y"}"#);
    let forged = format!("{header}.{claims}.{}", SIGNATURE_B64URL);
    let error =
        verify_id_token(&forged, &jwks(), &issuer, "y", 1_800_000_000).expect_err("refused");
    assert!(error.to_string().contains("HS256"));
}

#[test]
fn the_claims_are_a_document() {
    let claims: Value = mosql::parse_json(r#"{"sub":"user-1"}"#).expect("json");
    assert_eq!(claims.get("sub").and_then(|v| v.as_str()), Some("user-1"));
}
