//! The owner account, the platform file, and the rules that keep a password
//! from being stored or a login from leaking which emails exist.

// The serverless edition has no file store, no local database and no
// listener, so this test belongs to the full edition only.
#![cfg(feature = "native")]

use std::path::PathBuf;

use nanobase::crypto::random_id;
use nanobase::platform::{
    check_email, check_password, create_owner_with, iso8601, issue_token, normalize_email,
    verify_login, FileStore, MemoryStore, PasswordHash, Platform, PlatformStore,
};

/// A cheap iteration count. The production one is 600,000 rounds, which is the
/// point in production and ruinous in a test suite.
const FAST: u32 = 1000;

/// [`create_owner`](nanobase::create_owner) at [`FAST`] rounds.
fn create_owner(
    platform: &mut Platform,
    email: &str,
    password: &str,
    now: i64,
) -> uflite::Result<nanobase::User> {
    create_owner_with(platform, email, password, now, FAST)
}
use nanobase::{AppDoc, MIN_PASSWORD_LEN};
use uflite::db::block_on;

/// A directory that does not collide with another test, and that is removed
/// afterwards. Named with a random id, never a timestamp: the clock has been
/// seen to repeat a microsecond on macOS.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("nanobase-test-{}", random_id()));
        std::fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_700_000_000;

// ------------------------------------------------------------------ email ---

#[test]
fn email_is_normalised() {
    assert_eq!(normalize_email("  Ada@Example.COM "), "ada@example.com");
}

#[test]
fn email_is_checked_for_shape() {
    for good in [
        "a@b.co",
        "ada.lovelace@example.com",
        "x@y.z",
        "user+tag@sub.example.org",
    ] {
        assert!(check_email(good).is_ok(), "{good} should be accepted");
    }
    for bad in [
        "", "plain", "a@", "@b.co", "a@b", "a@.com", "a@b.", "a b@c.co", "a@b.c o",
    ] {
        assert!(check_email(bad).is_err(), "{bad:?} should be refused");
    }
}

#[test]
fn email_length_is_bounded() {
    let long = format!("{}@example.com", "a".repeat(300));
    assert!(check_email(&long).is_err());
}

// --------------------------------------------------------------- password ---

#[test]
fn password_length_is_enforced() {
    assert!(check_password(&"x".repeat(MIN_PASSWORD_LEN - 1)).is_err());
    assert!(check_password(&"x".repeat(MIN_PASSWORD_LEN)).is_ok());
    assert!(check_password(&"x".repeat(1025)).is_err());
}

#[test]
fn a_password_hash_verifies_its_own_password() {
    // A low iteration count: these tests would otherwise take minutes.
    let hash = PasswordHash::hash_with("hunter2hunter2", 1000);
    assert!(hash.verify("hunter2hunter2"));
    assert!(!hash.verify("hunter2hunter3"));
    assert!(!hash.verify(""));
}

#[test]
fn two_hashes_of_the_same_password_differ() {
    let a = PasswordHash::hash_with("same-password", 1000);
    let b = PasswordHash::hash_with("same-password", 1000);
    assert_eq!(a.hash.len(), 32);
    assert_ne!(a.salt, b.salt, "the salt must be random per password");
    assert_ne!(a.hash, b.hash);
    // And both still verify.
    assert!(a.verify("same-password") && b.verify("same-password"));
}

#[test]
fn a_hash_round_trips_through_the_platform_file() {
    let dir = TempDir::new();
    let path = dir.path().join("platform.json");
    let store = FileStore::new(&path);

    let mut platform = Platform::default();
    platform.ensure_jwt_secret();
    let owner = create_owner(&mut platform, "ada@example.com", "correct horse", NOW).unwrap();
    let original = owner.password.clone();
    block_on(store.save(&platform)).unwrap();

    let loaded = block_on(store.load()).unwrap();
    assert_eq!(loaded.users.len(), 1);
    assert_eq!(loaded.users[0].password, original);
    assert!(loaded.users[0].password.verify("correct horse"));
    assert!(!loaded.users[0].password.verify("wrong horse"));
}

#[test]
fn the_hash_records_its_iteration_count() {
    let hash = PasswordHash::hash_with("pw", 1234);
    assert_eq!(hash.iterations, 1234);
    // A stored hash is verified with the count it was made with, so the
    // production count can be raised later without invalidating old hashes.
    assert!(hash.verify("pw"));
}

// ------------------------------------------------------------- onboarding ---

