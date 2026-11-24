//! An app's configuration, schema and seed data — the unit the CLI exports,
//! imports and stores in the platform database.
//!
//! The shape is the file a user hands to `nanobase import`:
//!
//! ```json
//! {
//!   "meta":   { "nanobaseVersion": "1.0.0", "includesData": true },
//!   "app":    { "name": "Config Case", "type": "remote" },
//!   "config": { "auth": {…}, "database": {…}, "storage": {…},
//!               "email": null, "acl": {…} },
//!   "schema": { "todos": { "columns": { "id": { "type": "integer" } } } },
//!   "data":   { "todos": [ { "title": "Launch" } ] }
//! }
//! ```
//!
//! Parsing and validation are separate steps on purpose: [`AppDoc::from_value`]
//! reads the shape and collects *every* problem it can find, so a user with a
//! broken file is told about all of it at once instead of one error per run.
//! A document that parses is not necessarily valid — [`AppDoc::validate`] is
//! the gate, and it is what import calls.
//!
//! Secrets (the auth secret, OAuth client secrets, storage keys) are part of
//! the shape because a user has to be able to write them down. They are not
//! part of a *shareable* export: [`AppDoc::to_json_redacted`] blanks them.

use std::collections::BTreeMap;
use std::fmt;

use mosql::{to_json, Object, Value};

/// The `meta.nanobaseVersion` this build writes.
pub const NANOBASE_VERSION: &str = "1.0.0";

/// One thing wrong with a document, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Dot-separated path into the document, e.g. `config.storage.local.basePath`.
    pub path: String,
    /// What is wrong, in one sentence.
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

impl Issue {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Issue {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// A document that could not be accepted. Carries at least one [`Issue`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub issues: Vec<Issue>,
}

impl ConfigError {
    pub fn new(issues: Vec<Issue>) -> Self {
        ConfigError { issues }
    }

    /// One issue, for callers that bail on the first problem.
    pub fn one(path: impl Into<String>, message: impl Into<String>) -> Self {
        ConfigError {
            issues: vec![Issue::new(path, message)],
        }
    }

    /// `config` problems are the caller's mistake: 422, not 500.
    pub fn status(&self) -> u16 {
        422
    }

    /// The platform's error envelope, with `errors` keyed by field path so a
    /// form can highlight each one.
    ///
    /// ```json
    /// { "name": "Unprocessable", "message": "2 problems",
    ///   "code": 422, "className": "unprocessable",
    ///   "errors": { "config.storage.type": "unknown storage type \"disk\"" } }
    /// ```
    pub fn to_wire(&self) -> Value {
        let mut errors = Object::new();
        for issue in &self.issues {
            // Later issues on the same path lose; the first is the one to fix.
            if errors.get(&issue.path).is_none() {
                errors.set(issue.path.clone(), Value::str(issue.message.clone()));
            }
        }
        let mut o = Object::new();
        o.set("name", Value::str("Unprocessable"));
        o.set(
            "message",
            Value::str(format!(
                "{} problem{}",
                self.issues.len(),
                plural(self.issues.len())
            )),
        );
        o.set("code", Value::Int(i64::from(self.status())));
        o.set("className", Value::str("unprocessable"));
        o.set("errors", Value::Object(errors));
        Value::Object(o)
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, issue) in self.issues.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{}: {}", issue.path, issue.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// A document problem is the caller's mistake, so it becomes the 422 the
/// platform answers with. Every field path travels in the message, which is
/// what makes the error actionable; [`ConfigError::to_wire`] is the form that
/// also carries them as a map.
impl From<ConfigError> for uflite::error::Error {
    fn from(e: ConfigError) -> Self {
        uflite::error::Error::unprocessable(e.to_string())
    }
}

/// `meta` — what wrote the file, and what it contains.
#[derive(Debug, Clone, PartialEq)]
pub struct Meta {
    pub nanobase_version: String,
    /// Whether the `data` block is present (and so will overwrite rows).
    pub includes_data: bool,
    /// Whether secrets are present in cleartext. Always `false` in a redacted
    /// export; a reader should refuse to trust a file that claims `false` and
    /// then carries a secret anyway.
    pub includes_secrets: bool,
}

impl Default for Meta {
    fn default() -> Self {
        Meta {
            nanobase_version: NANOBASE_VERSION.to_string(),
            includes_data: false,
            includes_secrets: true,
        }
    }
}

/// `app` — the human-facing identity.
#[derive(Debug, Clone, PartialEq)]
pub struct AppMeta {
    pub name: String,
    /// `local` or `remote`. `remote` means the app's database is a hosted
    /// service rather than a file on this machine.
    pub app_type: AppType,
}

/// Where an app's data lives, from the user's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppType {
    Local,
    Remote,
}

impl AppType {
    pub fn as_str(self) -> &'static str {
        match self {
            AppType::Local => "local",
            AppType::Remote => "remote",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "local" => Some(AppType::Local),
            "remote" => Some(AppType::Remote),
            _ => None,
        }
    }
}

/// `config.database` — which engine holds this app's own tables, and how to
/// reach it. `auth.database` reuses the same shape.
#[derive(Debug, Clone, PartialEq)]
pub struct DatabaseConfig {
    pub kind: DbKind,
    /// The `type` string as written, so export is not lossy.
    pub raw_type: String,
    pub url: Option<String>,
    pub token: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            kind: DbKind::Sqlite,
            raw_type: "sqlite".to_string(),
            url: None,
            token: None,
        }
    }
}

impl DatabaseConfig {
    /// An engine that needs no connection details at all.
    pub fn is_embedded(&self) -> bool {
        matches!(self.kind, DbKind::Memory | DbKind::Sqlite)
    }

    /// A serverless engine: reachable over HTTP, no interactive transactions.
    pub fn is_serverless(&self) -> bool {
        matches!(self.kind, DbKind::Libsql | DbKind::D1)
    }

    /// A reachable engine: one that needs a URL.
    pub fn needs_url(&self) -> bool {
        !matches!(self.kind, DbKind::Memory | DbKind::Sqlite)
    }
}

/// The engines an app can be pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbKind {
    /// In-process, gone on restart. Demos and tests.
    Memory,
    /// A local file. The default.
    Sqlite,
    /// libSQL/Turso: SQLite over HTTP, batched statements.
    Libsql,
    /// PostgreSQL, Neon, Supabase.
    Postgres,
    /// MySQL, PlanetScale.
    Mysql,
    /// Cloudflare D1.
    D1,
}

