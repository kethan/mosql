//! `services/storage.js` — object storage as a service.
//!
//! uflite ships two backends behind one factory: a local filesystem service and
//! an S3-compatible one. They share a method surface (`find`, `get`, `create`,
//! `remove`, `presignUpload`, `presignDownload`); only the local one can move
//! bytes itself (`upload`, `download`), because S3 uploads go straight from the
//! client through a presigned URL.
//!
//! * [`LocalStorage`] — fully implemented over `std::fs`, with the same key
//!   safety as the JavaScript service (no absolute keys, no `..`, no symlink
//!   traversal, `O_NOFOLLOW` where the platform has it).
//! * [`S3Storage`] — every validation, default and mapping from storage.js,
//!   over an [`S3Client`] trait. Signing (SigV4) and HTTPS belong to the
//!   transport you implement; the AWS SDK's behaviour is the trait's contract.
//! * [`create_storage`] — the `type: 'local' | 's3'` dispatcher.

pub mod local;
pub mod s3;

use mosql::{Object, Value};

pub use local::{FileBody, LocalOptions, LocalStorage, Uploaded};
pub use s3::{HeadObject, ListResult, PresignCommand, S3Client, S3Options, S3Storage};

use crate::error::{Error, Result};

/// The query a `find` receives. The two backends validate the same fields
/// differently (a local prefix must be a valid key; an S3 prefix any string),
/// so the raw values are kept until the service interprets them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StorageQuery {
    pub prefix: Option<Value>,
    /// `limit` or, Feathers-style, `$limit`.
    pub limit: Option<Value>,
    pub cursor: Option<Value>,
}

impl StorageQuery {
    /// `const query = params.query || params` — an object with `prefix`,
    /// `limit`/`$limit` and `cursor`. A `null` is the same as an absent key.
    pub fn from_value(params: &Value) -> StorageQuery {
        let v = match params.get("query") {
            Some(q) if q.is_truthy() => q,
            _ => params,
        };
        StorageQuery {
            prefix: present(v.get("prefix")),
            limit: present(v.get("limit")).or_else(|| present(v.get("$limit"))),
            cursor: present(v.get("cursor")),
        }
    }

    pub fn prefix(mut self, p: impl Into<String>) -> Self {
        self.prefix = Some(Value::Str(p.into()));
        self
    }

    pub fn limit(mut self, l: i64) -> Self {
        self.limit = Some(Value::Int(l));
        self
    }

    pub fn cursor(mut self, c: impl Into<String>) -> Self {
        self.cursor = Some(Value::Str(c.into()));
        self
    }
}

fn present(v: Option<&Value>) -> Option<Value> {
    match v {
        None | Some(Value::Null) => None,
        Some(x) => Some(x.clone()),
    }
}

pub const DEFAULT_LIMIT: i64 = 100;
pub const MAX_LIMIT: i64 = 1000;

/// `requireLimit` (S3): `undefined`/`null` → default; else a number that is an
/// integer in `1..=1000`, or `RangeError("limit must be an integer between 1 and 1000")`.
pub fn check_limit(value: Option<&Value>) -> Result<i64> {
    match value {
        None | Some(Value::Null) => Ok(DEFAULT_LIMIT),
        Some(v) => match v.to_number() {
            Some(n)
                if n.is_finite() && n.fract() == 0.0 && (1.0..=MAX_LIMIT as f64).contains(&n) =>
            {
                Ok(n as i64)
            }
            _ => Err(Error::bad_request(format!(
                "limit must be an integer between 1 and {MAX_LIMIT}"
            ))),
        },
    }
}

/// One listed object. Field shapes follow the JavaScript services exactly when
/// serialised; see the `to_value_*` methods.
#[derive(Debug, Clone, PartialEq)]
pub struct ObjectInfo {
    pub key: String,
    pub size: Option<i64>,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    /// ISO-8601, like `Date.toISOString()`; the S3 SDK's `LastModified`.
    pub last_modified: Option<String>,
    /// S3 `HeadObject.Metadata`; absent from listings and from local files.
    pub metadata: Option<Value>,
}

