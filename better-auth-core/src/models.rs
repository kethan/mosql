//! The five models and the JSON they travel as.
//!
//! Every model is a plain row with a text id and UTC text timestamps, so the
//! same table definition runs on SQLite, MySQL and Postgres unchanged, and so
//! ISO-8601 strings compare correctly as strings — `expiresAt > now` needs no
//! parsing when both sides are written by the same `iso8601`.
//!
//! Records cross the [`AuthStore`](crate::store::AuthStore) boundary as
//! [`RecordMap`]s, so a store never has to know about Rust types. These
//! structs are the typed view on top: they convert to and from a record, and
//! carry any field the table does not declare in `extra`, so a caller's own
//! columns survive a round trip.

use mosql::{Object, Value};

use crate::error::{AuthError, AuthResult};

/// A record: an insertion-ordered JSON object.
pub type RecordMap = Object;

// ------------------------------------------------------------ JSON helpers ---

/// A JSON object from key/value pairs, in the order written.
#[macro_export]
macro_rules! object {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut o = mosql::Object::new();
        $( o.set($k, $v); )*
        o
    }};
}

/// A JSON array.
#[macro_export]
macro_rules! array {
    ($($v:expr),* $(,)?) => { Value::Array(vec![$($v),*]) };
}

pub fn s(v: impl Into<String>) -> Value {
    Value::str(v)
}

pub fn b(v: bool) -> Value {
    Value::Bool(v)
}

pub fn n(v: i64) -> Value {
    Value::Int(v)
}

/// A number the way JSON spells it: an integer when it is one.
pub fn f(v: f64) -> Value {
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        Value::Int(v as i64)
    } else {
        Value::Float(v)
    }
}

pub fn opt_s(v: &Option<String>) -> Value {
    match v {
        Some(x) => Value::str(x.clone()),
        None => Value::Null,
    }
}

pub fn get_str(r: &RecordMap, key: &str) -> Option<String> {
    r.get(key).and_then(Value::as_str).map(str::to_string)
}

/// A boolean as stored: `true`/`false`, or the 1/0 a SQL BOOLEAN column
/// hands back.
pub fn get_bool(r: &RecordMap, key: &str) -> Option<bool> {
    match r.get(key) {
        Some(Value::Bool(x)) => Some(*x),
        Some(Value::Int(x)) => Some(*x != 0),
        _ => None,
    }
}

pub fn get_i64(r: &RecordMap, key: &str) -> Option<i64> {
    match r.get(key) {
        Some(Value::Int(x)) => Some(*x),
        Some(Value::Float(x)) => Some(*x as i64),
        _ => None,
    }
}

pub fn get_array<'a>(r: &'a RecordMap, key: &str) -> Option<&'a Vec<Value>> {
    r.get(key).and_then(Value::as_array)
}

/// Dot-path lookup, `data.0.user.id` style: objects by key, arrays by index.
pub fn lookup<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.').filter(|s| !s.is_empty()) {
        current = match current {
            Value::Object(o) => o.get(segment)?,
            Value::Array(items) => {
                let index: usize = segment.parse().ok()?;
                items.get(index)?
            }
            _ => return None,
        };
    }
    Some(current)
}

/// The first of several dot-paths that yields a non-null string.
pub fn lookup_string(value: &Value, paths: &[&str]) -> Option<String> {
    paths
        .iter()
        .filter_map(|p| lookup(value, p).and_then(Value::as_str))
        .find(|s| !s.is_empty())
        .map(str::to_string)
}

// -------------------------------------------------------------- the models ---

pub const MODEL_USER: &str = "user";
pub const MODEL_SESSION: &str = "session";
pub const MODEL_ACCOUNT: &str = "account";
pub const MODEL_VERIFICATION: &str = "verification";
pub const MODEL_TWO_FACTOR: &str = "twoFactor";

pub const MODELS: [&str; 5] = [
    MODEL_USER,
    MODEL_SESSION,
    MODEL_ACCOUNT,
    MODEL_VERIFICATION,
    MODEL_TWO_FACTOR,
];

/// The provider id under which an email-and-password account is stored.
pub const PROVIDER_CREDENTIAL: &str = "credential";