impl DbKind {
    /// Every spelling accepted in `config.*.database.type`.
    ///
    /// Aliases matter because users write the *provider* they think of
    /// (`"neon"`, `"turso"`, `"planetscale"`), not the protocol.
    pub fn parse(s: &str) -> Option<(DbKind, &'static str)> {
        let kind = match s.to_ascii_lowercase().as_str() {
            "memory" => DbKind::Memory,
            "sqlite" | "sqlite3" | "local" => DbKind::Sqlite,
            "libsql" | "turso" => DbKind::Libsql,
            "postgres" | "postgresql" | "pg" | "neon" | "supabase" => DbKind::Postgres,
            "mysql" | "planetscale" => DbKind::Mysql,
            "d1" | "cloudflare" => DbKind::D1,
            _ => return None,
        };
        let canonical = match kind {
            DbKind::Memory => "memory",
            DbKind::Sqlite => "sqlite",
            DbKind::Libsql => "libsql",
            DbKind::Postgres => "postgres",
            DbKind::Mysql => "mysql",
            DbKind::D1 => "d1",
        };
        Some((kind, canonical))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            DbKind::Memory => "memory",
            DbKind::Sqlite => "sqlite",
            DbKind::Libsql => "libsql",
            DbKind::Postgres => "postgres",
            DbKind::Mysql => "mysql",
            DbKind::D1 => "d1",
        }
    }

    /// Every accepted spelling, for error messages.
    pub fn all_names() -> &'static [&'static str] {
        &[
            "memory",
            "sqlite",
            "libsql",
            "turso",
            "postgres",
            "neon",
            "mysql",
            "planetscale",
            "d1",
        ]
    }
}

/// `config.storage` — where user uploads go.
#[derive(Debug, Clone, PartialEq)]
pub struct StorageConfig {
    pub kind: StorageKind,
    pub local: Option<LocalStorage>,
    pub s3: Option<S3Storage>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            kind: StorageKind::Local,
            local: Some(LocalStorage::default()),
            s3: None,
        }
    }
}

/// Local disk, or an S3-compatible service (R2, MinIO, B2, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Local,
    S3,
}

impl StorageKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "local" | "disk" | "file" => Some(StorageKind::Local),
            "s3" | "r2" | "minio" => Some(StorageKind::S3),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            StorageKind::Local => "local",
            StorageKind::S3 => "s3",
        }
    }
}

/// A directory on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalStorage {
    pub base_path: String,
    pub public_url: String,
}

impl Default for LocalStorage {
    fn default() -> Self {
        LocalStorage {
            base_path: "./data/files".to_string(),
            public_url: "http://localhost:3000/files".to_string(),
        }
    }
}

/// An S3-compatible bucket. Keys are optional here because they usually come
/// from `SECRET_S3_ACCESS_KEY` / `SECRET_S3_SECRET_KEY` instead of a file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct S3Storage {
    pub endpoint: Option<String>,
    pub bucket: Option<String>,
    pub region: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub force_path_style: bool,
}

/// `config.auth` — how users prove who they are.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AuthConfig {
    /// A separate database for identities, when the app's own tables should
    /// not hold them.
    pub database: Option<DatabaseConfig>,
    pub secret: Option<String>,
    pub providers: BTreeMap<String, Provider>,
    pub features: Features,
    pub redis: RedisConfig,
}

impl AuthConfig {
    /// Is anything actually turned on? An app with no auth is legitimate —
    /// a public read-only API, for instance — and must not be forced to
    /// configure a secret.
    pub fn is_enabled(&self) -> bool {
        self.features.email_password || self.providers.values().any(|p| p.enabled)
    }

    /// Enabled provider names, sorted.
    pub fn enabled_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|(_, p)| p.enabled)
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// One external identity provider: `google`, `github`, `facebook`, …
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Provider {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// Feature flags under `config.auth.features`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Features {
    pub email_password: bool,
}

/// `config.auth.redis` — a shared session cache, for more than one process.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RedisConfig {
    pub enabled: bool,
    pub url: Option<String>,
}

/// `config.acl` — permission rules. Kept as raw JSON: the evaluator owns the
/// rule shape, and the platform must be able to store rules it does not
/// understand yet rather than refuse the file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AclConfig {
    pub enabled: bool,
    pub rules: Vec<Value>,
}

/// `schema` — declared collections and their columns.
pub type Schema = BTreeMap<String, CollectionDef>;

/// One collection's columns, in declaration order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CollectionDef {
    pub columns: BTreeMap<String, ColumnDef>,
}

impl CollectionDef {
    /// The primary key column, if the file declares one.
    pub fn primary(&self) -> Option<(&str, &ColumnDef)> {
        self.columns
            .iter()
            .find(|(_, c)| c.primary)
            .map(|(n, c)| (n.as_str(), c))
    }
}

/// One column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub kind: ColumnType,
    /// The `type` string as written, so export is not lossy.
    pub raw_type: String,
    pub primary: bool,
    pub auto_increment: bool,
    pub not_null: bool,
    pub unique: bool,
    pub default: Option<Value>,
}

impl ColumnDef {
    pub fn new(kind: ColumnType) -> Self {
        ColumnDef {
            raw_type: kind.as_str().to_string(),
            kind,
            primary: false,
            auto_increment: false,
            not_null: false,
            unique: false,
            default: None,
        }
    }
}

/// The column types an app can declare.
///
/// These are storage types, not API types: the API's six field types
/// (`string`, `number`, `integer`, `boolean`, `datetime`, `json`) map onto
/// them, and this is the superset a file is allowed to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Integer,
    Real,
    Text,
    Boolean,
    Datetime,
    Json,
}

