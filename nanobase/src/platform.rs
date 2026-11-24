//! The platform's own data: who may sign in, and which apps exist.
//!
//! This is the one database that is not an app's — it holds the owner's
//! account and the list of apps, each of which points at a database of its own.
//! It is a single JSON file today, because `nanobase` is built where no SQLite
//! driver can be fetched and because a few kilobytes of platform data does not
//! need one. [`PlatformStore`] is the seam: the file is an implementation, and
//! a SQLite or Postgres backend can replace it without touching a caller.
//!
//! Two things this file has to get right, because they are the whole of V1's
//! security:
//!
//! * **The password is never stored.** What is kept is a salted PBKDF2 hash
//!   with a per-password random salt, so a leaked platform file does not leak
//!   passwords, and two users with the same password do not hash alike.
//! * **Signing in takes the same time whether the email exists or not.** An
//!   unknown email still runs a hash, or the response time would reveal which
//!   addresses are real.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use mosql::{to_json, Object, Value};
use uflite::error::{Error, Result};

use crate::config::AppDoc;
use crate::crypto::{b64url_decode, b64url_encode, ct_eq, pbkdf2_sha256, random_bytes, random_id};
use crate::jwt::Claims;

/// PBKDF2 rounds. OWASP's 2023 recommendation for PBKDF2-HMAC-SHA256 is
/// 600,000; this is the one place where slow is the feature.
pub const PBKDF2_ITERATIONS: u32 = 600_000;

/// Shortest password accepted. Longer is better; this only rules out the
/// obviously broken.
pub const MIN_PASSWORD_LEN: usize = 8;

/// The only role V1 has: the one account that can do everything.
pub const ROLE_OWNER: &str = "owner";

/// Default session lifetime. A day is long enough to be useful and short
/// enough that a stolen token expires on its own.
pub const SESSION_TTL_SECS: i64 = 24 * 60 * 60;

/// A future the store returns. Boxed so the trait can be used as `dyn
/// PlatformStore`, which the HTTP layer needs to hold several backends.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

// ---------------------------------------------------------------- password ---

/// A salted PBKDF2 hash of a password. Serialises to a small JSON object so
/// the parameters travel with the hash and can be raised later without
/// invalidating stored passwords.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordHash {
    pub iterations: u32,
    pub salt: Vec<u8>,
    pub hash: Vec<u8>,
}

impl PasswordHash {
    /// Hash with the production iteration count.
    pub fn hash(password: &str) -> Self {
        Self::hash_with(password, PBKDF2_ITERATIONS)
    }

    /// Hash with an explicit iteration count — for tests, which would
    /// otherwise spend minutes on 600,000 rounds.
    pub fn hash_with(password: &str, iterations: u32) -> Self {
        let salt = random_bytes(16);
        let hash = pbkdf2_sha256(password.as_bytes(), &salt, iterations, 32);
        PasswordHash {
            iterations,
            salt,
            hash,
        }
    }

    /// Whether `password` is the one this hash came from.
    pub fn verify(&self, password: &str) -> bool {
        let candidate = pbkdf2_sha256(password.as_bytes(), &self.salt, self.iterations, 32);
        ct_eq(&candidate, &self.hash)
    }

    fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("iterations", Value::Int(i64::from(self.iterations)));
        o.set("salt", Value::str(b64url_encode(&self.salt)));
        o.set("hash", Value::str(b64url_encode(&self.hash)));
        Value::Object(o)
    }

    fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(PasswordHash {
            iterations: match o.get("iterations")? {
                Value::Int(i) => u32::try_from(*i).ok()?,
                _ => return None,
            },
            salt: b64url_decode(o.get("salt")?.as_str()?)?,
            hash: b64url_decode(o.get("hash")?.as_str()?)?,
        })
    }
}

// ------------------------------------------------------------------ user ---

/// An account on the platform. V1 has exactly one, the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password: PasswordHash,
    /// ISO-8601, e.g. `2026-11-23T10:00:00Z`.
    pub created_at: String,
}