impl ObjectInfo {
    /// Local `metadata()`: `{ key, size, contentType, lastModified }`.
    pub fn to_value_local(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        o.set("size", Value::Int(self.size.unwrap_or(0)));
        o.set(
            "contentType",
            Value::str(
                self.content_type
                    .as_deref()
                    .unwrap_or("application/octet-stream"),
            ),
        );
        o.set(
            "lastModified",
            self.last_modified
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        Value::Object(o)
    }

    /// S3 `ListObjectsV2` summary: `{ key, size, contentType: null, etag, lastModified }`.
    pub fn to_value_s3_list(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        o.set("size", self.size.map(Value::Int).unwrap_or(Value::Null));
        o.set("contentType", Value::Null);
        o.set(
            "etag",
            self.etag.clone().map(Value::Str).unwrap_or(Value::Null),
        );
        o.set(
            "lastModified",
            self.last_modified
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        Value::Object(o)
    }

    /// S3 `HeadObject`: `{ key, size, contentType, etag, lastModified, metadata }`.
    pub fn to_value_s3_head(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        o.set("size", self.size.map(Value::Int).unwrap_or(Value::Null));
        o.set(
            "contentType",
            self.content_type
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        o.set(
            "etag",
            self.etag.clone().map(Value::Str).unwrap_or(Value::Null),
        );
        o.set(
            "lastModified",
            self.last_modified
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        o.set(
            "metadata",
            self.metadata.clone().unwrap_or_else(Value::object),
        );
        Value::Object(o)
    }
}

/// A page of objects: `next_cursor` is set only when more objects remain.
#[derive(Debug, Clone, PartialEq)]
pub struct StoragePage {
    pub data: Vec<ObjectInfo>,
    pub next_cursor: Option<String>,
}

impl StoragePage {
    /// Apply the JavaScript cursor rule to a sorted key list.
    pub fn from_sorted(infos: Vec<ObjectInfo>, cursor: Option<&str>, limit: i64) -> StoragePage {
        let remaining: Vec<ObjectInfo> = match cursor {
            Some(c) if !c.is_empty() => infos.into_iter().filter(|i| i.key.as_str() > c).collect(),
            _ => infos,
        };
        let more = remaining.len() as i64 > limit;
        let data: Vec<ObjectInfo> = remaining.into_iter().take(limit as usize).collect();
        let next_cursor = if more {
            data.last().map(|i| i.key.clone())
        } else {
            None
        };
        StoragePage { data, next_cursor }
    }
}

impl StoragePage {
    /// `{ data: [{ key, size, contentType, lastModified }], nextCursor }` —
    /// the local `find` shape (each item is `metadata()`).
    pub fn to_value_local(&self) -> Value {
        let mut o = Object::new();
        o.set(
            "data",
            Value::Array(self.data.iter().map(ObjectInfo::to_value_local).collect()),
        );
        o.set(
            "nextCursor",
            self.next_cursor
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        Value::Object(o)
    }

    /// `{ data: [{ key, size, contentType: null, etag, lastModified }], nextCursor }` —
    /// the `ListObjectsV2` shape.
    pub fn to_value_s3(&self) -> Value {
        let mut o = Object::new();
        o.set(
            "data",
            Value::Array(self.data.iter().map(ObjectInfo::to_value_s3_list).collect()),
        );
        o.set(
            "nextCursor",
            self.next_cursor
                .clone()
                .map(Value::Str)
                .unwrap_or(Value::Null),
        );
        Value::Object(o)
    }
}

/// A presigned (or local) upload/download grant.
#[derive(Debug, Clone, PartialEq)]
pub struct Presigned {
    pub key: String,
    /// `None` for local uploads when the request URL is unknown.
    pub url: Option<String>,
    pub method: &'static str,
    /// Request headers the client must send, e.g. `{ "Content-Type": "..." }`.
    pub headers: Value,
    /// Seconds the grant is valid for; `None` for local storage.
    pub expires_in: Option<i64>,
}

impl Presigned {
    /// `{ key, url, method, headers, expiresIn }` — the local-upload shape.
    pub fn to_value_upload(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        o.set(
            "url",
            self.url.clone().map(Value::Str).unwrap_or(Value::Null),
        );
        o.set("method", Value::str(self.method));
        o.set("headers", self.headers.clone());
        o.set(
            "expiresIn",
            self.expires_in.map(Value::Int).unwrap_or(Value::Null),
        );
        Value::Object(o)
    }

    /// `{ key, url, expiresIn }` — both download shapes.
    pub fn to_value_download(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        o.set(
            "url",
            self.url.clone().map(Value::Str).unwrap_or(Value::Null),
        );
        o.set(
            "expiresIn",
            self.expires_in.map(Value::Int).unwrap_or(Value::Null),
        );
        Value::Object(o)
    }
}

/// `{ key, deleted: true }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deleted {
    pub key: String,
}

impl From<Deleted> for Value {
    fn from(d: Deleted) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&d.key));
        o.set("deleted", Value::Bool(true));
        Value::Object(o)
    }
}

/// An upload as the local service receives it.
#[derive(Debug, Clone, PartialEq)]
pub struct UploadInput {
    pub key: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

/// A presign request as typed Rust: `{ key, contentType?, expiresIn?, contentLength? }`.
/// Each backend validates the fields it cares about, so this only carries them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PresignInput {
    pub key: String,
    pub content_type: Option<String>,
    pub expires_in: Option<i64>,
    pub content_length: Option<i64>,
}

impl PresignInput {
    pub fn new(key: impl Into<String>) -> Self {
        PresignInput {
            key: key.into(),
            ..PresignInput::default()
        }
    }