impl ColumnType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "integer" | "int" | "bigint" => ColumnType::Integer,
            "real" | "number" | "float" | "double" | "numeric" | "decimal" => ColumnType::Real,
            "text" | "string" | "varchar" | "char" => ColumnType::Text,
            "boolean" | "bool" => ColumnType::Boolean,
            "datetime" | "timestamp" | "date" => ColumnType::Datetime,
            "json" | "jsonb" => ColumnType::Json,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ColumnType::Integer => "integer",
            ColumnType::Real => "real",
            ColumnType::Text => "text",
            ColumnType::Boolean => "boolean",
            ColumnType::Datetime => "datetime",
            ColumnType::Json => "json",
        }
    }

    /// Does `value` fit in a column of this type?
    ///
    /// Deliberately strict: `1` is not text, `"1"` is not an integer, and a
    /// boolean is not the string `"true"`. Seed data that only works because
    /// the engine coerces it is a bug that surfaces later, in a query.
    pub fn accepts(self, value: &Value) -> bool {
        match self {
            ColumnType::Integer => match value {
                Value::Int(_) => true,
                Value::Float(f) => f.fract() == 0.0,
                _ => false,
            },
            ColumnType::Real => matches!(value, Value::Int(_) | Value::Float(_)),
            ColumnType::Text => matches!(value, Value::Str(_) | Value::Date(_)),
            ColumnType::Boolean => matches!(value, Value::Bool(_)),
            ColumnType::Datetime => match value {
                Value::Date(_) => true,
                Value::Str(s) => looks_like_datetime(s),
                _ => false,
            },
            ColumnType::Json => !matches!(value, Value::Null),
        }
    }
}

/// A light check for an ISO-8601 date or date-time: `YYYY-MM-DD`, optionally
/// with a `T`-separated time.
fn looks_like_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 10 {
        return false;
    }
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(|c| c.is_ascii_digit());
    digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10)
}

/// `data` — rows to create on import, keyed by collection.
pub type Data = BTreeMap<String, Vec<Value>>;

/// The whole document.
#[derive(Debug, Clone, PartialEq)]
pub struct AppDoc {
    pub meta: Meta,
    pub app: AppMeta,
    pub config: AppConfig,
    pub schema: Schema,
    pub data: Data,
}

/// `config` — everything but schema and data.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppConfig {
    pub auth: AuthConfig,
    pub database: DatabaseConfig,
    pub storage: StorageConfig,
    pub email: Option<Value>,
    pub acl: AclConfig,
}

impl AppDoc {
    /// A valid, empty app: SQLite, local storage, no auth, no rules, no
    /// tables. What `nanobase app new` writes.
    pub fn empty(name: impl Into<String>) -> Self {
        AppDoc {
            meta: Meta::default(),
            app: AppMeta {
                name: name.into(),
                app_type: AppType::Local,
            },
            config: AppConfig {
                database: DatabaseConfig::default(),
                storage: StorageConfig::default(),
                ..AppConfig::default()
            },
            schema: Schema::new(),
            data: Data::new(),
        }
    }

    /// Read a document and check it, reporting every problem from either
    /// stage. Nothing panics on odd input: a string where an object was
    /// expected, an unknown engine name, a row that does not fit its column.
    ///
    /// This is the one call a caller wants. A document that parses but is not
    /// valid is not useful, and reporting the two kinds of problem separately
    /// makes a user fix them one run at a time.
    pub fn from_value(v: &Value) -> Result<Self, ConfigError> {
        let mut issues = Vec::new();
        let doc = Self::read(v, &mut issues);
        doc.check(&mut issues);
        if issues.is_empty() {
            Ok(doc)
        } else {
            Err(ConfigError::new(issues))
        }
    }

    /// Parse a JSON string, then check it. A file that is not JSON at all is
    /// one issue.
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let v = mosql::parse_json(input)
            .map_err(|e| ConfigError::one("$", format!("not valid JSON: {e}")))?;
        Self::from_value(&v)
    }

    /// Read the shape only, without checking it.
    ///
    /// For a document whose secrets will come from the environment: read,
    /// [`overlay`](AppDoc::overlay), then [`validate`](AppDoc::validate).
    /// [`load`](AppDoc::load) does those three steps in order.
    pub fn read_str(input: &str) -> Result<Self, ConfigError> {
        let v = mosql::parse_json(input)
            .map_err(|e| ConfigError::one("$", format!("not valid JSON: {e}")))?;
        Self::read_value(&v)
    }

    /// [`read_str`](AppDoc::read_str) for an already-parsed [`Value`].
    pub fn read_value(v: &Value) -> Result<Self, ConfigError> {
        let mut issues = Vec::new();
        let doc = Self::read(v, &mut issues);
        if issues.is_empty() {
            Ok(doc)
        } else {
            Err(ConfigError::new(issues))
        }
    }

    /// Read a document, fill in what the environment supplies, then check it.
    ///
    /// This is what the platform does with an imported file: a document that
    /// leaves its database URL to `SECRET_DATA_DB_URL` is complete, and must
    /// not be refused for omitting it.
    pub fn load(input: &str, secrets: &Secrets) -> Result<Self, ConfigError> {
        let v = mosql::parse_json(input)
            .map_err(|e| ConfigError::one("$", format!("not valid JSON: {e}")))?;
        let mut issues = Vec::new();
        let doc = Self::read(&v, &mut issues);
        let doc = doc.overlay(secrets);
        doc.check(&mut issues);
        if issues.is_empty() {
            Ok(doc)
        } else {
            Err(ConfigError::new(issues))
        }
    }

    /// Check a document that was built in code rather than read from a file.
    ///
    /// [`parse`](AppDoc::parse) already does this, so this is only needed for
    /// an [`AppDoc`] assembled field by field.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut issues = Vec::new();
        self.check(&mut issues);
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::new(issues))
        }
    }

    /// The document as JSON, secrets included. Round-trips through
    /// [`parse`](AppDoc::parse).
    pub fn to_json(&self) -> String {
        to_json(&self.to_value())
    }

    /// The document as JSON with secrets replaced by `null` and
    /// `meta.includesSecrets` set to `false`. This is what a user shares, and
    /// what the CLI writes unless it is asked not to.
    pub fn to_json_redacted(&self) -> String {
        let mut copy = self.clone();
        copy.redact();
        copy.to_json()
    }

    /// Blank every secret in place.
    pub fn redact(&mut self) {
        self.meta.includes_secrets = false;
        self.config.auth.secret = None;
        for p in self.config.auth.providers.values_mut() {
            p.client_secret = None;
        }
        if let Some(db) = &mut self.config.auth.database {
            db.token = None;
        }
        if let Some(s3) = &mut self.config.storage.s3 {
            s3.access_key = None;
            s3.secret_key = None;
        }
    }

    /// The names of every collection that holds data but is not declared in
    /// `schema`. Legal — the app creates the table on first write — but worth
    /// reporting, because a typo in `schema` silently produces a new table.
    pub fn undeclared_data_collections(&self) -> Vec<&str> {
        self.data
            .keys()
            .filter(|k| !self.schema.contains_key(*k))
            .map(String::as_str)
            .collect()
    }
}

