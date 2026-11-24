//! Session tokens: what a token proves, and what it must refuse to prove.

use nanobase::crypto::{b64url_encode, random_bytes};
use nanobase::jwt::{decode, encode, Claims, ISSUER};

const SECRET: &[u8] = b"a-platform-signing-key";
const NOW: i64 = 1_700_000_000;
const TTL: i64 = 3600;

fn claims() -> Claims {
    Claims::new("u123", "ada@example.com", "owner", NOW, TTL)
}

#[test]
fn a_token_round_trips() {
    let token = encode(&claims(), SECRET);
    let decoded = decode(&token, SECRET, NOW).expect("valid");
    assert_eq!(decoded, claims());
}

#[test]
fn a_token_has_three_base64url_parts() {
    let token = encode(&claims(), SECRET);
    let parts: Vec<&str> = token.split('.').collect();
    assert_eq!(parts.len(), 3, "{token}");
    for part in parts {
        assert!(!part.is_empty());
        assert!(
            !part.contains('+') && !part.contains('/') && !part.contains('='),
            "{part}"
        );
    }
}

#[test]
fn the_header_names_the_algorithm_and_type() {
    let token = encode(&claims(), SECRET);
    let header = token.split('.').next().unwrap();
    let json = String::from_utf8(nanobase::crypto::b64url_decode(header).unwrap()).unwrap();
    assert!(json.contains(r#""alg":"HS256""#), "{json}");
    assert!(json.contains(r#""typ":"JWT""#), "{json}");
}

#[test]
fn the_claims_are_readable_but_not_writable() {
    // A JWT is not encrypted: the point of putting nothing sensitive in it.
    let token = encode(&claims(), SECRET);
    let payload = token.split('.').nth(1).unwrap();
    let json = String::from_utf8(nanobase::crypto::b64url_decode(payload).unwrap()).unwrap();
    assert!(json.contains("ada@example.com"), "{json}");
    assert!(json.contains(ISSUER), "{json}");
    assert!(!json.contains("secret"), "nothing sensitive: {json}");
}

#[test]
fn a_tampered_payload_is_refused() {
    let token = encode(&claims(), SECRET);
    let mut parts: Vec<String> = token.split('.').map(str::to_string).collect();
    let other = Claims::new("u999", "mallory@example.com", "owner", NOW, TTL);
    parts[1] = b64url_encode(
        mosql::to_json(&{
            let mut o = mosql::Object::new();
            o.set("sub", mosql::Value::str(other.sub.clone()));
            o.set("email", mosql::Value::str(other.email.clone()));
            o.set("role", mosql::Value::str(other.role.clone()));
            o.set("iss", mosql::Value::str(ISSUER));
            o.set("iat", mosql::Value::Int(other.iat));
            o.set("exp", mosql::Value::Int(other.exp));
            mosql::Value::Object(o)
        })
        .as_bytes(),
    );
    let forged = parts.join(".");
    assert!(
        decode(&forged, SECRET, NOW).is_err(),
        "forged token accepted"
    );
}

#[test]
fn a_wrong_secret_is_refused() {
    let token = encode(&claims(), SECRET);
    assert!(decode(&token, b"another-platform-signing-key", NOW).is_err());
}

#[test]
fn an_expired_token_is_refused() {
    let token = encode(&claims(), SECRET);
    assert!(decode(&token, SECRET, NOW).is_ok());
    assert!(decode(&token, SECRET, NOW + TTL - 1).is_ok(), "still valid");
    assert!(
        decode(&token, SECRET, NOW + TTL).is_err(),
        "exp is exclusive"
    );
    assert!(decode(&token, SECRET, NOW + TTL + 9999).is_err());
}

#[test]
fn alg_none_is_refused() {
    // The classic JWT bug: a token that says "no signature needed". It must
    // not be honoured, whatever the rest of the token says.
    let header = b64url_encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload = encode(&claims(), SECRET)
        .split('.')
        .nth(1)
        .unwrap()
        .to_string();
    let token = format!("{header}.{payload}.");
    assert!(decode(&token, SECRET, NOW).is_err());
    // And with a real signature attached, still refused.
    let token = format!("{header}.{payload}.{}", b64url_encode(&[0u8; 32]));
    assert!(decode(&token, SECRET, NOW).is_err());
}

#[test]
fn a_token_with_a_different_algorithm_is_refused() {
    let header = b64url_encode(br#"{"alg":"HS512","typ":"JWT"}"#);
    let payload = encode(&claims(), SECRET)
        .split('.')
        .nth(1)
        .unwrap()
        .to_string();
    let sig = encode(&claims(), SECRET)
        .split('.')
        .nth(2)
        .unwrap()
        .to_string();
    assert!(decode(&format!("{header}.{payload}.{sig}"), SECRET, NOW).is_err());
}

#[test]
fn a_token_from_another_issuer_is_refused() {
    let mut o = mosql::Object::new();
    o.set("sub", mosql::Value::str("u1"));
    o.set("email", mosql::Value::str("a@b.com"));
    o.set("role", mosql::Value::str("owner"));
    o.set("iss", mosql::Value::str("someone-else"));
    o.set("iat", mosql::Value::Int(NOW));
    o.set("exp", mosql::Value::Int(NOW + TTL));
    let payload = b64url_encode(mosql::to_json(&mosql::Value::Object(o)).as_bytes());
    let header = b64url_encode(br#"{"alg":"HS256","typ":"JWT"}"#);
    let signing_input = format!("{header}.{payload}");
    let sig = nanobase::crypto::hmac_sha256_parts(SECRET, &[signing_input.as_bytes()]);
    let token = format!("{signing_input}.{}", b64url_encode(&sig));
    assert!(
        decode(&token, SECRET, NOW).is_err(),
        "foreign issuer accepted"
    );
}

#[test]
fn malformed_tokens_are_refused() {
    let cases = ["", "not-a-token", "a.b", "a.b.c.d", "....", "!!!.???.***"];
    for token in cases {
        assert!(decode(token, SECRET, NOW).is_err(), "{token:?} accepted");
    }
}

#[test]
fn claims_carry_issued_and_expiry() {
    let c = claims();
    assert_eq!(c.iat, NOW);
    assert_eq!(c.exp, NOW + TTL);
    assert!(!c.is_expired(NOW));
    assert!(c.is_expired(NOW + TTL));
}

#[test]
fn a_longer_key_still_verifies() {
    let key = random_bytes(200);
    let token = encode(&claims(), &key);
    assert!(decode(&token, &key, NOW).is_ok());
    assert!(decode(&token, &key[..32], NOW).is_err());
}