    pub fn content_type(mut self, ct: impl Into<String>) -> Self {
        self.content_type = Some(ct.into());
        self
    }

    pub fn expires_in(mut self, s: i64) -> Self {
        self.expires_in = Some(s);
        self
    }

    pub fn to_value(&self) -> Value {
        let mut o = Object::new();
        o.set("key", Value::str(&self.key));
        if let Some(ct) = &self.content_type {
            o.set("contentType", Value::str(ct));
        }
        if let Some(e) = self.expires_in {
            o.set("expiresIn", Value::Int(e));
        }
        if let Some(l) = self.content_length {
            o.set("contentLength", Value::Int(l));
        }
        Value::Object(o)
    }
}

// ---------------------------------------------------------------------------
// createStorageService — the dispatcher
// ---------------------------------------------------------------------------

/// Which backend `createStorageService(options)` selects.
#[derive(Debug, Clone, PartialEq)]
pub enum StorageKind {
    Local(LocalOptions),
    S3(S3Options),
}

impl StorageKind {
    /// `type` wins; otherwise any S3 option present selects S3, else local.
    pub fn from_value(options: &Value) -> Result<StorageKind> {
        // `!options || typeof options !== 'object' || Array.isArray(options)`
        if !options.is_object() {
            return Err(Error::bad_request("storage options must be an object"));
        }
        let s3_keys = [
            "bucket",
            "region",
            "accessKeyId",
            "secretAccessKey",
            "sessionToken",
            "endpoint",
            "forcePathStyle",
            "client",
            "presign",
        ];
        let has_s3 = s3_keys
            .iter()
            .any(|k| options.get(k).is_some_and(|v| !v.is_null()));
        let kind = match options.get("type") {
            None | Some(Value::Null) => {
                if has_s3 {
                    "s3"
                } else {
                    "local"
                }
            }
            Some(Value::Str(s)) => s.as_str(),
            Some(other) => {
                return Err(Error::bad_request(format!(
                    "unsupported storage type '{}'",
                    other.to_js_string()
                )))
            }
        };
        match kind {
            "local" => Ok(StorageKind::Local(LocalOptions::from_value(options)?)),
            "s3" => Ok(StorageKind::S3(S3Options::from_value(options)?)),
            other => Err(Error::bad_request(format!(
                "unsupported storage type '{other}'"
            ))),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            StorageKind::Local(_) => "local",
            StorageKind::S3(_) => "s3",
        }
    }
}

/// An [`S3Client`] that was never configured: every call is an error. It is
/// the client type of a [`Storage`] built without one, so a local
/// configuration never has to name an S3 type.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoS3Client;

impl S3Client for NoS3Client {
    async fn head_object(&self, _bucket: &str, _key: &str) -> Result<HeadObject> {
        Err(no_client())
    }
    async fn delete_object(&self, _bucket: &str, _key: &str) -> Result<()> {
        Err(no_client())
    }
    async fn list_objects(
        &self,
        _bucket: &str,
        _prefix: Option<&str>,
        _max_keys: i64,
        _continuation_token: Option<&str>,
    ) -> Result<ListResult> {
        Err(no_client())
    }
    async fn presign(&self, _command: PresignCommand<'_>, _expires_in: i64) -> Result<String> {
        Err(no_client())
    }
}

fn no_client() -> Error {
    Error::internal("[createStorageService] an S3Client is required for type 's3'")
}

/// The common surface of both backends, as `createStorageService` returns it.
#[derive(Debug)]
pub enum Storage<C: S3Client = NoS3Client> {
    Local(LocalStorage),
    S3(S3Storage<C>),
}

impl Storage<NoS3Client> {
    /// `createStorageService(options)` without a client: fine for `local`,
    /// and the only thing an S3 configuration can do without one is fail.
    pub fn new(options: &Value) -> Result<Storage<NoS3Client>> {
        match StorageKind::from_value(options)? {
            StorageKind::Local(o) => Ok(Storage::Local(LocalStorage::new(o)?)),
            StorageKind::S3(_) => Err(Error::internal(
                "[createStorageService] an S3Client is required for type 's3'",
            )),
        }
    }
}

impl<C: S3Client> Storage<C> {
    /// `createStorageService(options, client)`.
    pub fn with_client(options: &Value, client: C) -> Result<Storage<C>> {
        match StorageKind::from_value(options)? {
            StorageKind::Local(o) => Ok(Storage::Local(LocalStorage::new(o)?)),
            StorageKind::S3(o) => Ok(Storage::S3(S3Storage::new(client, o))),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Storage::Local(_) => "local",
            Storage::S3(_) => "s3",
        }
    }