// ---------------------------------------------------------------- reading ---

fn opt_str<'a>(o: &'a Object, key: &str) -> Option<&'a str> {
    o.get(key).and_then(Value::as_str)
}

fn opt_bool(o: &Object, key: &str, default: bool) -> bool {
    o.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn req_str<'a>(o: &'a Object, key: &str, path: &str, issues: &mut Vec<Issue>) -> Option<&'a str> {
    match o.get(key) {
        Some(Value::Str(s)) => Some(s.as_str()),
        Some(_) => {
            issues.push(Issue::new(path, "must be a string"));
            None
        }
        None => {
            issues.push(Issue::new(path, "is required"));
            None
        }
    }
}

/// The rule an app's display name follows, shared by the config validator and
/// by the platform's rename and create paths so the two cannot disagree.
///
/// Returns the problem when there is one. Letters, digits, spaces, `-` and `_`
/// are allowed: this is a name a person types, not a database identifier.
pub fn check_app_name(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("is required");
    }
    if name.chars().count() > 63 {
        return Some("must be 63 characters or fewer");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_'))
    {
        return Some("may only contain letters, digits, spaces, - and _");
    }
    None
}

/// `[A-Za-z][A-Za-z0-9_]{0,62}` — a name that is safe as a table or column
/// on every engine once quoted, which `mosql` does.
///
/// Both cases are allowed because apps write camelCase columns (`userId`) and
/// snake_case ones (`user_id`); the engine does not care once the identifier
/// is quoted, and refusing one style would break real documents.
fn is_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 63 {
        return false;
    }
    if !b[0].is_ascii_alphabetic() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

impl AppDoc {
    fn read(v: &Value, issues: &mut Vec<Issue>) -> Self {
        let root = match v.as_object() {
            Some(o) => Some(o),
            None => {
                issues.push(Issue::new("$", "the document must be a JSON object"));
                None
            }
        };
        AppDoc {
            meta: read_meta(root.and_then(|o| o.get("meta")), issues),
            app: read_app(root.and_then(|o| o.get("app")), issues),
            config: read_config(root.and_then(|o| o.get("config")), issues),
            schema: read_schema(root.and_then(|o| o.get("schema")), issues),
            data: read_data(root.and_then(|o| o.get("data")), issues),
        }
    }

    fn check(&self, issues: &mut Vec<Issue>) {
        if let Some(problem) = check_app_name(&self.app.name) {
            issues.push(Issue::new("app.name", problem));
        }

        if self.config.auth.is_enabled() {
            match self.config.auth.secret.as_deref() {
                None => issues.push(Issue::new(
                    "config.auth.secret",
                    "is required when auth is enabled",
                )),
                Some(s) if s.len() < 8 => issues.push(Issue::new(
                    "config.auth.secret",
                    "must be at least 8 characters (32 or more is recommended)",
                )),
                Some(_) => {}
            }
        }
        // Provider credentials are not required *here*: they normally come
        // from SECRET_<PROVIDER>_CLIENT_ID / _CLIENT_SECRET. A provider that
        // is enabled and still has none is reported by
        // [`incomplete_providers`](AppDoc::incomplete_providers), not refused.
        if self.config.auth.redis.enabled
            && self
                .config
                .auth
                .redis
                .url
                .as_deref()
                .unwrap_or("")
                .is_empty()
        {
            issues.push(Issue::new(
                "config.auth.redis.url",
                "is required when redis is enabled",
            ));
        }

        self.check_database(&self.config.database, "config.database", issues);
        if let Some(db) = &self.config.auth.database {
            self.check_database(db, "config.auth.database", issues);
        }
        self.check_storage(&self.config.storage, issues);
        self.check_schema(&self.schema, issues);
        self.check_data(&self.schema, &self.data, issues);
    }

    fn check_database(&self, db: &DatabaseConfig, path: &str, issues: &mut Vec<Issue>) {
        if db.needs_url() && db.url.as_deref().unwrap_or("").is_empty() {
            issues.push(Issue::new(
                format!("{path}.url"),
                format!("is required for a \"{}\" database", db.raw_type),
            ));
        }
    }

    fn check_storage(&self, storage: &StorageConfig, issues: &mut Vec<Issue>) {
        match storage.kind {
            StorageKind::Local => match &storage.local {
                None => issues.push(Issue::new(
                    "config.storage.local",
                    "is required when storage type is \"local\"",
                )),
                Some(l) => {
                    if l.base_path.is_empty() {
                        issues.push(Issue::new(
                            "config.storage.local.basePath",
                            "must not be empty",
                        ));
                    }
                    if !(l.public_url.starts_with("http://")
                        || l.public_url.starts_with("https://"))
                    {
                        issues.push(Issue::new(
                            "config.storage.local.publicUrl",
                            "must be an http:// or https:// URL",
                        ));
                    }
                }
            },
            StorageKind::S3 => match &storage.s3 {
                None => issues.push(Issue::new(
                    "config.storage.s3",
                    "is required when storage type is \"s3\"",
                )),
                Some(s3) => {
                    if s3.endpoint.as_deref().unwrap_or("").is_empty() {
                        issues.push(Issue::new("config.storage.s3.endpoint", "is required"));
                    }
                    if s3.bucket.as_deref().unwrap_or("").is_empty() {
                        issues.push(Issue::new("config.storage.s3.bucket", "is required"));
                    }
                }
            },
        }
    }

    fn check_schema(&self, schema: &Schema, issues: &mut Vec<Issue>) {
        for (name, def) in schema {
            let path = format!("schema.{name}");
            if !is_name(name) {
                issues.push(Issue::new(
                    path.clone(),
                    format!(
                        "collection name {name:?} must be letters, digits or _, starting with a letter"
                    ),
                ));
            }
            let primaries: Vec<&str> = def
                .columns
                .iter()
                .filter(|(_, c)| c.primary)
                .map(|(n, _)| n.as_str())
                .collect();
            if primaries.len() > 1 {
                issues.push(Issue::new(
                    path.clone(),
                    format!(
                        "declares {} primary keys ({}) but a collection has one",
                        primaries.len(),
                        primaries.join(", ")
                    ),
                ));
            }
            for (cname, col) in &def.columns {
                let cpath = format!("{path}.columns.{cname}");
                if !is_name(cname) {
                    issues.push(Issue::new(
                        cpath.clone(),
                        format!(
                            "column name {cname:?} must be letters, digits or _, starting with a letter"
                        ),
                    ));
                }
                if col.auto_increment && !(col.primary && col.kind == ColumnType::Integer) {
                    issues.push(Issue::new(
                        cpath,
                        "autoIncrement is only valid on an integer primary key",
                    ));
                }
            }
        }
    }

