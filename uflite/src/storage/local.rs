//! `services/local-storage.js` — files under a base directory, exposed as a
//! service.
//!
//! Keys are relative, forward-slash paths (`avatars/ada.png`). Every path
//! component is checked with `lstat` so a key can neither escape the base
//! directory nor traverse a symlink, and files are opened with `O_NOFOLLOW`
//! where the platform has it. Bodies are limited by `maxFileSize`; content
//! types live in memory (the filesystem does not store them).
//!
//! Listing is cursor-paginated over sorted keys: `nextCursor` is the last key
//! of the page when more remain.

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use mosql::Value;

use super::{
    check_limit, encode_component, form_encode, ObjectInfo, Presigned, StoragePage, StorageQuery,
    UploadInput,
};
use crate::error::{Error, Result};
use crate::Params;

pub const DEFAULT_BASE_PATH: &str = "./data/files";
pub const DEFAULT_MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalOptions {
    pub base_path: String,
    pub public_url: Option<String>,
    pub max_file_size: u64,
}

impl Default for LocalOptions {
    fn default() -> Self {
        LocalOptions {
            base_path: DEFAULT_BASE_PATH.to_string(),
            public_url: None,
            max_file_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl LocalOptions {
    pub fn base_path(mut self, p: impl Into<String>) -> Self {
        self.base_path = p.into();
        self
    }

    pub fn public_url(mut self, u: impl Into<String>) -> Self {
        self.public_url = Some(u.into());
        self
    }

    pub fn max_file_size(mut self, n: u64) -> Self {
        self.max_file_size = n;
        self
    }

    /// `{ basePath, publicUrl, maxFileSize }` with the service's own checks.
    pub fn from_value(v: &Value) -> Result<LocalOptions> {
        let mut o = LocalOptions::default();
        match v.get("basePath") {
            None | Some(Value::Null) => {}
            Some(Value::Str(s)) => o.base_path = s.clone(),
            Some(_) => {
                return Err(Error::bad_request(
                    "basePath must be a non-empty directory path",
                ))
            }
        }
        if o.base_path.trim().is_empty() {
            return Err(Error::bad_request(
                "basePath must be a non-empty directory path",
            ));
        }
        match v.get("publicUrl") {
            None | Some(Value::Null) => {}
            Some(x) => o.public_url = Some(x.to_js_string()),
        }
        // `Number.isSafeInteger(maxFileSize)`: only a real number qualifies,
        // so "1024" is rejected like the JavaScript service rejects it.
        if let Some(x) = v.get("maxFileSize") {
            if !x.is_null() {
                let n = match x {
                    Value::Int(n) if *n >= 1 => *n as u64,
                    Value::Float(f)
                        if *f >= 1.0 && f.fract() == 0.0 && *f <= 9_007_199_254_740_991.0 =>
                    {
                        *f as u64
                    }
                    _ => {
                        return Err(Error::bad_request(
                            "maxFileSize must be a positive safe integer",
                        ))
                    }
                };
                o.max_file_size = n;
            }
        }
        Ok(o)
    }
}

#[derive(Debug)]
pub struct LocalStorage {
    root: PathBuf,
    public_url: Option<String>,
    max_file_size: u64,
    content_types: Mutex<HashMap<String, String>>,
}

/// A stored file's metadata: `{ key, size, contentType, lastModified }`.
pub type FileMeta = ObjectInfo;

/// `download()` result: the metadata plus the bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct FileBody {
    pub meta: FileMeta,
    pub body: Vec<u8>,
}

/// `upload()` result: `{ key, size, contentType, url }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Uploaded {
    pub key: String,
    pub size: u64,
    pub content_type: String,
    pub url: Option<String>,
}

impl Uploaded {
    pub fn to_value(&self) -> Value {
        let mut o = mosql::Object::new();
        o.set("key", Value::str(&self.key));
        o.set("size", Value::Int(self.size as i64));
        o.set("contentType", Value::str(&self.content_type));
        o.set(
            "url",
            self.url.clone().map(Value::Str).unwrap_or(Value::Null),
        );
        Value::Object(o)
    }
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

fn bad_key() -> Error {
    Error::bad_request("key must be a relative file key using forward slashes")
}

/// `requireKey`: a non-empty relative path with forward slashes and no
/// empty, `.` or `..` segments.
pub fn require_key(key: &str) -> Result<&str> {
    if key.is_empty() || key.contains('\\') || key.contains('\0') || key.starts_with('/') {
        return Err(bad_key());
    }
    if key
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(Error::bad_request(
            "key must not contain empty, dot, or parent path segments",
        ));
    }
    Ok(key)
}

pub(crate) fn require_key_value(v: &Value) -> Result<String> {
    let Some(s) = v.as_str() else {
        return Err(bad_key());
    };
    require_key(s).map(|s| s.to_string())
}

/// `keyOrInput`: a string, or `{ key }` — as `get`, `download` and `remove`
/// accept it.
pub(crate) fn key_from_value(v: &Value) -> Result<String> {
    match v {
        Value::Str(_) => require_key_value(v),
        _ => require_key_value(v.get("key").unwrap_or(&Value::Null)),
    }
}

fn key_from(key: &str) -> Result<String> {
    require_key(key).map(|s| s.to_string())
}

/// `input.body ?? input.data` as bytes: a UTF-8 string, or a JSON array of bytes.
fn body_from(input: &Value) -> Result<Vec<u8>> {
    match input.get("body").or_else(|| input.get("data")) {
        None | Some(Value::Null) => Err(Error::bad_request(
            "body must be a string, Blob, ArrayBuffer, or byte array",
        )),
        Some(Value::Str(s)) => Ok(s.as_bytes().to_vec()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| {
                v.to_number()
                    .filter(|n| n.is_finite())
                    .map(|n| n as u8)
                    .ok_or_else(|| {
                        Error::bad_request(
                            "body must be a string, Blob, ArrayBuffer, or byte array",
                        )
                    })
            })
            .collect(),
        Some(_) => Err(Error::bad_request(
            "body must be a string, Blob, ArrayBuffer, or byte array",
        )),
    }
}

/// `upload({ key, body|data, contentType })` from JSON (bytes as an array).
pub fn upload_input_from_value(input: &Value) -> Result<UploadInput> {
    if !input.is_object() {
        return Err(Error::bad_request("upload input must be an object"));
    }
    Ok(UploadInput {
        key: require_key_value(input.get("key").unwrap_or(&Value::Null))?,
        body: body_from(input)?,
        content_type: match input.get("contentType") {
            None | Some(Value::Null) => None,
            Some(Value::Str(s)) => Some(s.clone()),
            Some(_) => {
                return Err(Error::bad_request(
                    "contentType must be a valid non-empty header value",
                ))
            }
        },
    })
}

fn check_content_type(ct: &str) -> Result<()> {
    if ct.trim().is_empty() || ct.contains('\r') || ct.contains('\n') {
        return Err(Error::bad_request(
            "contentType must be a valid non-empty header value",
        ));
    }
    Ok(())
}

fn not_found(key: &str) -> Error {
    Error::not_found(format!("No file found for key '{key}'"))
}

fn io_error(e: io::Error) -> Error {
    Error::Internal(e.to_string())
}

/// `new URL(path, base)` with `?key=` appended, or `None` without a base.
fn url_for(path: &str, key: &str, base: Option<&str>) -> Option<String> {
    let base = base?;
    // Only the origin survives `new URL("/absolute/path", base)`.
    let (scheme, rest) = base.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    Some(format!(
        "{scheme}://{authority}{path}?key={}",
        form_encode(key)
    ))
}

/// ISO-8601 like `Date.toISOString()` for a UNIX timestamp in milliseconds.
pub fn iso_from_unix_millis(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = mosql::memory::civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        (rem % 3_600_000) / 60_000,
        (rem % 60_000) / 1_000,
        rem % 1_000
    )
}