#[test]
fn a_platform_with_no_users_needs_setup() {
    assert!(Platform::default().needs_setup());
    let mut p = Platform::default();
    create_owner(&mut p, "ada@example.com", "correct horse", NOW).unwrap();
    assert!(!p.needs_setup());
    assert_eq!(p.owner().map(|u| u.email.as_str()), Some("ada@example.com"));
}

#[test]
fn there_is_exactly_one_owner() {
    let mut p = Platform::default();
    create_owner(&mut p, "ada@example.com", "correct horse", NOW).unwrap();
    let err = create_owner(&mut p, "grace@example.com", "correct horse", NOW)
        .expect_err("a second owner must be refused");
    assert_eq!(err.status(), 409);
    assert_eq!(p.users.len(), 1);
}

#[test]
fn a_bad_email_or_password_creates_nothing() {
    let mut p = Platform::default();
    assert!(create_owner(&mut p, "not-an-email", "correct horse", NOW).is_err());
    assert!(create_owner(&mut p, "ada@example.com", "short", NOW).is_err());
    assert!(
        p.users.is_empty(),
        "a failed setup must not half-create a user"
    );
}

#[test]
fn the_owner_gets_an_id_and_a_timestamp() {
    let mut p = Platform::default();
    let u = create_owner(&mut p, "ada@example.com", "correct horse", NOW).unwrap();
    assert!(u.id.starts_with('u') && u.id.len() == 33);
    assert_eq!(u.created_at, iso8601(NOW));
    assert_eq!(p.users[0].id, u.id);
}

// ------------------------------------------------------------------ login ---

fn signed_up() -> Platform {
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner(&mut p, "ada@example.com", "correct horse", NOW).unwrap();
    p
}

#[test]
fn login_with_the_right_credentials_works() {
    let p = signed_up();
    let user = verify_login(&p, "ada@example.com", "correct horse").expect("signed in");
    assert_eq!(user.email, "ada@example.com");
}

#[test]
fn login_is_case_insensitive_in_the_email() {
    let p = signed_up();
    assert!(verify_login(&p, "ADA@EXAMPLE.COM", "correct horse").is_ok());
}

#[test]
fn login_with_a_wrong_password_fails() {
    let p = signed_up();
    let err = verify_login(&p, "ada@example.com", "wrong horse").unwrap_err();
    assert_eq!(err.status(), 401);
}

#[test]
fn login_with_an_unknown_email_fails_the_same_way() {
    let p = signed_up();
    let unknown = verify_login(&p, "nobody@example.com", "whatever!!").unwrap_err();
    let wrong = verify_login(&p, "ada@example.com", "wrong horse").unwrap_err();
    // Same status and same message: the response must not say which.
    assert_eq!(unknown.status(), wrong.status());
    assert_eq!(unknown.to_string(), wrong.to_string());
    assert_eq!(unknown.status(), 401);
}

#[test]
fn an_empty_platform_refuses_every_login() {
    let p = Platform::default();
    assert!(verify_login(&p, "ada@example.com", "correct horse").is_err());
}

// ------------------------------------------------------------------ token ---

#[test]
fn a_session_token_round_trips() {
    let mut p = signed_up();
    p.ensure_jwt_secret();
    let user = p.owner().unwrap();
    let token = issue_token(user, p.jwt_key(), NOW, 3600);
    let claims = nanobase::jwt::decode(&token, p.jwt_key(), NOW).expect("valid");
    assert_eq!(claims.sub, user.id);
    assert_eq!(claims.email, user.email);
    assert_eq!(claims.role, nanobase::ROLE_OWNER);
}

#[test]
fn a_token_is_bound_to_this_platform() {
    let a = signed_up();
    let mut b = Platform::default();
    b.ensure_jwt_secret();
    create_owner(&mut b, "grace@example.com", "correct horse", NOW).unwrap();
    let token = issue_token(a.owner().unwrap(), a.jwt_key(), NOW, 3600);
    assert!(
        nanobase::jwt::decode(&token, b.jwt_key(), NOW).is_err(),
        "a token signed by one platform must not work on another"
    );
}

#[test]
fn the_signing_key_survives_a_restart() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("platform.json"));
    let mut p = Platform::default();
    p.ensure_jwt_secret();
    create_owner(&mut p, "ada@example.com", "correct horse", NOW).unwrap();
    block_on(store.save(&p)).unwrap();

    let reloaded = block_on(store.load()).unwrap();
    let token = issue_token(reloaded.owner().unwrap(), reloaded.jwt_key(), NOW, 3600);
    assert!(nanobase::jwt::decode(&token, reloaded.jwt_key(), NOW).is_ok());
    // And the stored key is the same one, not regenerated.
    assert_eq!(reloaded.jwt_secret, p.jwt_secret);
}