    /// `data` must fit `schema` where the collection is declared. Undeclared
    /// collections are left alone — the app creates them on first write.
    fn check_data(&self, schema: &Schema, data: &Data, issues: &mut Vec<Issue>) {
        for (name, rows) in data {
            let Some(def) = schema.get(name) else {
                continue;
            };
            for (i, row) in rows.iter().enumerate() {
                let Some(obj) = row.as_object() else {
                    issues.push(Issue::new(format!("data.{name}[{i}]"), "must be an object"));
                    continue;
                };
                let base = format!("data.{name}[{i}]");

                for (field, value) in obj.iter() {
                    let Some(col) = def.columns.get(field) else {
                        issues.push(Issue::new(
                            format!("{base}.{field}"),
                            format!(
                                "is not a column of {name:?}; declared: {}",
                                column_list(def)
                            ),
                        ));
                        continue;
                    };
                    if value.is_null() {
                        continue;
                    }
                    if !col.kind.accepts(value) {
                        issues.push(Issue::new(
                            format!("{base}.{field}"),
                            format!("{} does not fit a {} column", describe(value), col.raw_type),
                        ));
                    }
                }

                for (cname, col) in &def.columns {
                    if obj.get(cname).is_some() || col.primary && col.auto_increment {
                        continue;
                    }
                    if col.not_null && col.default.is_none() {
                        issues.push(Issue::new(
                            format!("{base}.{cname}"),
                            "is required: the column is notNull and has no default",
                        ));
                    }
                }
            }
        }
    }

    /// The document as a [`Value`], secrets included.
    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("meta", meta_to_value(&self.meta));
        o.set("app", app_meta_to_value(&self.app));
        o.set("config", config_to_value(&self.config));
        o.set("schema", schema_to_value(&self.schema));
        o.set("data", data_to_value(&self.data));
        Value::Object(o)
    }

    /// Indented JSON, for a file a human will edit. Scalars are printed by
    /// `mosql::to_json`, so string escaping is not re-implemented here.
    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        write_pretty(&self.to_value(), 0, &mut out);
        out.push('\n');
        out
    }

    /// [`to_json_redacted`](AppDoc::to_json_redacted), indented.
    pub fn to_json_pretty_redacted(&self) -> String {
        let mut copy = self.clone();
        copy.redact();
        let mut out = String::new();
        write_pretty(&copy.to_value(), 0, &mut out);
        out.push('\n');
        out
    }
}

fn column_list(def: &CollectionDef) -> String {
    let mut names: Vec<&str> = def.columns.keys().map(String::as_str).collect();
    names.sort_unstable();
    names.join(", ")
}