fn system_time_to_millis(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_millis() as i64,
        Err(e) => -(e.duration().as_millis() as i64),
    }
}

/// Lexically resolve `base` against the current directory, like `path.resolve`:
/// no symlink resolution, `.` and `..` folded.
fn resolve(base: &str) -> Result<PathBuf> {
    let path = PathBuf::from(base);
    if path.is_absolute() {
        return Ok(normalize(path));
    }
    let cwd = std::env::current_dir().map_err(io_error)?;
    Ok(normalize(cwd.join(path)))
}

fn normalize(p: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(unix)]
fn no_follow_flag() -> i32 {
    // O_NOFOLLOW where the value is known; 0 elsewhere, like the JavaScript
    // `constants.O_NOFOLLOW || 0`.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    return 0o400000;
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos"
    ))]
    return 0x0000_0100;
    #[cfg(any(target_os = "freebsd", target_os = "dragonfly"))]
    return 0x0000_0200;
    #[cfg(not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "freebsd",
        target_os = "dragonfly"
    )))]
    0
}

impl LocalStorage {
    /// `createLocalStorageService(options)`, validated.
    pub fn new(options: LocalOptions) -> Result<LocalStorage> {
        if options.base_path.trim().is_empty() {
            return Err(Error::bad_request(
                "basePath must be a non-empty directory path",
            ));
        }
        if options.max_file_size == 0 {
            return Err(Error::bad_request(
                "maxFileSize must be a positive safe integer",
            ));
        }
        let root = resolve(&options.base_path)?;
        std::fs::create_dir_all(&root).map_err(io_error)?;
        Ok(LocalStorage {
            root,
            public_url: options.public_url,
            max_file_size: options.max_file_size,
            content_types: Mutex::new(HashMap::new()),
        })
    }