impl User {
    fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("id", Value::str(self.id.clone()));
        o.set("email", Value::str(self.email.clone()));
        o.set("password", self.password.to_value());
        o.set("createdAt", Value::str(self.created_at.clone()));
        Value::Object(o)
    }

    fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(User {
            id: o.get("id")?.as_str()?.to_string(),
            email: o.get("email")?.as_str()?.to_string(),
            password: PasswordHash::from_value(o.get("password")?)?,
            created_at: o.get("createdAt")?.as_str()?.to_string(),
        })
    }
}

/// An app the platform knows about. The document is stored whole, so the
/// platform file is self-sufficient and an app's config is not reassembled
/// from somewhere else on startup.
#[derive(Debug, Clone, PartialEq)]
pub struct AppRecord {
    pub id: String,
    pub created_at: String,
    pub doc: AppDoc,
}

impl AppRecord {
    fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("id", Value::str(self.id.clone()));
        o.set("createdAt", Value::str(self.created_at.clone()));
        o.set("doc", self.doc.to_value());
        Value::Object(o)
    }

    fn from_value(v: &Value) -> Option<Self> {
        let o = v.as_object()?;
        Some(AppRecord {
            id: o.get("id")?.as_str()?.to_string(),
            created_at: o.get("createdAt")?.as_str()?.to_string(),
            doc: AppDoc::from_value(o.get("doc")?).ok()?,
        })
    }
}

// --------------------------------------------------------------- platform ---

/// Everything the platform persists.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Platform {
    /// The key session tokens are signed with. Generated on first setup and
    /// then stored, so tokens survive a restart. Anyone who can read the
    /// platform file can mint tokens — which is another way of saying the
    /// platform file is the platform's database and must be protected like one.
    pub jwt_secret: String,
    pub users: Vec<User>,
    pub apps: Vec<AppRecord>,
}

impl Platform {
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("version", Value::Int(1));
        o.set("jwtSecret", Value::str(self.jwt_secret.clone()));
        o.set(
            "users",
            Value::Array(self.users.iter().map(User::to_value).collect()),
        );
        o.set(
            "apps",
            Value::Array(self.apps.iter().map(AppRecord::to_value).collect()),
        );
        Value::Object(o)
    }

    pub fn from_value(v: &Value) -> Result<Self> {
        let o = v
            .as_object()
            .ok_or_else(|| Error::internal("platform file is not a JSON object"))?;
        let jwt_secret = o
            .get("jwtSecret")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let mut users = Vec::new();
        if let Some(Value::Array(list)) = o.get("users") {
            for item in list {
                users.push(User::from_value(item).ok_or_else(|| {
                    Error::internal("platform file has a user that cannot be read")
                })?);
            }
        }
        let mut apps = Vec::new();
        if let Some(Value::Array(list)) = o.get("apps") {
            for item in list {
                apps.push(AppRecord::from_value(item).ok_or_else(|| {
                    Error::internal("platform file has an app that cannot be read")
                })?);
            }
        }
        Ok(Platform {
            jwt_secret,
            users,
            apps,
        })
    }

    pub fn to_json(&self) -> String {
        to_json(&self.to_value())
    }

    /// The owner, if one has been created. V1 has at most one user, so this is
    /// simply the first.
    pub fn owner(&self) -> Option<&User> {
        self.users.first()
    }

    /// Make sure a signing key exists, generating one if it does not.
    ///
    /// Returns whether a new key was created, so a caller can tell a first run
    /// from a reload.
    pub fn ensure_jwt_secret(&mut self) -> bool {
        if !self.jwt_secret.is_empty() {
            return false;
        }
        self.jwt_secret = b64url_encode(&random_bytes(32));
        true
    }

    /// The bytes session tokens are signed with.
    pub fn jwt_key(&self) -> &[u8] {
        self.jwt_secret.as_bytes()
    }

    /// Whether first-run setup still has to happen.
    pub fn needs_setup(&self) -> bool {
        self.users.is_empty()
    }

    /// An app by id.
    pub fn app(&self, id: &str) -> Option<&AppRecord> {
        self.apps.iter().find(|a| a.id == id)
    }
}

// ------------------------------------------------------------------ store ---

/// Where the platform keeps its state.
pub trait PlatformStore: Send + Sync {
    fn load(&self) -> BoxFuture<'_, Result<Platform>>;
    fn save(&self, platform: &Platform) -> BoxFuture<'_, Result<()>>;
}