/// A short, language-neutral description for "does not fit" messages.
fn describe(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Int(_) => "an integer",
        Value::Float(_) => "a number",
        Value::Str(_) | Value::Date(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn write_pretty(v: &Value, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    let pad_inner = "  ".repeat(indent + 1);
    match v {
        Value::Object(o) if !o.is_empty() => {
            out.push_str("{\n");
            for (i, (k, val)) in o.iter().enumerate() {
                out.push_str(&pad_inner);
                out.push_str(&to_json(&Value::str(k.clone())));
                out.push_str(": ");
                write_pretty(val, indent + 1, out);
                if i + 1 < o.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push('}');
        }
        Value::Array(a) if !a.is_empty() => {
            out.push_str("[\n");
            for (i, val) in a.iter().enumerate() {
                out.push_str(&pad_inner);
                write_pretty(val, indent + 1, out);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&pad);
            out.push(']');
        }
        other => out.push_str(&to_json(other)),
    }
}

// ---------------------------------------------------------------- writing ---

fn meta_to_value(m: &Meta) -> Value {
    let mut o = Object::new();
    o.set("nanobaseVersion", Value::str(m.nanobase_version.clone()));
    o.set("includesData", Value::Bool(m.includes_data));
    o.set("includesSecrets", Value::Bool(m.includes_secrets));
    Value::Object(o)
}

fn app_meta_to_value(a: &AppMeta) -> Value {
    let mut o = Object::new();
    o.set("name", Value::str(a.name.clone()));
    o.set("type", Value::str(a.app_type.as_str()));
    Value::Object(o)
}

fn config_to_value(c: &AppConfig) -> Value {
    let mut o = Object::new();
    o.set("auth", auth_to_value(&c.auth));
    o.set("database", database_to_value(&c.database));
    o.set("storage", storage_to_value(&c.storage));
    o.set("email", c.email.clone().unwrap_or(Value::Null));
    o.set("acl", acl_to_value(&c.acl));
    Value::Object(o)
}

fn auth_to_value(a: &AuthConfig) -> Value {
    let mut o = Object::new();
    if let Some(db) = &a.database {
        o.set("database", database_to_value(db));
    }
    if let Some(secret) = &a.secret {
        o.set("secret", Value::str(secret.clone()));
    }
    if !a.providers.is_empty() {
        let mut p = Object::new();
        for (name, provider) in &a.providers {
            p.set(name.clone(), provider_to_value(provider));
        }
        o.set("providers", Value::Object(p));
    }
    let mut f = Object::new();
    f.set("emailPassword", Value::Bool(a.features.email_password));
    o.set("features", Value::Object(f));
    let mut r = Object::new();
    r.set("enabled", Value::Bool(a.redis.enabled));
    if let Some(url) = &a.redis.url {
        r.set("url", Value::str(url.clone()));
    }
    o.set("redis", Value::Object(r));
    Value::Object(o)
}

fn provider_to_value(p: &Provider) -> Value {
    let mut o = Object::new();
    o.set("enabled", Value::Bool(p.enabled));
    if let Some(id) = &p.client_id {
        o.set("clientId", Value::str(id.clone()));
    }
    if let Some(secret) = &p.client_secret {
        o.set("clientSecret", Value::str(secret.clone()));
    }
    Value::Object(o)
}

fn database_to_value(d: &DatabaseConfig) -> Value {
    let mut o = Object::new();
    o.set("type", Value::str(d.raw_type.clone()));
    if let Some(url) = &d.url {
        o.set("url", Value::str(url.clone()));
    }
    if let Some(token) = &d.token {
        o.set("token", Value::str(token.clone()));
    }
    Value::Object(o)
}

fn storage_to_value(s: &StorageConfig) -> Value {
    let mut o = Object::new();
    o.set("type", Value::str(s.kind.as_str()));
    if let Some(l) = &s.local {
        let mut lo = Object::new();
        lo.set("basePath", Value::str(l.base_path.clone()));
        lo.set("publicUrl", Value::str(l.public_url.clone()));
        o.set("local", Value::Object(lo));
    }
    if let Some(x) = &s.s3 {
        let mut so = Object::new();
        if let Some(v) = &x.endpoint {
            so.set("endpoint", Value::str(v.clone()));
        }
        if let Some(v) = &x.bucket {
            so.set("bucket", Value::str(v.clone()));
        }
        if let Some(v) = &x.region {
            so.set("region", Value::str(v.clone()));
        }
        if let Some(v) = &x.access_key {
            so.set("accessKey", Value::str(v.clone()));
        }
        if let Some(v) = &x.secret_key {
            so.set("secretKey", Value::str(v.clone()));
        }
        if x.force_path_style {
            so.set("forcePathStyle", Value::Bool(true));
        }
        o.set("s3", Value::Object(so));
    }
    Value::Object(o)
}

fn acl_to_value(a: &AclConfig) -> Value {
    let mut o = Object::new();
    o.set("enabled", Value::Bool(a.enabled));
    o.set("rules", Value::Array(a.rules.clone()));
    Value::Object(o)
}

fn schema_to_value(schema: &Schema) -> Value {
    let mut out = Object::new();
    for (name, def) in schema {
        let mut cols = Object::new();
        for (cname, col) in &def.columns {
            cols.set(cname.clone(), column_to_value(col));
        }
        let mut d = Object::new();
        d.set("columns", Value::Object(cols));
        out.set(name.clone(), Value::Object(d));
    }
    Value::Object(out)
}

fn column_to_value(c: &ColumnDef) -> Value {
    let mut o = Object::new();
    o.set("type", Value::str(c.raw_type.clone()));
    if c.primary {
        o.set("primary", Value::Bool(true));
    }
    if c.auto_increment {
        o.set("autoIncrement", Value::Bool(true));
    }
    if c.not_null {
        o.set("notNull", Value::Bool(true));
    }
    if c.unique {
        o.set("unique", Value::Bool(true));
    }
    if let Some(d) = &c.default {
        o.set("default", d.clone());
    }
    Value::Object(o)
}

fn data_to_value(data: &Data) -> Value {
    let mut out = Object::new();
    for (name, rows) in data {
        out.set(name.clone(), Value::Array(rows.clone()));
    }
    Value::Object(out)
}

// ---------------------------------------------------------- section readers ---

fn read_meta(v: Option<&Value>, issues: &mut Vec<Issue>) -> Meta {
    let mut m = Meta::default();
    let Some(x) = v else { return m };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("meta", "must be an object"));
        return m;
    };
    if let Some(s) = opt_str(o, "nanobaseVersion") {
        m.nanobase_version = s.to_string();
    }
    m.includes_data = opt_bool(o, "includesData", false);
    m.includes_secrets = opt_bool(o, "includesSecrets", true);
    m
}

fn read_app(v: Option<&Value>, issues: &mut Vec<Issue>) -> AppMeta {
    let mut a = AppMeta {
        name: String::new(),
        app_type: AppType::Local,
    };
    let Some(x) = v else {
        issues.push(Issue::new("app", "is required: a document needs a name"));
        return a;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("app", "must be an object"));
        return a;
    };
    a.name = req_str(o, "name", "app.name", issues)
        .unwrap_or_default()
        .to_string();
    match opt_str(o, "type") {
        Some(t) => match AppType::parse(t) {
            Some(kind) => a.app_type = kind,
            None => issues.push(Issue::new(
                "app.type",
                format!("must be \"local\" or \"remote\", not {t:?}"),
            )),
        },
        None => a.app_type = AppType::Local,
    }
    a
}

fn read_config(v: Option<&Value>, issues: &mut Vec<Issue>) -> AppConfig {
    let mut c = AppConfig::default();
    let Some(x) = v else {
        issues.push(Issue::new("config", "is required"));
        return c;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("config", "must be an object"));
        return c;
    };
    c.auth = read_auth(o.get("auth"), issues);
    c.database = read_database(o.get("database"), "config.database", issues, true);
    c.storage = read_storage(o.get("storage"), issues);
    c.email = o.get("email").cloned().filter(|e| !e.is_null());
    c.acl = read_acl(o.get("acl"), issues);
    c
}

fn read_auth(v: Option<&Value>, issues: &mut Vec<Issue>) -> AuthConfig {
    let mut a = AuthConfig::default();
    let Some(x) = v else { return a };
    if x.is_null() {
        return a;
    }
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("config.auth", "must be an object"));
        return a;
    };
    if let Some(db) = o.get("database").filter(|d| !d.is_null()) {
        a.database = Some(read_database(
            Some(db),
            "config.auth.database",
            issues,
            false,
        ));
    }
    a.secret = opt_str(o, "secret").map(str::to_string);
    if let Some(p) = o.get("providers").filter(|p| !p.is_null()) {
        match p.as_object() {
            Some(providers) => {
                for (name, provider) in providers.iter() {
                    a.providers.insert(
                        name.clone(),
                        read_provider(provider, &format!("config.auth.providers.{name}"), issues),
                    );
                }
            }
            None => issues.push(Issue::new(
                "config.auth.providers",
                "must be an object of provider name -> settings",
            )),
        }
    }
    if let Some(f) = o.get("features").filter(|f| !f.is_null()) {
        if let Some(fo) = f.as_object() {
            a.features.email_password = opt_bool(fo, "emailPassword", false);
        }
    }
    if let Some(r) = o.get("redis").filter(|r| !r.is_null()) {
        if let Some(ro) = r.as_object() {
            a.redis.enabled = opt_bool(ro, "enabled", false);
            a.redis.url = opt_str(ro, "url").map(str::to_string);
        }
    }
    a
}