    /// A service rooted at a fresh temporary directory (tests, examples).
    pub fn temp() -> Result<LocalStorage> {
        let dir = std::env::temp_dir().join(format!(
            "uflite-storage-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        LocalStorage::new(LocalOptions::default().base_path(dir.to_string_lossy().into_owned()))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn max_file_size(&self) -> u64 {
        self.max_file_size
    }

    pub fn type_name(&self) -> &'static str {
        "local"
    }

    /// Resolve a validated key to its path, creating parent directories, and
    /// refusing to traverse anything that is not a directory.
    fn path_for(&self, key: &str) -> Result<PathBuf> {
        require_key(key)?;
        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        let segments: Vec<&str> = key.split('/').collect();
        let mut parent = self.root.clone();
        for seg in &segments[..segments.len() - 1] {
            let next = parent.join(seg);
            match std::fs::symlink_metadata(&next) {
                Ok(md) => {
                    if !md.is_dir() {
                        return Err(Error::bad_request(
                            "key resolves through a non-directory path",
                        ));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => match std::fs::create_dir(&next) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(io_error(e)),
                },
                Err(e) => return Err(io_error(e)),
            }
            parent = next;
        }
        let file = parent.join(segments[segments.len() - 1]);
        match std::fs::symlink_metadata(&file) {
            Ok(md) if !md.is_file() => {
                return Err(Error::bad_request("key must refer to a regular file"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
        Ok(file)
    }

    fn content_type(&self, key: &str) -> String {
        self.content_types
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
            .unwrap_or_else(|| "application/octet-stream".to_string())
    }

    fn meta_for(&self, key: &str, size: u64, mtime: SystemTime) -> FileMeta {
        ObjectInfo {
            key: key.to_string(),
            size: Some(size as i64),
            content_type: Some(self.content_type(key)),
            etag: None,
            last_modified: Some(iso_from_unix_millis(system_time_to_millis(mtime))),
            metadata: None,
        }
    }

    /// `metadata(key)`.
    pub async fn get(&self, key: &str) -> Result<FileMeta> {
        let key = key_from(key)?;
        let path = self.path_for(&key)?;
        let md = match std::fs::symlink_metadata(&path) {
            Ok(md) => md,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_found(&key)),
            Err(e) => return Err(io_error(e)),
        };
        if !md.is_file() {
            return Err(Error::bad_request("key must refer to a regular file"));
        }
        Ok(self.meta_for(&key, md.len(), md.modified().unwrap_or(UNIX_EPOCH)))
    }

    /// `upload({ key, body, contentType }, params)`.
    pub async fn upload(&self, input: UploadInput, params: &Params) -> Result<Uploaded> {
        let key = require_key(&input.key)?.to_string();
        if input.body.len() as u64 > self.max_file_size {
            return Err(Error::TooLarge(format!(
                "file exceeds maxFileSize of {} bytes",
                self.max_file_size
            )));
        }
        let content_type = input
            .content_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());
        check_content_type(&content_type)?;
        let path = self.path_for(&key)?;

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600).custom_flags(no_follow_flag());
        }
        let mut f = opts.open(&path).map_err(io_error)?;
        use std::io::Write;
        f.write_all(&input.body).map_err(io_error)?;
        drop(f);

        self.content_types
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(key.clone(), content_type.clone());

        let url = url_for("/storage/download", &key, params.base_url.as_deref());
        Ok(Uploaded {
            key,
            size: input.body.len() as u64,
            content_type,
            url,
        })
    }

    /// `download(key)` — bytes plus metadata.
    pub async fn download(&self, key: &str) -> Result<FileBody> {
        let key = key_from(key)?;
        let path = self.path_for(&key)?;
        let mut opts = std::fs::OpenOptions::new();
        opts.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(no_follow_flag());
        }
        let mut f = match opts.open(&path) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_found(&key)),
            Err(e) => return Err(io_error(e)),
        };
        let mut body = Vec::new();
        use std::io::Read;
        f.read_to_end(&mut body).map_err(io_error)?;
        let md = f.metadata().map_err(io_error)?;
        Ok(FileBody {
            meta: self.meta_for(&key, md.len(), md.modified().unwrap_or(UNIX_EPOCH)),
            body,
        })
    }

    /// `remove(key)` — idempotent for a missing file.
    pub async fn remove(&self, key: &str) -> Result<super::Deleted> {
        let key = key_from(key)?;
        let path = self.path_for(&key)?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(e)),
        }
        self.content_types
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&key);
        Ok(super::Deleted { key })
    }

    /// `find({ prefix, limit, cursor })` — sorted keys, cursor pagination.
    pub async fn find(&self, query: &StorageQuery) -> Result<StoragePage> {
        // `typeof prefix !== 'string' || (prefix && requireKey(prefix.replace(/\/$/, '')) !== …)`
        // — an invalid prefix throws requireKey's own message.
        let prefix: String = match &query.prefix {
            None => String::new(),
            Some(Value::Str(s)) => {
                let trimmed = s.strip_suffix('/').unwrap_or(s);
                if !s.is_empty() && require_key(trimmed)? != trimmed {
                    return Err(Error::bad_request("prefix must be a relative file prefix"));
                }
                s.clone()
            }
            Some(_) => return Err(Error::bad_request("prefix must be a relative file prefix")),
        };
        // `Number.isSafeInteger(limit) && limit >= 1 && limit <= 1000`.
        let limit = check_limit(query.limit.as_ref())?;
        if let Some(c) = &query.cursor {
            if !matches!(c, Value::Str(_)) {
                return Err(Error::bad_request("cursor must be a string"));
            }
        }

        std::fs::create_dir_all(&self.root).map_err(io_error)?;
        let mut keys = Vec::new();
        self.walk(&self.root.clone(), "", &prefix, &mut keys)?;
        keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));

        let infos: Vec<ObjectInfo> = {
            let mut v = Vec::with_capacity(keys.len());
            for k in keys {
                let path = self.root.join(&k);
                let md = std::fs::symlink_metadata(&path).map_err(io_error)?;
                v.push(self.meta_for(&k, md.len(), md.modified().unwrap_or(UNIX_EPOCH)));
            }
            v
        };
        let cursor = match &query.cursor {
            Some(Value::Str(s)) if !s.is_empty() => Some(s.as_str()),
            _ => None,
        };
        Ok(StoragePage::from_sorted(infos, cursor, limit))
    }

    fn walk(&self, dir: &Path, relative: &str, prefix: &str, out: &mut Vec<String>) -> Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(io_error)?
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(io_error)?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let key = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            if ft.is_dir() {
                self.walk(&entry.path(), &key, prefix, out)?;
            } else if ft.is_file() && key.starts_with(prefix) {
                out.push(key);
            }
        }
        Ok(())
    }

    /// `presignUpload({ key, contentType }, params)` — for a local service this
    /// is the URL of the plain PUT route, or `null` without a request URL.
    pub fn presign_upload(&self, input: &Value, params: &Params) -> Result<Presigned> {
        let key = require_key_value(input.get("key").unwrap_or(&Value::Null))?;
        // `input.contentType || default`: a falsy value falls back; a truthy
        // non-string then fails the header check, as `typeof !== 'string'` does.
        let content_type = match input.get("contentType") {
            Some(v) if v.is_truthy() => match v {
                Value::Str(s) => s.clone(),
                _ => {
                    return Err(Error::bad_request(
                        "contentType must be a valid non-empty header value",
                    ))
                }
            },
            _ => "application/octet-stream".to_string(),
        };
        check_content_type(&content_type)?;
        let mut headers = mosql::Object::new();
        headers.set("Content-Type", Value::str(&content_type));
        Ok(Presigned {
            url: url_for("/storage", &key, params.base_url.as_deref()),
            key,
            method: "PUT",
            headers: Value::Object(headers),
            expires_in: None,
        })
    }

    /// `presignDownload({ key }, params)` — a `publicUrl` wins over the request URL.
    pub fn presign_download(&self, input: &Value, params: &Params) -> Result<Presigned> {
        let key = require_key_value(input.get("key").unwrap_or(&Value::Null))?;
        let url = match &self.public_url {
            Some(base) => {
                let base = base.trim_end_matches('/');
                let encoded = key
                    .split('/')
                    .map(encode_component)
                    .collect::<Vec<_>>()
                    .join("/");
                Some(format!("{base}/{encoded}"))
            }
            None => url_for("/storage/download", &key, params.base_url.as_deref()),
        };
        Ok(Presigned {
            key,
            url,
            method: "GET",
            headers: Value::object(),
            expires_in: None,
        })
    }
}
