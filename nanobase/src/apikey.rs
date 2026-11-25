//! API keys: a credential a program can hold instead of the owner's
//! password.
//!
//! A session token is a *person*. It is minted when a person signs in,
//! it lasts a day, and it carries the owner's whole authority: it can
//! create apps, delete apps, read every row. Handing that token to a
//! front end, a CI job or a webhook is handing over the keys to the
//! kingdom.
//!
//! An API key is a *program*. It is created by the owner, on purpose,
//! with a name somebody will recognise later ("the deploy script"), and
//! with limits:
//!
//! * **a scope** — what the key may do: read apps, write rows, and so
//!   on. A key that may only read cannot write, whatever it is asked to
//!   do.
//! * **an app** — the key may be limited to one app. A key scoped to
//!   `todos` cannot see `billing`, and cannot list the platform's other
//!   apps either.
//! * **an expiry** — the key stops working on its own. A key with no
//!   expiry never stops working, which is why one with an expiry is the
//!   better default.
//!
//! The key itself is shown once, at the moment it is made, and never
//! again. What the platform keeps is a hash of it: the same bargain as
//! a password, but with a key that already has its entropy, so the hash
//! is a fast SHA-256 rather than a slow PBKDF2 — the slowness of
//! PBKDF2 is there to make guessing a *human* password expensive, and
//! there is nothing to guess about 256 random bits.

use mosql::{Object, Value};

use crate::crypto::{b64url_decode, b64url_encode, ct_eq, random_bytes, sha256};

/// The prefix every API key starts with.
///
/// It says what the credential is, it says "this is not a session
/// token", and it means a key that turns up in a log is recognisable as
/// a key.
pub const KEY_PREFIX: &str = "nbk_";

/// How many random bytes a key has. 256 bits: not a number anybody
/// guesses.
const KEY_BYTES: usize = 32;

/// Read the apps: list them, look at one.
pub const SCOPE_APPS_READ: &str = "apps:read";
/// Change the apps: create, rename, remove.
pub const SCOPE_APPS_WRITE: &str = "apps:write";
/// Read the rows of an app.
pub const SCOPE_ROWS_READ: &str = "rows:read";
/// Write the rows of an app.
pub const SCOPE_ROWS_WRITE: &str = "rows:write";
/// Everything, including creating and removing API keys.
pub const SCOPE_ADMIN: &str = "admin";

/// Every scope there is.
pub const SCOPES: [&str; 5] = [
    SCOPE_APPS_READ,
    SCOPE_APPS_WRITE,
    SCOPE_ROWS_READ,
    SCOPE_ROWS_WRITE,
    SCOPE_ADMIN,
];

/// What a key may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiKey {
    /// The key's own id, `key_…`.
    pub id: String,
    /// What somebody calls it. A name is what makes a key findable in a
    /// list six months later.
    pub name: String,
    /// The hash of the key. Never the key.
    pub hash: String,
    /// The first few characters of the key, so a list can show which key
    /// is which without showing the key.
    pub preview: String,
    /// The app this key belongs to.
    ///
    /// A key belongs to exactly one app: it is made for that app, it
    /// may only see that app, and it lives in that app's database.
    pub app: String,
    /// What it may do.
    pub scopes: Vec<String>,
    /// When it stops working, in Unix seconds.
    pub expires_at: Option<i64>,
    /// When it was made.
    pub created_at: String,
    /// When it was last used, in Unix seconds.
    pub last_used_at: Option<i64>,
    /// Whether it has been revoked.
    pub revoked: bool,
}

impl ApiKey {
    /// Make a key.
    ///
    /// Returns the key and its record. The key is in the answer and in
    /// nowhere else: the platform keeps only the hash, so this is the
    /// one time it can be read.
    pub fn create(
        name: &str,
        app: impl Into<String>,
        scopes: Vec<String>,
        expires_at: Option<i64>,
        now: i64,
    ) -> (ApiKey, String) {
        let material = random_bytes(KEY_BYTES);
        let secret = format!("{KEY_PREFIX}{}", b64url_encode(&material));
        let record = ApiKey {
            id: format!("key_{}", b64url_encode(&random_bytes(9))),
            name: name.to_string(),
            hash: hash_of(&secret),
            preview: preview_of(&secret),
            app: app.into(),
            scopes,
            expires_at,
            created_at: crate::platform::iso8601(now),
            last_used_at: None,
            revoked: false,
        };
        (record, secret)
    }