/// Declared columns, in table order. The store refuses any other field name,
/// which is what keeps generated SQL from being an injection surface.
pub fn columns(model: &str) -> Option<&'static [&'static str]> {
    let columns: &'static [&'static str] = match model {
        MODEL_USER => &[
            "id",
            "name",
            "email",
            "emailVerified",
            "image",
            "username",
            "displayUsername",
            "isAnonymous",
            "twoFactorEnabled",
            "role",
            "banned",
            "banReason",
            "banExpires",
            "createdAt",
            "updatedAt",
        ],
        MODEL_SESSION => &[
            "id",
            "userId",
            "token",
            "expiresAt",
            "ipAddress",
            "userAgent",
            "impersonatedBy",
            "createdAt",
            "updatedAt",
        ],
        MODEL_ACCOUNT => &[
            "id",
            "userId",
            "accountId",
            "providerId",
            "accessToken",
            "refreshToken",
            "idToken",
            "accessTokenExpiresAt",
            "refreshTokenExpiresAt",
            "scope",
            "password",
            "createdAt",
            "updatedAt",
        ],
        MODEL_VERIFICATION => &[
            "id",
            "identifier",
            "value",
            "expiresAt",
            "createdAt",
            "updatedAt",
        ],
        MODEL_TWO_FACTOR => &["id", "userId", "secret", "backupCodes"],
        _ => return None,
    };
    Some(columns)
}

/// Columns declared BOOLEAN, which SQL stores as 0/1 and reads back as
/// integers unless put right.
pub fn boolean_columns(model: &str) -> &'static [&'static str] {
    match model {
        MODEL_USER => &["emailVerified", "isAnonymous", "twoFactorEnabled", "banned"],
        _ => &[],
    }
}

/// The column where fields the table does not declare are kept, as JSON.
pub const EXTRA_COLUMN: &str = "data";

/// Values a record gets when a caller leaves the field out — the same ones
/// the table definitions declare as `DEFAULT`, applied in Rust so a store
/// without SQL defaults behaves identically.
pub fn defaults(model: &str) -> Vec<(&'static str, Value)> {
    if model != MODEL_USER {
        return Vec::new();
    }
    vec![
        ("role", Value::str("user")),
        ("emailVerified", Value::Bool(false)),
        ("isAnonymous", Value::Bool(false)),
        ("twoFactorEnabled", Value::Bool(false)),
        ("banned", Value::Bool(false)),
    ]
}

/// The table definitions. Quoted identifiers and `TEXT`/`BOOLEAN` only, so
/// the same statements run on SQLite, MySQL and Postgres. The extra `data`
/// column holds whatever a caller adds beyond the declared fields.
pub fn schema_sql() -> String {
    let mut sql = String::new();
    for model in MODELS {
        sql.push_str(&format!("CREATE TABLE IF NOT EXISTS \"{model}\" (\n"));
        let mut lines: Vec<String> = Vec::new();
        for col in columns(model).expect("declared model") {
            let line = match (model, *col) {
                (_, "id") => format!("  \"{col}\" TEXT PRIMARY KEY"),
                (MODEL_USER, "email") => format!("  \"{col}\" TEXT NOT NULL UNIQUE"),
                (MODEL_USER, "username") => format!("  \"{col}\" TEXT UNIQUE"),
                (MODEL_USER, "name") => format!("  \"{col}\" TEXT NOT NULL"),
                (MODEL_SESSION, "token") => format!("  \"{col}\" TEXT NOT NULL UNIQUE"),
                (MODEL_SESSION, "userId") | (MODEL_ACCOUNT, "userId") => {
                    format!("  \"{col}\" TEXT NOT NULL")
                }
                (MODEL_TWO_FACTOR, "userId") => format!("  \"{col}\" TEXT NOT NULL UNIQUE"),
                (MODEL_USER, "role") => "  \"role\" TEXT NOT NULL DEFAULT 'user'".to_string(),
                _ if boolean_columns(model).contains(col) => {
                    format!("  \"{col}\" BOOLEAN NOT NULL DEFAULT FALSE")
                }
                (_, "createdAt") | (_, "updatedAt") => format!("  \"{col}\" TEXT NOT NULL"),
                (MODEL_VERIFICATION, "identifier" | "value" | "expiresAt")
                | (MODEL_ACCOUNT, "accountId" | "providerId")
                | (MODEL_TWO_FACTOR, "secret" | "backupCodes") => {
                    format!("  \"{col}\" TEXT NOT NULL")
                }
                _ => format!("  \"{col}\" TEXT"),
            };
            lines.push(line);
        }
        lines.push(format!("  \"{EXTRA_COLUMN}\" TEXT NOT NULL DEFAULT '{{}}'"));
        sql.push_str(&lines.join(",\n"));
        sql.push_str("\n);\n\n");
    }
    sql.push_str(
        "CREATE INDEX IF NOT EXISTS \"session_userId\" ON \"session\" (\"userId\");\n\
         CREATE INDEX IF NOT EXISTS \"account_userId\" ON \"account\" (\"userId\");\n\
         CREATE INDEX IF NOT EXISTS \"account_provider\" ON \"account\" (\"providerId\", \"accountId\");\n\
         CREATE INDEX IF NOT EXISTS \"verification_identifier\" ON \"verification\" (\"identifier\");\n",
    );
    sql
}