/// A single JSON file. The default.
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        FileStore { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl PlatformStore for FileStore {
    fn load(&self) -> BoxFuture<'_, Result<Platform>> {
        Box::pin(async move {
            let text = match std::fs::read_to_string(&self.path) {
                Ok(t) => t,
                // A missing file is a platform that has not been set up yet,
                // not an error.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Platform::default())
                }
                Err(e) => {
                    return Err(Error::internal(format!(
                        "cannot read {}: {e}",
                        self.path.display()
                    )))
                }
            };
            let v = mosql::parse_json(&text).map_err(|e| {
                Error::internal(format!("{} is not valid JSON: {e}", self.path.display()))
            })?;
            Platform::from_value(&v)
        })
    }

    fn save(&self, platform: &Platform) -> BoxFuture<'_, Result<()>> {
        // The platform is small, so the future owns a copy: it then borrows
        // only `self`, which is what `'_` in the signature promises.
        let platform = platform.clone();
        Box::pin(async move {
            let body = platform.to_json();
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| {
                    Error::internal(format!("cannot create {}: {e}", dir.display()))
                })?;
            }
            // Write beside the target and rename, so a crash mid-write leaves
            // the previous file intact rather than a truncated one.
            let tmp = self.path.with_extension("json.tmp");
            std::fs::write(&tmp, body)
                .map_err(|e| Error::internal(format!("cannot write {}: {e}", tmp.display())))?;
            std::fs::rename(&tmp, &self.path).map_err(|e| {
                Error::internal(format!("cannot replace {}: {e}", self.path.display()))
            })?;
            Ok(())
        })
    }
}

/// An in-memory store, for tests and for a platform that keeps state
/// elsewhere.
#[derive(Debug, Default)]
pub struct MemoryStore {
    state: std::sync::Mutex<Platform>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl PlatformStore for MemoryStore {
    fn load(&self) -> BoxFuture<'_, Result<Platform>> {
        Box::pin(async move {
            self.state
                .lock()
                .map(|g| g.clone())
                .map_err(|_| Error::internal("platform store lock poisoned"))
        })
    }

    fn save(&self, platform: &Platform) -> BoxFuture<'_, Result<()>> {
        let platform = platform.clone();
        Box::pin(async move {
            self.state
                .lock()
                .map(|mut g| *g = platform)
                .map_err(|_| Error::internal("platform store lock poisoned"))
        })
    }
}

// ------------------------------------------------------------- onboarding ---

/// The email is lowercased before it is stored, so `Ada@x.com` and
/// `ada@x.com` are one account rather than two.
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// A minimal email check: something, an `@`, a dot in the domain. It is not
/// RFC 5322, and it is not trying to be — it exists to catch a typo, not to
/// decide whether mail can be delivered.
pub fn check_email(email: &str) -> Result<String> {
    let normalized = normalize_email(email);
    let valid = !normalized.is_empty()
        && normalized.len() <= 254
        && normalized
            .chars()
            .all(|c| c.is_ascii() && !c.is_ascii_control() && !c.is_whitespace())
        && match normalized.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty()
                    && !domain.is_empty()
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
                    && domain.contains('.')
            }
            None => false,
        };
    if valid {
        Ok(normalized)
    } else {
        Err(Error::unprocessable(format!(
            "{email:?} is not a valid email address"
        )))
    }
}