    /// Is this the key?
    ///
    /// The comparison is in constant time, so a caller cannot learn the
    /// hash one byte at a time by timing.
    pub fn verify(&self, secret: &str) -> bool {
        let candidate = hash_of(secret);
        match (b64url_decode(&candidate), b64url_decode(&self.hash)) {
            (Some(a), Some(b)) => ct_eq(&a, &b),
            _ => false,
        }
    }

    /// Has it stopped working?
    pub fn is_expired(&self, now: i64) -> bool {
        match self.expires_at {
            Some(at) => now >= at,
            None => false,
        }
    }

    /// May it do this?
    ///
    /// `admin` is everything. A key with no scopes at all may do
    /// nothing, which is the safe reading of an empty list.
    pub fn allows(&self, scope: &str) -> bool {
        if self.revoked {
            return false;
        }
        self.scopes.iter().any(|s| {
            s == scope
                || s == SCOPE_ADMIN
                || s == "*"
                || (s.ends_with('*') && scope.starts_with(&s[..s.len() - 1]))
        })
    }

    /// Is it limited to one app?
    ///
    /// Always: a key belongs to exactly one app.
    pub fn is_scoped_to_app(&self) -> bool {
        true
    }

    /// The key as the world sees it: never the hash.
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("id", Value::str(self.id.clone()));
        o.set("name", Value::str(self.name.clone()));
        o.set("preview", Value::str(self.preview.clone()));
        o.set("app", Value::str(self.app.clone()));
        o.set(
            "scopes",
            Value::Array(self.scopes.iter().map(|s| Value::str(s.clone())).collect()),
        );
        o.set(
            "expiresAt",
            match self.expires_at {
                Some(at) => Value::str(crate::platform::iso8601(at)),
                None => Value::Null,
            },
        );
        o.set("createdAt", Value::str(self.created_at.clone()));
        o.set(
            "lastUsedAt",
            match self.last_used_at {
                Some(at) => Value::str(crate::platform::iso8601(at)),
                None => Value::Null,
            },
        );
        o.set("revoked", Value::Bool(self.revoked));
        Value::Object(o)
    }

    /// The key as it is stored: with the hash, and with the two moments
    /// as plain seconds so they can be compared without parsing.
    pub fn to_stored_value(&self) -> Value {
        let mut o = match self.to_value() {
            Value::Object(o) => o,
            _ => Object::new(),
        };
        o.set("hash", Value::str(self.hash.clone()));
        o.set(
            "expiresAt",
            match self.expires_at {
                Some(at) => Value::Int(at),
                None => Value::Null,
            },
        );
        o.set(
            "lastUsedAt",
            match self.last_used_at {
                Some(at) => Value::Int(at),
                None => Value::Null,
            },
        );
        Value::Object(o)
    }

    /// Read a stored key.
    pub fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        let scopes = match o.get("scopes") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        Some(ApiKey {
            id: o.get("id")?.as_str()?.to_string(),
            name: o.get("name")?.as_str()?.to_string(),
            hash: o.get("hash")?.as_str()?.to_string(),
            preview: o.get("preview")?.as_str()?.to_string(),
            app: o.get("app")?.as_str()?.to_string(),
            scopes,
            expires_at: match o.get("expiresAt") {
                Some(Value::Int(at)) => Some(*at),
                _ => None,
            },
            created_at: o
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            last_used_at: match o.get("lastUsedAt") {
                Some(Value::Int(at)) => Some(*at),
                _ => None,
            },
            revoked: matches!(o.get("revoked"), Some(Value::Bool(true))),
        })
    }
}

/// A key from an older platform, where a key could name no app.
///
/// The platform used to keep every key in one list, and a key could be
/// made for the whole platform rather than for one app. A key now
/// belongs to an app and lives in that app's database, so a platform
/// file from before that change is read into this first: the record
/// itself, and the app it named — if it named one.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyKey {
    /// The key's own id.
    pub id: String,
    /// What somebody called it.
    pub name: String,
    /// The hash of the key.
    pub hash: String,
    /// The first few characters of the key.
    pub preview: String,
    /// The app the key named, when it named one.
    ///
    /// `None` is the old "a key for the whole platform", which the new
    /// model does not have: a key belongs to an app. Such a key waits
    /// until somebody says which app it is for.
    pub app: Option<String>,
    /// What it may do.
    pub scopes: Vec<String>,
    /// When it stops working.
    pub expires_at: Option<i64>,
    /// When it was made.
    pub created_at: String,
    /// When it was last used.
    pub last_used_at: Option<i64>,
    /// Whether it has been revoked.
    pub revoked: bool,
}