/// A model as Rust sees it: convertible both ways, with the model's name.
pub trait Record: Sized {
    const MODEL: &'static str;
    fn from_record(record: &RecordMap) -> AuthResult<Self>;
    fn to_record(&self) -> RecordMap;
}

fn req_str(r: &RecordMap, model: &str, field: &str) -> AuthResult<String> {
    get_str(r, field).ok_or_else(|| {
        AuthError::internal(format!("{model}.{field} is missing or of the wrong type"))
    })
}

// ------------------------------------------------------------------- user ---

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub email_verified: bool,
    pub image: Option<String>,
    pub username: Option<String>,
    pub display_username: Option<String>,
    pub is_anonymous: bool,
    pub two_factor_enabled: bool,
    pub role: String,
    pub banned: bool,
    pub ban_reason: Option<String>,
    pub ban_expires: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extra: RecordMap,
}

impl Record for User {
    const MODEL: &'static str = MODEL_USER;

    fn from_record(r: &RecordMap) -> AuthResult<Self> {
        Ok(User {
            id: req_str(r, "user", "id")?,
            name: req_str(r, "user", "name")?,
            email: req_str(r, "user", "email")?,
            email_verified: get_bool(r, "emailVerified").unwrap_or(false),
            image: get_str(r, "image"),
            username: get_str(r, "username"),
            display_username: get_str(r, "displayUsername"),
            is_anonymous: get_bool(r, "isAnonymous").unwrap_or(false),
            two_factor_enabled: get_bool(r, "twoFactorEnabled").unwrap_or(false),
            role: get_str(r, "role").unwrap_or_else(|| "user".to_string()),
            banned: get_bool(r, "banned").unwrap_or(false),
            ban_reason: get_str(r, "banReason"),
            ban_expires: get_str(r, "banExpires"),
            created_at: get_str(r, "createdAt").unwrap_or_default(),
            updated_at: get_str(r, "updatedAt").unwrap_or_default(),
            extra: extras(r, Self::MODEL),
        })
    }

    fn to_record(&self) -> RecordMap {
        let mut r = object! {
            "id" => s(self.id.clone()),
            "name" => s(self.name.clone()),
            "email" => s(self.email.clone()),
            "emailVerified" => b(self.email_verified),
            "image" => opt_s(&self.image),
            "username" => opt_s(&self.username),
            "displayUsername" => opt_s(&self.display_username),
            "isAnonymous" => b(self.is_anonymous),
            "twoFactorEnabled" => b(self.two_factor_enabled),
            "role" => s(self.role.clone()),
            "banned" => b(self.banned),
            "banReason" => opt_s(&self.ban_reason),
            "banExpires" => opt_s(&self.ban_expires),
            "createdAt" => s(self.created_at.clone()),
            "updatedAt" => s(self.updated_at.clone()),
        };
        for (k, v) in self.extra.iter() {
            r.set(k.clone(), v.clone());
        }
        r
    }
}

impl User {
    /// The user as clients receive it. Nothing secret lives on a user row —
    /// passwords are on the credential account — so this is the whole record.
    pub fn to_json(&self) -> Value {
        Value::Object(self.to_record())
    }