fn read_provider(v: &Value, path: &str, issues: &mut Vec<Issue>) -> Provider {
    let mut p = Provider::default();
    let Some(o) = v.as_object() else {
        issues.push(Issue::new(path, "must be an object"));
        return p;
    };
    p.enabled = opt_bool(o, "enabled", false);
    p.client_id = opt_str(o, "clientId").map(str::to_string);
    p.client_secret = opt_str(o, "clientSecret").map(str::to_string);
    p
}

fn read_database(
    v: Option<&Value>,
    path: &str,
    issues: &mut Vec<Issue>,
    required: bool,
) -> DatabaseConfig {
    let mut d = DatabaseConfig::default();
    let Some(x) = v.filter(|x| !x.is_null()) else {
        if required {
            issues.push(Issue::new(format!("{path}.type"), "is required"));
        }
        return d;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new(path, "must be an object"));
        return d;
    };
    match opt_str(o, "type") {
        Some(t) => match DbKind::parse(t) {
            Some((kind, _)) => {
                d.kind = kind;
                d.raw_type = t.to_string();
            }
            None => issues.push(Issue::new(
                format!("{path}.type"),
                format!(
                    "unknown database {t:?}; expected one of {}",
                    DbKind::all_names().join(", ")
                ),
            )),
        },
        None => issues.push(Issue::new(format!("{path}.type"), "is required")),
    }
    d.url = opt_str(o, "url").map(str::to_string);
    d.token = opt_str(o, "token").map(str::to_string);
    d
}

fn read_storage(v: Option<&Value>, issues: &mut Vec<Issue>) -> StorageConfig {
    let mut s = StorageConfig::default();
    let Some(x) = v.filter(|x| !x.is_null()) else {
        return s;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("config.storage", "must be an object"));
        return s;
    };
    match opt_str(o, "type") {
        Some(t) => match StorageKind::parse(t) {
            Some(kind) => s.kind = kind,
            None => issues.push(Issue::new(
                "config.storage.type",
                format!("unknown storage type {t:?}; expected \"local\" or \"s3\""),
            )),
        },
        None => issues.push(Issue::new("config.storage.type", "is required")),
    }
    if let Some(l) = o.get("local").filter(|l| !l.is_null()) {
        s.local = Some(read_local(l, issues));
    }
    if let Some(x) = o.get("s3").filter(|x| !x.is_null()) {
        s.s3 = Some(read_s3(x, issues));
    }
    s
}

fn read_local(v: &Value, issues: &mut Vec<Issue>) -> LocalStorage {
    let mut l = LocalStorage::default();
    let Some(o) = v.as_object() else {
        issues.push(Issue::new(
            "config.storage.local",
            "must be an object with basePath and publicUrl",
        ));
        return l;
    };
    if let Some(p) = opt_str(o, "basePath") {
        l.base_path = p.to_string();
    }
    if let Some(p) = opt_str(o, "publicUrl") {
        l.public_url = p.to_string();
    }
    l
}

fn read_s3(v: &Value, issues: &mut Vec<Issue>) -> S3Storage {
    let mut s = S3Storage::default();
    let Some(o) = v.as_object() else {
        issues.push(Issue::new(
            "config.storage.s3",
            "must be an object with endpoint and bucket",
        ));
        return s;
    };
    s.endpoint = opt_str(o, "endpoint").map(str::to_string);
    s.bucket = opt_str(o, "bucket").map(str::to_string);
    s.region = opt_str(o, "region").map(str::to_string);
    s.access_key = opt_str(o, "accessKey").map(str::to_string);
    s.secret_key = opt_str(o, "secretKey").map(str::to_string);
    s.force_path_style = opt_bool(o, "forcePathStyle", false);
    s
}

fn read_acl(v: Option<&Value>, issues: &mut Vec<Issue>) -> AclConfig {
    let mut a = AclConfig::default();
    let Some(x) = v.filter(|x| !x.is_null()) else {
        return a;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new("config.acl", "must be an object"));
        return a;
    };
    a.enabled = opt_bool(o, "enabled", false);
    if let Some(rules) = o.get("rules").filter(|r| !r.is_null()) {
        match rules.as_array() {
            Some(list) => a.rules = list.clone(),
            None => issues.push(Issue::new("config.acl.rules", "must be an array")),
        }
    }
    a
}

fn read_schema(v: Option<&Value>, issues: &mut Vec<Issue>) -> Schema {
    let mut out = Schema::new();
    let Some(x) = v.filter(|x| !x.is_null()) else {
        return out;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new(
            "schema",
            "must be an object of collection name -> { columns: {…} }",
        ));
        return out;
    };
    for (name, collection) in o.iter() {
        let path = format!("schema.{name}");
        if !is_name(name) {
            issues.push(Issue::new(
                path.clone(),
                format!(
                    "collection name {name:?} must be letters, digits or _, starting with a letter"
                ),
            ));
        }
        let mut def = CollectionDef::default();
        let columns = collection
            .as_object()
            .and_then(|c| c.get("columns"))
            .and_then(Value::as_object);
        match columns {
            Some(cols) if !cols.is_empty() => {
                for (cname, col) in cols.iter() {
                    read_column(cname, col, &path, &mut def, issues);
                }
            }
            _ => issues.push(Issue::new(
                format!("{path}.columns"),
                "is required and must be a non-empty object",
            )),
        }
        out.insert(name.clone(), def);
    }
    out
}

fn read_column(
    name: &str,
    v: &Value,
    path: &str,
    def: &mut CollectionDef,
    issues: &mut Vec<Issue>,
) {
    let cpath = format!("{path}.columns.{name}");
    if !is_name(name) {
        issues.push(Issue::new(
            cpath.clone(),
            format!("column name {name:?} must be letters, digits or _, starting with a letter"),
        ));
    }
    let Some(o) = v.as_object() else {
        issues.push(Issue::new(cpath, "must be an object with a \"type\""));
        return;
    };
    let raw = opt_str(o, "type").unwrap_or("");
    let Some(kind) = ColumnType::parse(raw) else {
        issues.push(Issue::new(
            format!("{cpath}.type"),
            format!(
                "unknown column type {raw:?}; expected integer, real, text, boolean, datetime or json"
            ),
        ));
        return;
    };
    let mut col = ColumnDef::new(kind);
    col.raw_type = raw.to_string();
    col.primary = opt_bool(o, "primary", false);
    col.auto_increment = opt_bool(o, "autoIncrement", false);
    col.not_null = opt_bool(o, "notNull", false);
    col.unique = opt_bool(o, "unique", false);
    col.default = o.get("default").cloned().filter(|d| !d.is_null());
    def.columns.insert(name.to_string(), col);
}