impl LegacyKey {
    /// Read a key from an older platform.
    pub fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        let scopes = match o.get("scopes") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            _ => Vec::new(),
        };
        Some(LegacyKey {
            id: o.get("id")?.as_str()?.to_string(),
            name: o.get("name")?.as_str()?.to_string(),
            hash: o.get("hash")?.as_str()?.to_string(),
            preview: o.get("preview")?.as_str()?.to_string(),
            app: o
                .get("app")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|app| !app.is_empty())
                .map(str::to_string),
            scopes,
            expires_at: match o.get("expiresAt") {
                Some(Value::Int(at)) => Some(*at),
                _ => None,
            },
            created_at: o
                .get("createdAt")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            last_used_at: match o.get("lastUsedAt") {
                Some(Value::Int(at)) => Some(*at),
                _ => None,
            },
            revoked: matches!(o.get("revoked"), Some(Value::Bool(true))),
        })
    }

    /// The key as the older platform kept it.
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("id", Value::str(self.id.clone()));
        o.set("name", Value::str(self.name.clone()));
        o.set("hash", Value::str(self.hash.clone()));
        o.set("preview", Value::str(self.preview.clone()));
        o.set(
            "app",
            match &self.app {
                Some(app) => Value::str(app.clone()),
                None => Value::Null,
            },
        );
        o.set(
            "scopes",
            Value::Array(self.scopes.iter().map(|s| Value::str(s.clone())).collect()),
        );
        o.set(
            "expiresAt",
            match self.expires_at {
                Some(at) => Value::Int(at),
                None => Value::Null,
            },
        );
        o.set("createdAt", Value::str(self.created_at.clone()));
        o.set(
            "lastUsedAt",
            match self.last_used_at {
                Some(at) => Value::Int(at),
                None => Value::Null,
            },
        );
        o.set("revoked", Value::Bool(self.revoked));
        Value::Object(o)
    }

    /// The key as it is now: bound to an app.
    ///
    /// The hash comes along, so a key that was working before the
    /// change keeps working: the secret that was shown once still is
    /// the secret that is accepted.
    pub fn bind(&self, app: &str) -> ApiKey {
        ApiKey {
            id: self.id.clone(),
            name: self.name.clone(),
            hash: self.hash.clone(),
            preview: self.preview.clone(),
            app: app.to_string(),
            scopes: self.scopes.clone(),
            expires_at: self.expires_at,
            created_at: self.created_at.clone(),
            last_used_at: self.last_used_at,
            revoked: self.revoked,
        }
    }
}

/// The hash of a key, as base64url of its SHA-256.
pub fn hash_of(secret: &str) -> String {
    b64url_encode(&sha256(secret.as_bytes()))
}

/// The first few characters of a key, for showing in a list.
///
/// Enough to tell two keys apart; far too little to use one. It is also
/// how the platform finds the app a presented key belongs to: the
/// directory is keyed by it, and a directory entry cannot be used as a
/// key.
pub fn preview_of(secret: &str) -> String {
    let material = secret.strip_prefix(KEY_PREFIX).unwrap_or(secret);
    let head: String = material.chars().take(6).collect();
    format!("{KEY_PREFIX}{head}…")
}

/// Is this string shaped like an API key?
///
/// A cheap test, used to tell a key from a session token without
/// decoding either.
pub fn looks_like_a_key(secret: &str) -> bool {
    secret.starts_with(KEY_PREFIX) && secret.len() > KEY_PREFIX.len() + 10
}

/// Check a scope against the list of scopes there are.
///
/// A scope nobody has heard of is a mistake, and a mistake in a
/// permission is worth refusing: a key that was meant to read rows and
/// was given `rows:reed` should do nothing, not everything.
pub fn is_known_scope(scope: &str) -> bool {
    SCOPES.contains(&scope) || scope == "*"
}