    /// A ban that has run out is not a ban. Returns the reason if banned now.
    pub fn ban_in_force(&self, now: &str) -> Option<&str> {
        if !self.banned {
            return None;
        }
        match &self.ban_expires {
            Some(expires) if expires.as_str() <= now => None,
            _ => Some(self.ban_reason.as_deref().unwrap_or("banned")),
        }
    }
}

// ---------------------------------------------------------------- session ---

#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub id: String,
    pub user_id: String,
    pub token: String,
    pub expires_at: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub impersonated_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extra: RecordMap,
}

impl Record for Session {
    const MODEL: &'static str = MODEL_SESSION;

    fn from_record(r: &RecordMap) -> AuthResult<Self> {
        Ok(Session {
            id: req_str(r, "session", "id")?,
            user_id: req_str(r, "session", "userId")?,
            token: req_str(r, "session", "token")?,
            expires_at: get_str(r, "expiresAt").unwrap_or_default(),
            ip_address: get_str(r, "ipAddress"),
            user_agent: get_str(r, "userAgent"),
            impersonated_by: get_str(r, "impersonatedBy"),
            created_at: get_str(r, "createdAt").unwrap_or_default(),
            updated_at: get_str(r, "updatedAt").unwrap_or_default(),
            extra: extras(r, Self::MODEL),
        })
    }

    fn to_record(&self) -> RecordMap {
        let mut r = object! {
            "id" => s(self.id.clone()),
            "userId" => s(self.user_id.clone()),
            "token" => s(self.token.clone()),
            "expiresAt" => s(self.expires_at.clone()),
            "ipAddress" => opt_s(&self.ip_address),
            "userAgent" => opt_s(&self.user_agent),
            "impersonatedBy" => opt_s(&self.impersonated_by),
            "createdAt" => s(self.created_at.clone()),
            "updatedAt" => s(self.updated_at.clone()),
        };
        for (k, v) in self.extra.iter() {
            r.set(k.clone(), v.clone());
        }
        r
    }
}

impl Session {
    /// The session as `/get-session` returns it: no token hash.
    pub fn to_json(&self) -> Value {
        let r = self.to_record();
        let mut out = Object::new();
        for key in [
            "id",
            "userId",
            "expiresAt",
            "ipAddress",
            "userAgent",
            "impersonatedBy",
            "createdAt",
            "updatedAt",
        ] {
            if let Some(v) = r.get(key) {
                out.set(key, v.clone());
            }
        }
        Value::Object(out)
    }
}

// ---------------------------------------------------------------- account ---

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: String,
    pub user_id: String,
    pub account_id: String,
    pub provider_id: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub access_token_expires_at: Option<String>,
    pub refresh_token_expires_at: Option<String>,
    pub scope: Option<String>,
    pub password: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub extra: RecordMap,
}

impl Record for Account {
    const MODEL: &'static str = MODEL_ACCOUNT;

    fn from_record(r: &RecordMap) -> AuthResult<Self> {
        Ok(Account {
            id: req_str(r, "account", "id")?,
            user_id: req_str(r, "account", "userId")?,
            account_id: get_str(r, "accountId").unwrap_or_default(),
            provider_id: get_str(r, "providerId").unwrap_or_default(),
            access_token: get_str(r, "accessToken"),
            refresh_token: get_str(r, "refreshToken"),
            id_token: get_str(r, "idToken"),
            access_token_expires_at: get_str(r, "accessTokenExpiresAt"),
            refresh_token_expires_at: get_str(r, "refreshTokenExpiresAt"),
            scope: get_str(r, "scope"),
            password: get_str(r, "password"),
            created_at: get_str(r, "createdAt").unwrap_or_default(),
            updated_at: get_str(r, "updatedAt").unwrap_or_default(),
            extra: extras(r, Self::MODEL),
        })
    }

    fn to_record(&self) -> RecordMap {
        let mut r = object! {
            "id" => s(self.id.clone()),
            "userId" => s(self.user_id.clone()),
            "accountId" => s(self.account_id.clone()),
            "providerId" => s(self.provider_id.clone()),
            "accessToken" => opt_s(&self.access_token),
            "refreshToken" => opt_s(&self.refresh_token),
            "idToken" => opt_s(&self.id_token),
            "accessTokenExpiresAt" => opt_s(&self.access_token_expires_at),
            "refreshTokenExpiresAt" => opt_s(&self.refresh_token_expires_at),
            "scope" => opt_s(&self.scope),
            "password" => opt_s(&self.password),
            "createdAt" => s(self.created_at.clone()),
            "updatedAt" => s(self.updated_at.clone()),
        };
        for (k, v) in self.extra.iter() {
            r.set(k.clone(), v.clone());
        }
        r
    }
}

