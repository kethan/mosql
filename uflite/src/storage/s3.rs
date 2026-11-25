//! `services/storage.js` — the S3 backend.
//!
//! The service owns every rule that is not the wire protocol: configuration
//! validation, key checks, the `1..=1000` limit, expiry bounds and the shape
//! of each result. What is left is an [`S3Client`] — four calls mirroring the
//! AWS SDK's `HeadObject`, `DeleteObject`, `ListObjectsV2` and
//! `getSignedUrl` — which you implement over the AWS SDK, a presigning proxy,
//! or a test double.
//!
//! No SigV4 or TLS lives in this crate; a real client brings its own. Errors
//! from the client are propagated untouched, exactly as storage.js does.

use core::future::Future;

use mosql::{Object, Value};

use super::{check_limit, ObjectInfo, Presigned, StoragePage, StorageQuery};
use crate::error::{Error, Result};

pub const DEFAULT_EXPIRES_IN: i64 = 900;
pub const MAX_EXPIRES_IN: i64 = 60 * 60 * 24 * 7;

// ---------------------------------------------------------------------------
// Client contract
// ---------------------------------------------------------------------------

/// `HeadObjectCommand` / `ListObjectsV2Command` item fields. `key` is left
/// empty by the client for a `HeadObject`; the service fills it in.
pub type HeadObject = ObjectInfo;

/// `ListObjectsV2Command` result.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListResult {
    /// `Contents` — every item S3 returned for this page.
    pub contents: Vec<ObjectInfo>,
    pub is_truncated: bool,
    /// `NextContinuationToken`.
    pub next_token: Option<String>,
}

/// Which command to sign. `Put` with a content type also signs the
/// `content-type` header (`signableHeaders: Set(['content-type'])`).
#[derive(Debug, Clone, PartialEq)]
pub enum PresignCommand<'a> {
    Put {
        bucket: &'a str,
        key: &'a str,
        content_type: Option<&'a str>,
    },
    Get {
        bucket: &'a str,
        key: &'a str,
    },
}

impl PresignCommand<'_> {
    pub fn bucket(&self) -> &str {
        match self {
            PresignCommand::Put { bucket, .. } | PresignCommand::Get { bucket, .. } => bucket,
        }
    }

    pub fn key(&self) -> &str {
        match self {
            PresignCommand::Put { key, .. } | PresignCommand::Get { key, .. } => key,
        }
    }
}

/// The wire-protocol surface the service needs, over your AWS SDK client.
pub trait S3Client: Send + Sync {
    /// `HeadObjectCommand` → `{ Size, ContentType, ETag, LastModified, Metadata }`.
    fn head_object<'a>(
        &'a self,
        bucket: &'a str,
        key: &'a str,
    ) -> impl Future<Output = Result<HeadObject>> + Send + 'a;

    /// `DeleteObjectCommand` — idempotent in S3; the service never checks
    /// for a missing key first.
    fn delete_object<'a>(
        &'a self,
        bucket: &'a str,
        key: &'a str,
    ) -> impl Future<Output = Result<()>> + Send + 'a;

    /// `ListObjectsV2Command` with `Prefix`, `MaxKeys` and `ContinuationToken`.
    fn list_objects<'a>(
        &'a self,
        bucket: &'a str,
        prefix: Option<&'a str>,
        max_keys: i64,
        continuation_token: Option<&'a str>,
    ) -> impl Future<Output = Result<ListResult>> + Send + 'a;

    /// `getSignedUrl(client, command, { expiresIn, signableHeaders? })`.
    fn presign<'a>(
        &'a self,
        command: PresignCommand<'a>,
        expires_in: i64,
    ) -> impl Future<Output = Result<String>> + Send + 'a;
}

/// A shared client is still a client: `S3Storage::new(&client, …)` and
/// `S3Storage::new(Arc::new(client), …)` both work.
impl<T: S3Client + ?Sized> S3Client for &T {
    async fn head_object(&self, bucket: &str, key: &str) -> Result<HeadObject> {
        (**self).head_object(bucket, key).await
    }
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        (**self).delete_object(bucket, key).await
    }
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        max_keys: i64,
        continuation_token: Option<&str>,
    ) -> Result<ListResult> {
        (**self)
            .list_objects(bucket, prefix, max_keys, continuation_token)
            .await
    }
    async fn presign(&self, command: PresignCommand<'_>, expires_in: i64) -> Result<String> {
        (**self).presign(command, expires_in).await
    }
}