fn read_data(v: Option<&Value>, issues: &mut Vec<Issue>) -> Data {
    let mut out = Data::new();
    let Some(x) = v.filter(|x| !x.is_null()) else {
        return out;
    };
    let Some(o) = x.as_object() else {
        issues.push(Issue::new(
            "data",
            "must be an object of collection name -> array of rows",
        ));
        return out;
    };
    for (name, rows) in o.iter() {
        let path = format!("data.{name}");
        let Some(list) = rows.as_array() else {
            issues.push(Issue::new(path, "must be an array of row objects"));
            continue;
        };
        let mut vec = Vec::with_capacity(list.len());
        for (i, row) in list.iter().enumerate() {
            if !row.is_object() {
                issues.push(Issue::new(format!("{path}[{i}]"), "must be an object"));
                continue;
            }
            vec.push(row.clone());
        }
        out.insert(name.clone(), vec);
    }
    out
}

// ---------------------------------------------------------------- secrets ---
//
// A config file is committed, shared and exported; a secret is none of those.
// So a document may leave a secret out and have it supplied by the
// environment. `SECRET_*` is the only prefix honoured, which keeps an
// unrelated variable from silently configuring a platform.

/// Secrets supplied outside the document, normally the environment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    vars: BTreeMap<String, String>,
}

impl Secrets {
    /// Every `SECRET_*` variable in the process environment.
    pub fn from_env() -> Self {
        let mut vars = BTreeMap::new();
        for (k, v) in std::env::vars() {
            if k.starts_with("SECRET_") {
                vars.insert(k, v);
            }
        }
        Secrets { vars }
    }

    /// A fixed set, for tests and for a platform that keeps secrets elsewhere.
    pub fn from_pairs<'a, I>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (&'a str, &'a str)>,
    {
        Secrets {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    /// Look one up by its full name, `SECRET_JWT_SECRET` for instance.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.vars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// `SECRET_JWT_SECRET`, if set.
    pub fn jwt_secret(&self) -> Option<&str> {
        self.get("SECRET_JWT_SECRET")
    }

    /// `SECRET_<PROVIDER>_CLIENT_ID`, for any provider name.
    pub fn provider_client_id(&self, provider: &str) -> Option<&str> {
        self.get(&provider_var(provider, "CLIENT_ID"))
    }

    /// `SECRET_<PROVIDER>_CLIENT_SECRET`, for any provider name.
    pub fn provider_client_secret(&self, provider: &str) -> Option<&str> {
        self.get(&provider_var(provider, "CLIENT_SECRET"))
    }
}

/// `google` -> `SECRET_GOOGLE_CLIENT_ID`.
fn provider_var(provider: &str, field: &str) -> String {
    let mut name = String::from("SECRET_");
    for c in provider.chars() {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_uppercase());
        } else {
            name.push('_');
        }
    }
    name.push('_');
    name.push_str(field);
    name
}

impl AppDoc {
    /// This document with every missing secret filled in from `secrets`.
    ///
    /// The environment wins only where the file is silent: a value written in
    /// the file is a deliberate choice for that app, and overriding it from a
    /// variable named after a *different* app would be a surprising way to
    /// leak one app's credentials into another.
    pub fn overlay(&self, secrets: &Secrets) -> AppDoc {
        let mut doc = self.clone();

        if doc.config.auth.secret.is_none() {
            doc.config.auth.secret = secrets.jwt_secret().map(str::to_string);
        }
        for (name, provider) in doc.config.auth.providers.iter_mut() {
            if provider.client_id.as_deref().unwrap_or("").is_empty() {
                provider.client_id = secrets
                    .provider_client_id(name)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
            }
            if provider.client_secret.as_deref().unwrap_or("").is_empty() {
                provider.client_secret = secrets
                    .provider_client_secret(name)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
            }
        }

        fill_database(
            &mut doc.config.database,
            secrets,
            "SECRET_DATA_DB_URL",
            "SECRET_DATA_DB_TOKEN",
        );
        if let Some(db) = doc.config.auth.database.as_mut() {
            fill_database(db, secrets, "SECRET_AUTH_DB_URL", "SECRET_AUTH_DB_TOKEN");
        }

        if let Some(s3) = doc.config.storage.s3.as_mut() {
            if s3.endpoint.as_deref().unwrap_or("").is_empty() {
                s3.endpoint = secrets.get("SECRET_S3_ENDPOINT").map(str::to_string);
            }
            if s3.access_key.as_deref().unwrap_or("").is_empty() {
                s3.access_key = secrets.get("SECRET_S3_ACCESS_KEY").map(str::to_string);
            }
            if s3.secret_key.as_deref().unwrap_or("").is_empty() {
                s3.secret_key = secrets.get("SECRET_S3_SECRET_KEY").map(str::to_string);
            }
        }

        if let Some(Value::Object(email)) = doc.config.email.as_mut() {
            if email
                .get("apiKey")
                .and_then(Value::as_str)
                .unwrap_or("")
                .is_empty()
            {
                if let Some(key) = secrets.get("SECRET_EMAIL_API_KEY") {
                    email.set("apiKey", Value::str(key));
                }
            }
        }

        doc
    }

    /// Enabled providers that still have no credentials after [`overlay`].
    ///
    /// Not an import error — the app is otherwise fine — but the OAuth flow
    /// cannot start, so the user is told instead of finding out at runtime.
    pub fn incomplete_providers(&self) -> Vec<&str> {
        self.config
            .auth
            .providers
            .iter()
            .filter(|(_, p)| {
                p.enabled
                    && (p.client_id.as_deref().unwrap_or("").is_empty()
                        || p.client_secret.as_deref().unwrap_or("").is_empty())
            })
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

fn fill_database(db: &mut DatabaseConfig, secrets: &Secrets, url: &str, token: &str) {
    if db.url.as_deref().unwrap_or("").is_empty() {
        db.url = secrets.get(url).map(str::to_string);
    }
    if db.token.as_deref().unwrap_or("").is_empty() {
        db.token = secrets.get(token).map(str::to_string);
    }
}