// ----------------------------------------------------------- verification ---

#[derive(Clone, Debug, PartialEq)]
pub struct Verification {
    pub id: String,
    pub identifier: String,
    pub value: String,
    pub expires_at: String,
    pub created_at: String,
    pub updated_at: String,
    pub extra: RecordMap,
}

impl Record for Verification {
    const MODEL: &'static str = MODEL_VERIFICATION;

    fn from_record(r: &RecordMap) -> AuthResult<Self> {
        Ok(Verification {
            id: req_str(r, "verification", "id")?,
            identifier: get_str(r, "identifier").unwrap_or_default(),
            value: get_str(r, "value").unwrap_or_default(),
            expires_at: get_str(r, "expiresAt").unwrap_or_default(),
            created_at: get_str(r, "createdAt").unwrap_or_default(),
            updated_at: get_str(r, "updatedAt").unwrap_or_default(),
            extra: extras(r, Self::MODEL),
        })
    }

    fn to_record(&self) -> RecordMap {
        let mut r = object! {
            "id" => s(self.id.clone()),
            "identifier" => s(self.identifier.clone()),
            "value" => s(self.value.clone()),
            "expiresAt" => s(self.expires_at.clone()),
            "createdAt" => s(self.created_at.clone()),
            "updatedAt" => s(self.updated_at.clone()),
        };
        for (k, v) in self.extra.iter() {
            r.set(k.clone(), v.clone());
        }
        r
    }
}

// ------------------------------------------------------------- two factor ---

#[derive(Clone, Debug, PartialEq)]
pub struct TwoFactor {
    pub id: String,
    pub user_id: String,
    /// `v1.<base64url(nonce ‖ ciphertext ‖ tag)>`, AES-256-GCM.
    pub secret: String,
    /// A JSON array of SHA-256 hex digests, one per unused backup code.
    pub backup_codes: String,
    pub extra: RecordMap,
}

impl Record for TwoFactor {
    const MODEL: &'static str = MODEL_TWO_FACTOR;

    fn from_record(r: &RecordMap) -> AuthResult<Self> {
        Ok(TwoFactor {
            id: req_str(r, "twoFactor", "id")?,
            user_id: req_str(r, "twoFactor", "userId")?,
            secret: get_str(r, "secret").unwrap_or_default(),
            backup_codes: get_str(r, "backupCodes").unwrap_or_else(|| "[]".to_string()),
            extra: extras(r, Self::MODEL),
        })
    }

    fn to_record(&self) -> RecordMap {
        let mut r = object! {
            "id" => s(self.id.clone()),
            "userId" => s(self.user_id.clone()),
            "secret" => s(self.secret.clone()),
            "backupCodes" => s(self.backup_codes.clone()),
        };
        for (k, v) in self.extra.iter() {
            r.set(k.clone(), v.clone());
        }
        r
    }
}

/// Fields a record carries beyond its model's declared columns.
fn extras(record: &RecordMap, model: &str) -> RecordMap {
    let declared = columns(model).expect("declared model");
    let mut extra = RecordMap::new();
    for (k, v) in record.iter() {
        if !declared.contains(&k.as_str()) && k != EXTRA_COLUMN {
            extra.set(k.clone(), v.clone());
        }
    }
    extra
}

// -------------------------------------------------------------- validation ---

pub const MIN_PASSWORD_LEN: usize = 8;
pub const MAX_PASSWORD_LEN: usize = 128;

/// Lower-cased and trimmed, if it looks like an address.
pub fn check_email(email: &str) -> AuthResult<String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email.is_ascii()
        && email.matches('@').count() == 1
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !domain.contains("..")
        });
    if valid {
        Ok(email)
    } else {
        Err(AuthError::bad_request(
            "INVALID_EMAIL",
            "not a valid email address",
        ))
    }
}