impl<T: S3Client + ?Sized> S3Client for std::sync::Arc<T> {
    async fn head_object(&self, bucket: &str, key: &str) -> Result<HeadObject> {
        (**self).head_object(bucket, key).await
    }
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        (**self).delete_object(bucket, key).await
    }
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        max_keys: i64,
        continuation_token: Option<&str>,
    ) -> Result<ListResult> {
        (**self)
            .list_objects(bucket, prefix, max_keys, continuation_token)
            .await
    }
    async fn presign(&self, command: PresignCommand<'_>, expires_in: i64) -> Result<String> {
        (**self).presign(command, expires_in).await
    }
}

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// The `createS3StorageService` options, validated.
#[derive(Debug, Clone, PartialEq)]
pub struct S3Options {
    pub bucket: String,
    pub region: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub endpoint: Option<String>,
    /// Defaults to `Boolean(endpoint)`.
    pub force_path_style: bool,
    pub upload_expires_in: i64,
    pub download_expires_in: i64,
}

impl S3Options {
    pub fn new(
        bucket: impl Into<String>,
        region: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Self {
        S3Options {
            bucket: bucket.into(),
            region: region.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            session_token: None,
            endpoint: None,
            force_path_style: false,
            upload_expires_in: DEFAULT_EXPIRES_IN,
            download_expires_in: DEFAULT_EXPIRES_IN,
        }
    }

    pub fn endpoint(mut self, e: impl Into<String>) -> Self {
        self.force_path_style = true;
        self.endpoint = Some(e.into());
        self
    }

    /// `{ bucket, region, accessKeyId, secretAccessKey, sessionToken?, endpoint?,
    /// forcePathStyle?, uploadExpiresIn?, downloadExpiresIn? }`, in the same
    /// order and with the same messages as storage.js.
    pub fn from_value(v: &Value) -> Result<S3Options> {
        let required = |name: &str, trim: bool| -> Result<String> {
            match v.get(name) {
                Some(Value::Str(s)) if !s.is_empty() && (!trim || !s.trim().is_empty()) => {
                    Ok(s.clone())
                }
                _ => Err(Error::bad_request(format!(
                    "[createStorageService] '{name}' is required"
                ))),
            }
        };
        let mut o = S3Options {
            bucket: required("bucket", true)?,
            region: required("region", true)?,
            access_key_id: required("accessKeyId", false)?,
            secret_access_key: required("secretAccessKey", false)?,
            session_token: None,
            endpoint: None,
            force_path_style: false,
            upload_expires_in: DEFAULT_EXPIRES_IN,
            download_expires_in: DEFAULT_EXPIRES_IN,
        };
        o.session_token = match v.get("sessionToken") {
            Some(s) if s.is_truthy() => Some(s.to_js_string()),
            _ => None,
        };
        o.endpoint = match v.get("endpoint") {
            Some(e) if e.is_truthy() => {
                o.force_path_style = true;
                Some(e.to_js_string())
            }
            _ => None,
        };
        if let Some(f) = v.get("forcePathStyle") {
            o.force_path_style = f.is_truthy();
        }
        o.upload_expires_in = require_expires_in(v.get("uploadExpiresIn"), "uploadExpiresIn")?;
        o.download_expires_in =
            require_expires_in(v.get("downloadExpiresIn"), "downloadExpiresIn")?;
        Ok(o)
    }
}

/// `requireExpiresIn(value, DEFAULT, label)`: `undefined` → the default;
/// otherwise `Number(value)` must be an integer in `1..=604800`.
fn require_expires_in(value: Option<&Value>, label: &str) -> Result<i64> {
    let Some(v) = value else {
        return Ok(DEFAULT_EXPIRES_IN);
    };
    let n = v.to_number().unwrap_or(f64::NAN);
    if n.is_finite() && n.fract() == 0.0 && (1.0..=MAX_EXPIRES_IN as f64).contains(&n) {
        Ok(n as i64)
    } else {
        Err(Error::bad_request(format!(
            "{label} must be an integer between 1 and {MAX_EXPIRES_IN} seconds"
        )))
    }
}

/// `requireKey`: a non-empty string.
fn require_key(v: &Value) -> Result<String> {
    match v {
        Value::Str(s) if !s.is_empty() => Ok(s.clone()),
        _ => Err(Error::bad_request("key must be a non-empty string")),
    }
}

/// `keyFrom(value)`: the string, or `value.key`.
fn key_from(v: &Value) -> Result<String> {
    match v {
        Value::Str(_) => require_key(v),
        _ => require_key(v.get("key").unwrap_or(&Value::Null)),
    }
}

fn is_object(v: &Value) -> bool {
    matches!(v, Value::Object(_))
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// `createS3StorageService(options, client)`.
#[derive(Debug)]
pub struct S3Storage<C: S3Client> {
    client: C,
    options: S3Options,
}

impl<C: S3Client> S3Storage<C> {
    pub fn new(client: C, options: S3Options) -> Self {
        S3Storage { client, options }
    }

    pub fn client(&self) -> &C {
        &self.client
    }

    pub fn options(&self) -> &S3Options {
        &self.options
    }

    pub fn type_name(&self) -> &'static str {
        "s3"
    }

    /// `find({ prefix?, limit?, cursor? })` — the page is S3's: `MaxKeys` is
    /// the limit and `nextCursor` is S3's continuation token.
    pub async fn find(&self, query: &StorageQuery) -> Result<StoragePage> {
        let prefix = match &query.prefix {
            None => None,
            Some(Value::Str(s)) => Some(s.as_str()),
            Some(_) => return Err(Error::bad_request("prefix must be a string")),
        };
        let cursor = match &query.cursor {
            None => None,
            Some(Value::Str(s)) if !s.is_empty() => Some(s.as_str()),
            Some(Value::Str(_)) => None,
            Some(_) => return Err(Error::bad_request("cursor must be a string")),
        };
        let limit = check_limit(query.limit.as_ref())?;
        let result = self
            .client
            .list_objects(&self.options.bucket, prefix, limit, cursor)
            .await?;
        let next_cursor = if result.is_truncated {
            result.next_token
        } else {
            None
        };
        Ok(StoragePage {
            data: result.contents,
            next_cursor,
        })
    }

    /// `get(key)` — `HeadObject`.
    pub async fn get(&self, key: &Value) -> Result<HeadObject> {
        let key = key_from(key)?;
        let mut head = self.client.head_object(&self.options.bucket, &key).await?;
        head.key = key;
        Ok(head)
    }

    /// `create(data)` is `presignUpload(data)` — a direct-to-S3 PUT grant.
    pub async fn create(&self, input: &Value) -> Result<Presigned> {
        self.presign_upload(input).await
    }

    /// `remove(key)` — `DeleteObject`; always `{ key, deleted: true }`.
    pub async fn remove(&self, key: &Value) -> Result<super::Deleted> {
        let key = key_from(key)?;
        self.client
            .delete_object(&self.options.bucket, &key)
            .await?;
        Ok(super::Deleted { key })
    }

    /// `presignUpload({ key, contentType?, expiresIn? })`.
    pub async fn presign_upload(&self, input: &Value) -> Result<Presigned> {
        if !is_object(input) {
            return Err(Error::bad_request("upload input must be an object"));
        }
        let key = require_key(input.get("key").unwrap_or(&Value::Null))?;
        if let Some(ct) = input.get("contentType") {
            match ct {
                Value::Str(s) if !s.trim().is_empty() => {}
                _ => return Err(Error::bad_request("contentType must be a non-empty string")),
            }
        }
        if input.get("contentLength").is_some() {
            return Err(Error::Internal(
                "contentLength cannot be enforced with a presigned PUT URL".into(),
            ));
        }
        let expires_in = match input.get("expiresIn") {
            None => self.options.upload_expires_in,
            Some(v) => require_expires_in(Some(v), "expiresIn")?,
        };
        let content_type = match input.get("contentType") {
            Some(Value::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let url = self
            .client
            .presign(
                PresignCommand::Put {
                    bucket: &self.options.bucket,
                    key: &key,
                    content_type,
                },
                expires_in,
            )
            .await?;
        let mut headers = Object::new();
        if let Some(ct) = content_type {
            headers.set("Content-Type", Value::str(ct));
        }
        Ok(Presigned {
            key,
            url: Some(url),
            method: "PUT",
            headers: Value::Object(headers),
            expires_in: Some(expires_in),
        })
    }

    /// `presignDownload({ key, expiresIn? })`.
    pub async fn presign_download(&self, input: &Value) -> Result<Presigned> {
        if !is_object(input) {
            return Err(Error::bad_request("download input must be an object"));
        }
        let key = require_key(input.get("key").unwrap_or(&Value::Null))?;
        let expires_in = match input.get("expiresIn") {
            None => self.options.download_expires_in,
            Some(v) => require_expires_in(Some(v), "expiresIn")?,
        };
        let url = self
            .client
            .presign(
                PresignCommand::Get {
                    bucket: &self.options.bucket,
                    key: &key,
                },
                expires_in,
            )
            .await?;
        Ok(Presigned {
            key,
            url: Some(url),
            method: "GET",
            headers: Value::object(),
            expires_in: Some(expires_in),
        })
    }
}