    pub async fn find(&self, query: &StorageQuery) -> Result<StoragePage> {
        match self {
            Storage::Local(s) => s.find(query).await,
            Storage::S3(s) => s.find(query).await,
        }
    }

    /// `get(key)` — pass `{ "key": "..." }` for the body-based REST form.
    pub async fn get(&self, key: &Value) -> Result<Value> {
        match self {
            Storage::Local(s) => Ok(s.get(&local::key_from_value(key)?).await?.to_value_local()),
            Storage::S3(s) => Ok(s.get(key).await?.to_value_s3_head()),
        }
    }

    /// `create(input)`: local uploads the bytes; S3 answers with a presigned PUT.
    pub async fn create(&self, input: &Value, params: &crate::Params) -> Result<Value> {
        match self {
            Storage::Local(s) => {
                let up = local::upload_input_from_value(input)?;
                Ok(s.upload(up, params).await?.to_value())
            }
            Storage::S3(s) => Ok(s.presign_upload(input).await?.to_value_upload()),
        }
    }

    /// `remove(key)` — a string or `{ key }`.
    pub async fn remove(&self, key: &Value) -> Result<Value> {
        match self {
            Storage::Local(s) => Ok(s.remove(&local::key_from_value(key)?).await?.into()),
            Storage::S3(s) => Ok(s.remove(key).await?.into()),
        }
    }

    pub async fn presign_upload(&self, input: &Value, params: &crate::Params) -> Result<Presigned> {
        match self {
            Storage::Local(s) => s.presign_upload(input, params),
            Storage::S3(s) => s.presign_upload(input).await,
        }
    }

    pub async fn presign_download(
        &self,
        input: &Value,
        params: &crate::Params,
    ) -> Result<Presigned> {
        match self {
            Storage::Local(s) => s.presign_download(input, params),
            Storage::S3(s) => s.presign_download(input).await,
        }
    }
}

/// Percent-encode a path segment like `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Encode like `URLSearchParams`: space is `+`, only `*-._` and alphanumerics
/// stay literal.
pub fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'*' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