pub fn check_password(password: &str) -> AuthResult<()> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AuthError::bad_request(
            "PASSWORD_TOO_SHORT",
            format!("password must be at least {MIN_PASSWORD_LEN} characters"),
        ));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err(AuthError::bad_request(
            "PASSWORD_TOO_LONG",
            format!("password must be at most {MAX_PASSWORD_LEN} characters"),
        ));
    }
    Ok(())
}

/// Usernames: letters, digits, `-`, `_`, `.`, 3–32 characters. Compared
/// case-insensitively everywhere, so `Ada` and `ada` are one username.
pub fn check_username(username: &str) -> AuthResult<String> {
    let username = username.trim().to_string();
    let ok = (3..=32).contains(&username.len())
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !username.starts_with(['-', '_', '.'])
        && !username.ends_with(['-', '_', '.']);
    if ok {
        Ok(username)
    } else {
        Err(AuthError::bad_request(
            "INVALID_USERNAME",
            "username must be 3-32 letters, digits, '-', '_' or '.'",
        ))
    }
}

pub fn now_iso() -> String {
    nanobase::platform::iso8601(nanobase::platform::now_secs())
}

pub fn iso(secs: i64) -> String {
    nanobase::platform::iso8601(secs)
}

pub fn now_secs() -> i64 {
    nanobase::platform::now_secs()
}

pub fn new_id() -> String {
    // UUID-shaped, so clients that validate the format accept it.
    nanobase::crypto::random_id()
        .trim_start_matches('u')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_validation() {
        assert_eq!(check_email(" Ada@Example.COM ").unwrap(), "ada@example.com");
        assert!(check_email("no-at-sign").is_err());
        assert!(check_email("a@b").is_err());
        assert!(check_email("a b@c.d").is_err());
        assert!(check_email("a@@b.d").is_err());
        assert!(check_email("@b.d").is_err());
    }

    #[test]
    fn password_length() {
        assert!(check_password("short").is_err());
        assert!(check_password("correct horse").is_ok());
        assert!(check_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn username_rules() {
        assert!(check_username("ada.l").is_ok());
        assert!(check_username("ab").is_err());
        assert!(check_username("-ada").is_err());
        assert!(check_username("ada bye").is_err());
    }

    #[test]
    fn schema_declares_every_model_with_an_extra_column() {
        let sql = schema_sql();
        for model in MODELS {
            assert!(
                sql.contains(&format!("CREATE TABLE IF NOT EXISTS \"{model}\"")),
                "{model}"
            );
        }
        assert_eq!(sql.matches(EXTRA_COLUMN).count(), MODELS.len());
    }

    #[test]
    fn dot_paths() {
        let v = mosql::parse_json(r#"{"data":[{"user":{"id":7}}],"a":{"b":"x"}}"#).unwrap();
        assert_eq!(lookup(&v, "data.0.user.id"), Some(&Value::Int(7)));
        assert_eq!(lookup(&v, "a.b"), Some(&Value::str("x")));
        assert_eq!(lookup(&v, "a.z"), None);
        assert_eq!(lookup(&v, "data.5.user.id"), None);
    }

    #[test]
    fn user_round_trips_with_extras() {
        let mut u = User {
            id: "u1".into(),
            name: "Ada".into(),
            email: "ada@example.com".into(),
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
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            extra: RecordMap::new(),
        };
        u.extra.set("plan", Value::str("pro"));
        let back = User::from_record(&u.to_record()).unwrap();
        assert_eq!(back, u);
        assert_eq!(back.extra.get("plan"), Some(&Value::str("pro")));
    }

    #[test]
    fn an_expired_ban_is_not_in_force() {
        let mut u = User {
            id: "u".into(),
            name: "n".into(),
            email: "e@x.co".into(),
            email_verified: false,
            image: None,
            username: None,
            display_username: None,
            is_anonymous: false,
            two_factor_enabled: false,
            role: "user".into(),
            banned: true,
            ban_reason: Some("spam".into()),
            ban_expires: Some("2026-01-02T00:00:00Z".into()),
            created_at: String::new(),
            updated_at: String::new(),
            extra: RecordMap::new(),
        };
        assert_eq!(u.ban_in_force("2026-01-01T00:00:00Z"), Some("spam"));
        assert_eq!(u.ban_in_force("2026-01-03T00:00:00Z"), None);
        u.ban_expires = None;
        assert_eq!(u.ban_in_force("2027-01-01T00:00:00Z"), Some("spam"));
    }
}