/// Password rules. Deliberately short and explicit, so the reason is in the
/// message rather than in a validator someone has to read.
pub fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(Error::unprocessable(format!(
            "password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.len() > 1024 {
        return Err(Error::unprocessable("password is too long"));
    }
    Ok(())
}

/// Create the owner account with the production iteration count.
pub fn create_owner(
    platform: &mut Platform,
    email: &str,
    password: &str,
    now: i64,
) -> Result<User> {
    create_owner_with(platform, email, password, now, PBKDF2_ITERATIONS)
}

/// [`create_owner`](create_owner) with an explicit iteration count.
///
/// Only tests should pass anything but [`PBKDF2_ITERATIONS`]: a lower count
/// makes a stored password cheaper to guess.
pub fn create_owner_with(
    platform: &mut Platform,
    email: &str,
    password: &str,
    now: i64,
    iterations: u32,
) -> Result<User> {
    if !platform.needs_setup() {
        return Err(Error::conflict(
            "this platform already has an owner; sign in instead",
        ));
    }
    let email = check_email(email)?;
    check_password(password)?;
    let user = User {
        id: random_id(),
        email,
        password: PasswordHash::hash_with(password, iterations),
        created_at: iso8601(now),
    };
    platform.users.push(user.clone());
    Ok(user)
}

/// Check an email and password.
///
/// The hash is computed even when the email is unknown, so the time taken does
/// not say whether the account exists.
pub fn verify_login<'a>(platform: &'a Platform, email: &str, password: &str) -> Result<&'a User> {
    let normalized = normalize_email(email);
    match platform.users.iter().find(|u| u.email == normalized) {
        Some(user) if user.password.verify(password) => Ok(user),
        Some(_) => Err(Error::unauthorized("email or password is incorrect")),
        None => {
            // Burn the same work a real check would. Skipping it would make an
            // unknown email answer measurably faster, which is enough to tell
            // which addresses have accounts. With no accounts at all there is
            // nothing to distinguish, so nothing needs burning.
            let iterations = platform
                .users
                .first()
                .map(|u| u.password.iterations)
                .unwrap_or(1);
            black_box(PasswordHash::hash_with(password, iterations));
            Err(Error::unauthorized("email or password is incorrect"))
        }
    }
}

/// A session token for `user`.
pub fn issue_token(user: &User, secret: &[u8], now: i64, ttl_secs: i64) -> String {
    let claims = Claims::new(&user.id, &user.email, ROLE_OWNER, now, ttl_secs);
    crate::jwt::encode(&claims, secret)
}

// ------------------------------------------------------------------- time ---

