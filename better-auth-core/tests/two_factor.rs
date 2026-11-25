//! §7.3 — Two-factor authentication.
//!
//! The whole lifecycle: enable, prove the authenticator, sign in again
//! and answer the second factor, then fall back to a backup code and
//! finally switch the factor off.

mod common;

use better_auth_core::async_test;
use better_auth_core::crypto;
use common::*;
use mosql::Value;

const PASSWORD: &str = "correct horse battery staple";

/// The base32 secret inside a `otpauth://` URI.
fn secret_of(uri: &str) -> String {
    uri.split("secret=")
        .nth(1)
        .expect("a secret in the URI")
        .split('&')
        .next()
        .unwrap()
        .to_string()
}

async_test! {
    async fn enable_then_verify_totp() {
        let rig = Rig::new();
        let (user, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let user_id = str_of(&user, "id");

        // Enabling needs the password and hands out the URI and the codes.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        let uri = str_of(&json, "totpURI");
        assert!(uri.starts_with("otpauth://totp/"), "{uri}");
        assert!(uri.contains("secret="), "{uri}");
        assert!(uri.contains("issuer="), "{uri}");
        let codes = json.get("backupCodes").unwrap().as_array().unwrap();
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|c| {
            let text = c.as_str().unwrap();
            text.len() == 11 && text.chars().nth(5) == Some('-')
        }), "{codes:?}");

        // The wrong password will not do.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str("wrong"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");

        // The factor is not on until the first code is accepted.
        let (_, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access), None).await;
        assert_eq!(json.get("user").unwrap().get("twoFactorEnabled"), Some(&Value::Bool(false)));

        // A wrong code is refused.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&access),
                Some(obj(&[("code", Value::str("000000"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TWO_FACTOR_CODE");

        // The right code switches the factor on and signs in.
        let code = crypto::totp(&crypto::base32_decode(&secret_of(&uri)).expect("a decodable secret"), better_auth_core::models::now_secs());
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&access),
                Some(obj(&[("code", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        assert!(json.get("token").is_some());
        let new_access = Rig::header_token(&response).unwrap().to_string();

        let (_, json, _) = rig.call("GET", "/api/auth/get-session", Some(&new_access), None).await;
        assert_eq!(json.get("user").unwrap().get("twoFactorEnabled"), Some(&Value::Bool(true)));

        // Now a plain sign-in stops at the second factor.
        let (status, json, response) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200, "{json:?}");
        let pending = Rig::header_token(&response).unwrap().to_string();
        let claims = rig.auth.decode(&pending).unwrap();
        assert!(claims.is_two_factor_pending());
        assert_eq!(claims.sub, user_id);
        // The pending token is no session: get-session stays empty.
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&pending), None).await;
        assert_eq!((status, json.clone()), (200, Value::Null));

        // The second factor completes the sign-in.
        let code = crypto::totp(&crypto::base32_decode(&secret_of(&uri)).expect("a decodable secret"), better_auth_core::models::now_secs());
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&pending),
                Some(obj(&[("code", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        let access2 = Rig::header_token(&response).unwrap().to_string();
        assert_ne!(access2, pending);
        let (status, json, _) = rig.call("GET", "/api/auth/get-session", Some(&access2), None).await;
        assert_eq!(status, 200);
        assert_eq!(str_of(json.get("user").unwrap(), "email"), "ada@x.co");
    }
}

async_test! {
    async fn backup_codes_are_single_use() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        let uri = str_of(&json, "totpURI");
        let codes: Vec<String> = json
            .get("backupCodes")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();

        // Turn the factor on.
        let code = crypto::totp(&crypto::base32_decode(&secret_of(&uri)).expect("a decodable secret"), better_auth_core::models::now_secs());
        let (status, _, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&access),
                Some(obj(&[("code", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200);

        // The verify route accepts a session as well as a pending token.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-backup-code",
                Some(&access),
                Some(obj(&[("code", Value::str(&codes[0]))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(json.get("token").is_some());

        // The same code twice: the second use fails.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-backup-code",
                Some(&access),
                Some(obj(&[("code", Value::str(&codes[0]))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        assert_eq!(str_of(&json, "code"), "INVALID_TWO_FACTOR_CODE");

        // The other codes still work.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-backup-code",
                Some(&access),
                Some(obj(&[("code", Value::str(&codes[1]))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
    }
}

async_test! {
    async fn emailed_otp_completes_the_sign_in() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        let uri = str_of(&json, "totpURI");
        let code = crypto::totp(&crypto::base32_decode(&secret_of(&uri)).expect("a decodable secret"), better_auth_core::models::now_secs());
        let (status, _, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&access),
                Some(obj(&[("code", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200);

        // Ask for a code by mail.
        let (status, json, _) = rig.call("POST", "/api/auth/two-factor/send-otp", Some(&access), Some(Value::Null)).await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        let (recipient, otp) = {
            let mails = rig.mailer.sent.lock().unwrap();
            let last = mails.last().expect("a mail");
            let otp = last
                .2
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()))
                .rfind(|w| w.len() == 6 && !w.is_empty())
                .expect("a six digit code")
                .to_string();
            (last.0.clone(), otp)
        };
        assert_eq!(recipient, "ada@x.co");

        // The wrong code fails, the right one signs in.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-otp",
                Some(&access),
                Some(obj(&[("code", Value::str("000000"))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
        let (status, json, response) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-otp",
                Some(&access),
                Some(obj(&[("code", Value::str(&otp))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert!(Rig::header_token(&response).is_some());
    }
}

async_test! {
    async fn disabling_turns_the_factor_off() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        let uri = str_of(&json, "totpURI");
        let code = crypto::totp(&crypto::base32_decode(&secret_of(&uri)).expect("a decodable secret"), better_auth_core::models::now_secs());
        let (status, _, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-totp",
                Some(&access),
                Some(obj(&[("code", Value::str(&code))])),
            )
            .await;
        assert_eq!(status, 200);

        // The URI can be fetched again, and it is the same secret.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/get-totp-uri",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(str_of(&json, "totpURI"), uri);

        // Turning it off.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/disable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));

        // A plain sign-in works again, with no second factor.
        let (status, json, response) = rig.sign_in_raw("ada@x.co", PASSWORD).await;
        assert_eq!(status, 200, "{json:?}");
        assert!(Rig::header_token(&response).is_some());

        // And the routes that need the factor now say it is not enabled.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/get-totp-uri",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        assert_eq!(status, 400, "{json:?}");
        assert_eq!(str_of(&json, "code"), "TWO_FACTOR_NOT_ENABLED");
    }
}

async_test! {
    async fn new_backup_codes_replace_the_old_ones() {
        let rig = Rig::new();
        let (_, access, _) = rig.sign_up("ada@x.co", PASSWORD).await;
        let (_, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/enable",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        let first: Vec<String> = json
            .get("backupCodes")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();

        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/generate-backup-codes",
                Some(&access),
                Some(obj(&[("password", Value::str(PASSWORD))])),
            )
            .await;
        assert_eq!(status, 200, "{json:?}");
        assert_eq!(json.get("status"), Some(&Value::Bool(true)));
        let second: Vec<String> = json
            .get("backupCodes")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_string())
            .collect();
        assert_eq!(second.len(), 10);
        assert_ne!(first, second);

        // An old code no longer opens the door.
        let (status, json, _) = rig
            .call(
                "POST",
                "/api/auth/two-factor/verify-backup-code",
                Some(&access),
                Some(obj(&[("code", Value::str(&first[0]))])),
            )
            .await;
        assert_eq!(status, 401, "{json:?}");
    }
}