#[test]
fn the_signing_key_is_not_reused_across_platforms() {
    let mut a = Platform::default();
    let mut b = Platform::default();
    a.ensure_jwt_secret();
    b.ensure_jwt_secret();
    assert_ne!(a.jwt_secret, b.jwt_secret);
    assert!(!a.jwt_secret.is_empty());
    assert!(!a.ensure_jwt_secret(), "already had one");
}

// ------------------------------------------------------------------ store ---

#[test]
fn a_missing_platform_file_is_an_empty_platform() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("never-written.json"));
    let p = block_on(store.load()).unwrap();
    assert!(p.needs_setup());
    assert!(p.apps.is_empty());
}

#[test]
fn the_store_creates_its_directory() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("nested/deeper/platform.json"));
    let p = Platform::default();
    block_on(store.save(&p)).unwrap();
    assert!(dir.path().join("nested/deeper/platform.json").exists());
}

#[test]
fn the_platform_file_is_json_a_human_can_read() {
    let dir = TempDir::new();
    let path = dir.path().join("platform.json");
    let store = FileStore::new(&path);
    let p = signed_up();
    block_on(store.save(&p)).unwrap();

    let text = std::fs::read_to_string(&path).unwrap();
    let v = mosql::parse_json(&text).unwrap();
    assert!(v.get("jwtSecret").is_some());
    assert_eq!(
        v.get("users").and_then(|u| u.as_array()).map(|a| a.len()),
        Some(1)
    );
    // The password itself is nowhere in the file.
    assert!(
        !text.contains("correct horse"),
        "password stored in cleartext"
    );
    assert!(
        text.contains("iterations"),
        "the hash parameters travel with it"
    );
}

#[test]
fn a_corrupt_platform_file_is_an_error_not_a_silent_reset() {
    let dir = TempDir::new();
    let path = dir.path().join("platform.json");
    std::fs::write(&path, "{not json").unwrap();
    let store = FileStore::new(&path);
    assert!(block_on(store.load()).is_err());
}

#[test]
fn a_half_written_platform_file_is_an_error() {
    let dir = TempDir::new();
    let path = dir.path().join("platform.json");
    std::fs::write(&path, r#"{"users": [{"id": 5}]}"#).unwrap();
    let store = FileStore::new(&path);
    assert!(block_on(store.load()).is_err());
}

#[test]
fn the_memory_store_round_trips() {
    let store = MemoryStore::new();
    let mut p = signed_up();
    block_on(store.save(&p)).unwrap();
    assert_eq!(block_on(store.load()).unwrap(), p);

    p.users.clear();
    block_on(store.save(&p)).unwrap();
    assert!(block_on(store.load()).unwrap().needs_setup());
}

#[test]
fn apps_round_trip_through_the_store() {
    let dir = TempDir::new();
    let store = FileStore::new(dir.path().join("platform.json"));
    let mut p = signed_up();
    p.apps.push(nanobase::AppRecord {
        id: "app-1".to_string(),
        created_at: iso8601(NOW),
        doc: AppDoc::empty("todos"),
    });
    block_on(store.save(&p)).unwrap();

    let loaded = block_on(store.load()).unwrap();
    assert_eq!(loaded.apps.len(), 1);
    assert_eq!(
        loaded.app("app-1").map(|a| a.doc.app.name.as_str()),
        Some("todos")
    );
    assert!(loaded.app("nope").is_none());
}

// -------------------------------------------------------------------- time ---

#[test]
fn iso8601_known_instants() {
    let cases = [
        (0, "1970-01-01T00:00:00Z"),
        (1, "1970-01-01T00:00:01Z"),
        (86_399, "1970-01-01T23:59:59Z"),
        (86_400, "1970-01-02T00:00:00Z"),
        (1_000_000_000, "2001-09-09T01:46:40Z"),
        (1_700_000_000, "2023-11-14T22:13:20Z"),
        (4_102_444_800, "2100-01-01T00:00:00Z"),
    ];
    for (secs, want) in cases {
        assert_eq!(iso8601(secs), want, "{secs}");
    }
}

#[test]
fn iso8601_handles_a_leap_day() {
    // 2024-02-29 exists; 2023-02-29 does not.
    assert_eq!(iso8601(1_709_164_800), "2024-02-29T00:00:00Z");
    // 2023 has no leap day: the day after 2023-02-28 is 2023-03-01.
    assert_eq!(iso8601(1_677_542_400), "2023-02-28T00:00:00Z");
    assert_eq!(iso8601(1_677_628_800), "2023-03-01T00:00:00Z");
}