/// Keep a value alive so the compiler cannot delete the work that produced it.
fn black_box<T>(value: T) -> T {
    std::hint::black_box(value)
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `1970-01-01T00:00:00Z` for an epoch-second count.
///
/// Howard Hinnant's civil-from-days algorithm: correct for every date in the
/// proleptic Gregorian calendar, without a table of month lengths.
pub fn iso8601(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (hour, minute, second) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

// ------------------------------------------------------------------- apps ---

/// A new app id: `app_` and 24 hex characters.
fn new_app_id() -> String {
    let bytes = random_bytes(12);
    let mut s = String::with_capacity(28);
    s.push_str("app_");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

impl Platform {
    /// The index of an app, by id first and then by name.
    ///
    /// Ids are authoritative: a name can change, and a name is something a
    /// person types, so it is only a fallback.
    fn find_app_index(&self, key: &str) -> Option<usize> {
        if let Some(index) = self.apps.iter().position(|a| a.id == key) {
            return Some(index);
        }
        let wanted = key.trim().to_ascii_lowercase();
        self.apps
            .iter()
            .position(|a| a.doc.app.name.trim().to_ascii_lowercase() == wanted)
    }

    /// An app by id, or by name when no id matches.
    pub fn find_app(&self, key: &str) -> Option<&AppRecord> {
        self.find_app_index(key).map(|i| &self.apps[i])
    }

    /// An app by name, case-insensitively, so `Todos` and `todos` are one app.
    pub fn app_by_name(&self, name: &str) -> Option<&AppRecord> {
        let wanted = name.trim().to_ascii_lowercase();
        self.apps
            .iter()
            .find(|a| a.doc.app.name.trim().to_ascii_lowercase() == wanted)
    }

    /// Create an app from a document.
    ///
    /// The document is checked again here even though a caller usually reads it
    /// through [`AppDoc::load`](crate::config::AppDoc::load): a document built
    /// in code, or handed over by a future API route, has to pass the same gate
    /// as one read from a file.
    pub fn create_app(&mut self, doc: AppDoc, now: i64) -> Result<&AppRecord> {
        doc.validate()?;
        if let Some(problem) = crate::config::check_app_name(&doc.app.name) {
            return Err(Error::unprocessable(format!("app name {problem}")));
        }
        let name = doc.app.name.trim().to_string();
        if let Some(existing) = self.app_by_name(&name) {
            return Err(Error::conflict(format!(
                "an app named {:?} already exists ({})",
                existing.doc.app.name, existing.id
            )));
        }
        self.apps.push(AppRecord {
            id: new_app_id(),
            created_at: iso8601(now),
            doc,
        });
        Ok(self.apps.last().expect("just pushed an app"))
    }

    /// Rename an app. The id does not change, so anything referring to the app
    /// — a token, a config, a URL path — keeps working.
    pub fn rename_app(&mut self, key: &str, new_name: &str) -> Result<&AppRecord> {
        let index = self
            .find_app_index(key)
            .ok_or_else(|| Error::not_found(format!("no app for {key:?}")))?;
        if let Some(problem) = crate::config::check_app_name(new_name) {
            return Err(Error::unprocessable(format!("app name {problem}")));
        }
        let new_name = new_name.trim().to_string();
        if let Some(other) = self.app_by_name(&new_name) {
            if other.id != self.apps[index].id {
                return Err(Error::conflict(format!(
                    "an app named {new_name:?} already exists ({})",
                    other.id
                )));
            }
        }
        self.apps[index].doc.app.name = new_name;
        Ok(&self.apps[index])
    }

    /// Copy an app's configuration and schema to a new app, without its rows.
    ///
    /// Data is deliberately not copied: a clone is for trying a schema change
    /// or standing up a staging copy of the shape, and duplicating a live
    /// table is the surprise, not the expectation.
    pub fn clone_app(&mut self, key: &str, new_name: &str, now: i64) -> Result<&AppRecord> {
        let source = self
            .find_app(key)
            .ok_or_else(|| Error::not_found(format!("no app for {key:?}")))?;
        let mut doc = source.doc.clone();
        doc.app.name = new_name.trim().to_string();
        doc.data.clear();
        doc.meta.includes_data = false;
        self.create_app(doc, now)
    }

    /// Delete an app.
    ///
    /// `confirm` must be the app's id. Deleting an app deletes its data, and
    /// an id is not something a person types by accident — a name is, which is
    /// exactly why a name is not accepted here.
    pub fn remove_app(&mut self, key: &str, confirm: &str) -> Result<AppRecord> {
        let index = self
            .find_app_index(key)
            .ok_or_else(|| Error::not_found(format!("no app for {key:?}")))?;
        let id = self.apps[index].id.clone();
        if confirm != id {
            return Err(Error::bad_request(format!(
                "refusing to delete {id}: pass --confirm {id}"
            )));
        }
        Ok(self.apps.remove(index))
    }

    /// Replace an app's document, keeping its id and creation time.
    ///
    /// The import-over-an-existing-app path: a user with an edited export
    /// wants the app to become that, not to gain a second app.
    pub fn replace_app(&mut self, key: &str, doc: AppDoc) -> Result<&AppRecord> {
        let index = self
            .find_app_index(key)
            .ok_or_else(|| Error::not_found(format!("no app for {key:?}")))?;
        doc.validate()?;
        let name = doc.app.name.trim().to_string();
        if let Some(other) = self.app_by_name(&name) {
            if other.id != self.apps[index].id {
                return Err(Error::conflict(format!(
                    "an app named {name:?} already exists ({})",
                    other.id
                )));
            }
        }
        self.apps[index].doc = doc;
        Ok(&self.apps[index])
    }

    /// App names, sorted case-insensitively, for listing.
    pub fn app_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.apps.iter().map(|a| a.doc.app.name.as_str()).collect();
        names.sort_unstable_by_key(|n| n.to_ascii_lowercase());
        names
    }
}

impl AppRecord {
    /// Where this app's own data lives, under the platform's root.
    ///
    /// Each app gets a directory of its own, which is what "each app gets its
    /// own database" means for the engines that are files rather than servers.
    pub fn data_dir(&self, root: &Path) -> PathBuf {
        root.join("apps").join(&self.id)
    }

    /// The SQLite file for this app, when its database is `sqlite` with no
    /// explicit path.
    pub fn sqlite_path(&self, root: &Path) -> PathBuf {
        self.data_dir(root).join("app.db")
    }
}

impl FileStore {
    /// The directory the platform file sits in — the root that per-app data
    /// directories hang off.
    pub fn root(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}