/// Check a list of scopes, and say which one is wrong.
pub fn first_unknown_scope(scopes: &[String]) -> Option<String> {
    scopes.iter().find(|scope| !is_known_scope(scope)).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_verifies_only_itself() {
        let (record, secret) = ApiKey::create("deploy", "app_1", vec![], None, 1_700_000_000);
        assert!(record.verify(&secret));
        assert!(!record.verify("nbk_something_else"));
        assert!(!record.verify(""));
    }

    #[test]
    fn a_key_never_stores_itself() {
        let (record, secret) = ApiKey::create("deploy", "app_1", vec![], None, 1_700_000_000);
        let stored = mosql::to_json(&record.to_stored_value());
        assert!(!stored.contains(&secret), "the key is not in the record");
        assert!(stored.contains(&record.hash), "the hash is");
    }

    #[test]
    fn a_key_expires_on_its_own() {
        let (record, _) = ApiKey::create("deploy", "app_1", vec![], Some(1_700_000_000), 0);
        assert!(!record.is_expired(1_699_999_999));
        assert!(record.is_expired(1_700_000_000));
        assert!(record.is_expired(1_700_000_001));
    }

    #[test]
    fn a_key_without_an_expiry_never_expires() {
        let (record, _) = ApiKey::create("deploy", "app_1", vec![], None, 0);
        assert!(!record.is_expired(i64::MAX));
    }

    #[test]
    fn scopes_are_exact() {
        let (record, _) = ApiKey::create(
            "read only",
            "app_1",
            vec![SCOPE_ROWS_READ.to_string()],
            None,
            0,
        );
        assert!(record.allows(SCOPE_ROWS_READ));
        assert!(!record.allows(SCOPE_ROWS_WRITE));
        assert!(!record.allows(SCOPE_APPS_READ));
    }

    #[test]
    fn an_admin_key_may_do_everything() {
        let (record, _) = ApiKey::create("root", "app_1", vec![SCOPE_ADMIN.to_string()], None, 0);
        assert!(record.allows(SCOPE_APPS_WRITE));
        assert!(record.allows(SCOPE_ROWS_WRITE));
        assert!(record.allows(SCOPE_ADMIN));
    }

    #[test]
    fn a_key_with_no_scopes_may_do_nothing() {
        let (record, _) = ApiKey::create("empty", "app_1", vec![], None, 0);
        assert!(!record.allows(SCOPE_ROWS_READ));
        assert!(!record.allows(SCOPE_ADMIN));
    }

    #[test]
    fn a_revoked_key_may_do_nothing() {
        let (mut record, _) =
            ApiKey::create("revoked", "app_1", vec![SCOPE_ADMIN.to_string()], None, 0);
        assert!(record.allows(SCOPE_ADMIN));
        record.revoked = true;
        assert!(!record.allows(SCOPE_ADMIN));
        assert!(!record.allows(SCOPE_ROWS_READ));
    }

    #[test]
    fn a_key_survives_the_round_trip() {
        let (record, _) = ApiKey::create(
            "deploy",
            "app_123",
            vec![SCOPE_ROWS_READ.to_string(), SCOPE_ROWS_WRITE.to_string()],
            Some(1_800_000_000),
            1_700_000_000,
        );
        let stored = record.to_stored_value();
        let back = ApiKey::from_value(&stored).expect("the key");
        assert_eq!(back.id, record.id);
        assert_eq!(back.name, record.name);
        assert_eq!(back.hash, record.hash);
        assert_eq!(back.app, record.app);
        assert_eq!(back.scopes, record.scopes);
        assert_eq!(back.expires_at, record.expires_at);
        assert_eq!(back.created_at, record.created_at);
    }

    #[test]
    fn a_key_is_shaped_like_a_key() {
        let (_, secret) = ApiKey::create("deploy", "app_1", vec![], None, 0);
        assert!(looks_like_a_key(&secret));
        assert!(!looks_like_a_key("eyJhbGciOiJIUzI1NiJ9.abc.def"));
        assert!(!looks_like_a_key("nbk_short"));
    }

    #[test]
    fn an_unknown_scope_is_caught() {
        assert!(first_unknown_scope(&[SCOPE_ROWS_READ.to_string()]).is_none());
        assert_eq!(
            first_unknown_scope(&["rows:reed".to_string()]),
            Some("rows:reed".to_string())
        );
    }

    #[test]
    fn the_preview_shows_only_the_beginning() {
        let (_, secret) = ApiKey::create("deploy", "app_1", vec![], None, 0);
        let (record, _) = ApiKey::create("deploy", "app_1", vec![], None, 0);
        assert!(record.preview.starts_with(KEY_PREFIX));
        assert!(record.preview.len() < secret.len());
    }
}
