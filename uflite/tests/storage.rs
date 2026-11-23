//! Port of uflite's `tests/storage.spec.js`: the local filesystem backend in
//! full, the S3 backend over a recording fake of the four `S3Client` calls
//! (standing in for the spec's `mockClient()`), and the `createStorageService`
//! dispatcher. REST routes and hook wiring belong to the host framework.

mod common;

use std::sync::Mutex;

use common::{docs, expect_err, expect_json, expect_match, json, run, TempStorage};
use mosql::Value;
use uflite::storage::{
    encode_component, form_encode, HeadObject, ListResult, LocalOptions, LocalStorage, ObjectInfo,
    PresignCommand, S3Client, S3Options, S3Storage, Storage, StorageKind, StoragePage,
    StorageQuery, UploadInput, DEFAULT_LIMIT, MAX_LIMIT,
};
use uflite::{Error, Params, Result};

fn local(options: LocalOptions) -> TempStorage {
    TempStorage::new(options)
}

fn upload(service: &LocalStorage, key: &str, body: &str) -> uflite::storage::Uploaded {
    run(service.upload(
        UploadInput {
            key: key.into(),
            body: body.as_bytes().to_vec(),
            content_type: None,
        },
        &Params::new(),
    ))
    .unwrap()
}

// -- local: defaults and configuration ---------------------------------------------

#[test]
fn defaults_to_local_storage_and_validates_provider_options() {
    let opts = json("{}");
    assert!(matches!(
        StorageKind::from_value(&opts).unwrap(),
        StorageKind::Local(_)
    ));
    assert!(matches!(
        StorageKind::from_value(&json(r#"{"type": "local", "basePath": "./files"}"#)).unwrap(),
        StorageKind::Local(_)
    ));
    expect_err(
        StorageKind::from_value(&json(r#"{"type": "unknown"}"#)),
        "unsupported storage type 'unknown'",
    );
    expect_err(
        StorageKind::from_value(&json(r#"{"type": "local", "maxFileSize": 0}"#)),
        "maxFileSize",
    );
    // `!options || typeof options !== 'object' || Array.isArray(options)`
    expect_err(
        StorageKind::from_value(&Value::Null),
        "storage options must be an object",
    );
    expect_err(
        StorageKind::from_value(&json("[]")),
        "storage options must be an object",
    );
    expect_err(
        StorageKind::from_value(&json("5")),
        "storage options must be an object",
    );
    // Any S3 key selects S3 even without `type`.
    assert!(matches!(
        StorageKind::from_value(&json(
            r#"{"bucket": "b", "region": "r", "accessKeyId": "a", "secretAccessKey": "s"}"#
        ))
        .unwrap(),
        StorageKind::S3(_)
    ));
    assert_eq!(
        StorageKind::from_value(&json("{}")).unwrap().type_name(),
        "local"
    );

    let service = LocalStorage::new(LocalOptions::default()).unwrap();
    assert_eq!(service.type_name(), "local");
    assert_eq!(service.max_file_size(), 25 * 1024 * 1024);
    assert!(service.root().ends_with("data/files"));
}

#[test]
fn local_configuration_messages() {
    expect_err(
        LocalOptions::from_value(&json(r#"{"basePath": ""}"#)),
        "basePath must be a non-empty directory path",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"basePath": "   "}"#)),
        "basePath must be a non-empty directory path",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"basePath": 5}"#)),
        "basePath must be a non-empty directory path",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"maxFileSize": 0}"#)),
        "maxFileSize must be a positive safe integer",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"maxFileSize": -1}"#)),
        "maxFileSize must be a positive safe integer",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"maxFileSize": 1.5}"#)),
        "maxFileSize must be a positive safe integer",
    );
    expect_err(
        LocalOptions::from_value(&json(r#"{"maxFileSize": "10"}"#)),
        "maxFileSize must be a positive safe integer",
    );
    let o = LocalOptions::from_value(&json(
        r#"{"maxFileSize": 1024, "publicUrl": "https://x.test/"}"#,
    ))
    .unwrap();
    assert_eq!(o.max_file_size, 1024);
    assert_eq!(o.public_url.as_deref(), Some("https://x.test/"));
    let e = LocalStorage::new(LocalOptions::default().max_file_size(0))
        .err()
        .unwrap();
    assert_eq!(e.status(), 400);
}

// -- local: bytes, metadata, unsafe keys, limits -----------------------------------

#[test]
fn writes_and_reads_bytes_reports_metadata_and_blocks_unsafe_keys() {
    run(async {
        let tmp = local(LocalOptions::default().max_file_size(16));
        let service = tmp.get();
        let uploaded = service
            .upload(
                UploadInput {
                    key: "users/u1/avatar.txt".into(),
                    body: b"avatar bytes".to_vec(),
                    content_type: Some("text/plain".into()),
                },
                &Params::new(),
            )
            .await
            .unwrap();
        assert_eq!(uploaded.key, "users/u1/avatar.txt");
        assert_eq!(uploaded.size, 12);
        assert_eq!(uploaded.content_type, "text/plain");
        assert_eq!(uploaded.url, None);

        let meta = service.get("users/u1/avatar.txt").await.unwrap();
        assert_eq!(meta.key, uploaded.key);
        assert_eq!(meta.size, Some(12));
        assert_eq!(meta.content_type.as_deref(), Some("text/plain"));
        assert!(meta.last_modified.as_deref().unwrap().ends_with('Z'));
        expect_match(
            &meta.to_value_local(),
            r#"{"key": "users/u1/avatar.txt", "size": 12, "contentType": "text/plain"}"#,
        );

        let file = service.download("users/u1/avatar.txt").await.unwrap();
        assert_eq!(file.body, b"avatar bytes");
        assert_eq!(file.meta.content_type.as_deref(), Some("text/plain"));

        for key in [
            "../escape.txt",
            "/absolute.txt",
            "folder/../escape.txt",
            "folder\\escape.txt",
            "folder//file",
            "",
            "a/./b",
            "a\0b",
        ] {
            let e = service
                .upload(
                    UploadInput {
                        key: key.into(),
                        body: b"no".to_vec(),
                        content_type: None,
                    },
                    &Params::new(),
                )
                .await
                .expect_err(key);
            assert_eq!(e.status(), 400, "{key:?}");
        }
        // `key must not contain empty, dot, or parent path segments`
        expect_err(
            service.remove("a//b").await,
            "key must not contain empty, dot, or parent path segments",
        );
        expect_err(
            service.get("/abs.txt").await,
            "key must be a relative file key using forward slashes",
        );
        expect_err(
            service.get("../x").await,
            "key must not contain empty, dot, or parent path segments",
        );

        let e = expect_err(
            service
                .upload(
                    UploadInput {
                        key: "large.bin".into(),
                        body: vec![b'x'; 17],
                        content_type: None,
                    },
                    &Params::new(),
                )
                .await,
            "file exceeds maxFileSize of 16 bytes",
        );
        assert_eq!(e.status(), 413);
        assert_eq!(e.name(), "RangeError");
        // Exactly the limit is fine.
        service
            .upload(
                UploadInput {
                    key: "exact.bin".into(),
                    body: vec![b'x'; 16],
                    content_type: None,
                },
                &Params::new(),
            )
            .await
            .unwrap();

        let e = expect_err(
            service.get("missing.txt").await,
            "No file found for key 'missing.txt'",
        );
        assert_eq!(e.status(), 404);
        assert_eq!(e.name(), "NotFound");
        assert_eq!(Value::from(e).get("code").unwrap().as_f64(), Some(404.0));
        let e = expect_err(service.download("missing.txt").await, "No file found");
        assert_eq!(e.status(), 404);
    });
}

#[test]
fn rejects_bad_content_types_and_bodies_before_touching_the_disk() {
    run(async {
        let tmp = local(LocalOptions::default());
        let service = tmp.get();
        for ct in ["", "   ", "text/plain\r\nx"] {
            let e = service
                .upload(
                    UploadInput {
                        key: "a.txt".into(),
                        body: b"x".to_vec(),
                        content_type: Some(ct.into()),
                    },
                    &Params::new(),
                )
                .await
                .expect_err(ct);
            assert_eq!(
                e.to_string(),
                "contentType must be a valid non-empty header value"
            );
        }
        expect_err(
            service.presign_upload(&json(r#"{"key": "a", "contentType": 5}"#), &Params::new()),
            "contentType must be a valid non-empty header value",
        );
        // JSON-level upload validation (`create`).
        expect_err(
            uflite::storage::local::upload_input_from_value(&json("5")),
            "upload input must be an object",
        );
        expect_err(
            uflite::storage::local::upload_input_from_value(&json(r#"{"key": "a"}"#)),
            "body must be a string, Blob, ArrayBuffer, or byte array",
        );
        expect_err(
            uflite::storage::local::upload_input_from_value(&json(r#"{"key": "a", "body": 5}"#)),
            "body must be a string, Blob, ArrayBuffer, or byte array",
        );
        assert!(
            std::fs::read_dir(service.root()).unwrap().next().is_none(),
            "nothing was written"
        );
    });
}

#[test]
fn accepts_byte_arrays_deletes_idempotently_and_builds_download_urls() {
    run(async {
        let tmp = local(LocalOptions::default().public_url("https://files.example.test/public/"));
        let service = tmp.get();
        service
            .upload(
                UploadInput {
                    key: "binary/data.bin".into(),
                    body: vec![0, 1, 255],
                    content_type: None,
                },
                &Params::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            service.download("binary/data.bin").await.unwrap().body,
            vec![0, 1, 255]
        );
        // Default content type when none was given.
        assert_eq!(
            service
                .get("binary/data.bin")
                .await
                .unwrap()
                .content_type
                .as_deref(),
            Some("application/octet-stream")
        );

        let dl = service
            .presign_download(&json(r#"{"key": "binary/data.bin"}"#), &Params::new())
            .unwrap();
        assert_eq!(
            dl.url.as_deref(),
            Some("https://files.example.test/public/binary/data.bin")
        );
        assert_eq!(dl.method, "GET");
        assert_eq!(dl.expires_in, None);
        // Keys are percent-encoded per segment.
        let dl = service
            .presign_download(&json(r#"{"key": "a b/c+d.txt"}"#), &Params::new())
            .unwrap();
        assert_eq!(
            dl.url.as_deref(),
            Some("https://files.example.test/public/a%20b/c%2Bd.txt")
        );

        // `{ key, deleted: true }` — twice.
        let removed = service.remove("binary/data.bin").await.unwrap();
        expect_json(
            &removed.clone().into(),
            r#"{"key": "binary/data.bin", "deleted": true}"#,
        );
        expect_json(
            &service.remove("binary/data.bin").await.unwrap().into(),
            r#"{"key": "binary/data.bin", "deleted": true}"#,
        );
        let e = expect_err(service.download("binary/data.bin").await, "No file found");
        assert_eq!(e.status(), 404);

        // Trailing slashes on publicUrl are stripped.
        let tmp = local(LocalOptions::default().public_url("https://x.test///"));
        let dl = tmp
            .get()
            .presign_download(&json(r#"{"key": "k"}"#), &Params::new())
            .unwrap();
        assert_eq!(dl.url.as_deref(), Some("https://x.test/k"));
    });
}

#[test]
fn lists_sorted_files_with_prefix_filtering_and_stable_cursors() {
    run(async {
        let tmp = local(LocalOptions::default());
        let service = tmp.get();
        for key in [
            "users/b.txt",
            "users/a.txt",
            "public/c.txt",
            "zebra.txt",
            "users/0.txt",
        ] {
            upload(service, key, key);
        }
        // `find({ query: { prefix, limit, cursor } })`
        let query =
            StorageQuery::from_value(&json(r#"{"query": {"prefix": "users/", "limit": 1}}"#));
        let first = service.find(&query).await.unwrap();
        assert_eq!(keys(&first), ["users/0.txt"]);
        assert_eq!(first.next_cursor.as_deref(), Some("users/0.txt"));
        let second = service
            .find(&StorageQuery::from_value(&json(
                r#"{"query": {"prefix": "users/", "limit": 1, "cursor": "users/0.txt"}}"#,
            )))
            .await
            .unwrap();
        assert_eq!(keys(&second), ["users/a.txt"]);
        assert_eq!(second.next_cursor.as_deref(), Some("users/a.txt"));
        let third = service
            .find(&StorageQuery::from_value(&json(
                r#"{"query": {"prefix": "users/", "limit": 1, "cursor": "users/a.txt"}}"#,
            )))
            .await
            .unwrap();
        assert_eq!(keys(&third), ["users/b.txt"]);
        assert_eq!(third.next_cursor, None);
        // A trailing slash on the prefix is allowed and means the same thing.
        let all = service
            .find(&StorageQuery::from_value(&json(r#"{"prefix": "users/"}"#)))
            .await
            .unwrap();
        assert_eq!(all.data.len(), 3);
        assert_eq!(all.next_cursor, None);
        // Direct params (no `query` wrapper) and the default limit of 100.
        let everything = service
            .find(&StorageQuery::from_value(&json("{}")))
            .await
            .unwrap();
        assert_eq!(everything.data.len(), 5);
        // JSON page shape (`lastModified` is the file's mtime).
        let Value::Object(page) = first.to_value_local() else {
            panic!()
        };
        assert_eq!(
            page.get("nextCursor").unwrap().as_str(),
            Some("users/0.txt")
        );
        expect_match(
            page.get("data").unwrap(),
            r#"[{"key": "users/0.txt", "size": 11, "contentType": "application/octet-stream"}]"#,
        );
        let Value::Array(data) = page.get("data").unwrap() else {
            panic!()
        };
        assert!(data[0]
            .get("lastModified")
            .unwrap()
            .as_str()
            .unwrap()
            .ends_with("Z"));
    });
}

fn keys(page: &StoragePage) -> Vec<&str> {
    page.data.iter().map(|d| d.key.as_str()).collect()
}

#[test]
fn find_validates_prefix_limit_and_cursor() {
    run(async {
        let tmp = local(LocalOptions::default());
        let service = tmp.get();
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"prefix": 5}"#)))
                .await,
            "prefix must be a relative file prefix",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"prefix": "../x"}"#)))
                .await,
            "key must not contain empty, dot, or parent path segments",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"prefix": "/abs"}"#)))
                .await,
            "key must be a relative file key",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"limit": 0}"#)))
                .await,
            "limit must be an integer between 1 and 1000",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"limit": 1001}"#)))
                .await,
            "limit must be an integer between 1 and 1000",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"limit": 1.5}"#)))
                .await,
            "limit must be an integer between 1 and 1000",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"limit": "x"}"#)))
                .await,
            "limit must be an integer between 1 and 1000",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"cursor": {}}"#)))
                .await,
            "cursor must be a string",
        );
        // Numeric strings coerce like `Number()`.
        let page = service
            .find(&StorageQuery::from_value(&json(r#"{"limit": "3"}"#)))
            .await
            .unwrap();
        assert_eq!(page.data.len(), 0);
    });
}

#[test]
fn builds_request_urls_from_params() {
    run(async {
        let tmp = local(LocalOptions::default());
        let service = tmp.get();
        let params = Params::new().base_url("http://localhost:3000/files?x=1");
        let uploaded = service
            .upload(
                UploadInput {
                    key: "users/u1/avatar.txt".into(),
                    body: b"ab".to_vec(),
                    content_type: None,
                },
                &params,
            )
            .await
            .unwrap();
        assert_eq!(
            uploaded.url.as_deref(),
            Some("http://localhost:3000/storage/download?key=users%2Fu1%2Favatar.txt")
        );
        let up = service
            .presign_upload(
                &json(r#"{"key": "a b.txt", "contentType": "text/plain"}"#),
                &params,
            )
            .unwrap();
        // `searchParams.set` encodes a space as `+`, like URLSearchParams.
        assert_eq!(
            up.url.as_deref(),
            Some("http://localhost:3000/storage?key=a+b.txt")
        );
        assert_eq!(up.method, "PUT");
        expect_json(&up.headers, r#"{"Content-Type": "text/plain"}"#);
        expect_json(
            &up.to_value_upload(),
            r#"{"key": "a b.txt", "url": "http://localhost:3000/storage?key=a+b.txt", "method": "PUT", "headers": {"Content-Type": "text/plain"}, "expiresIn": null}"#,
        );
        // An empty content type falls back to the default, like `||`.
        let up = service
            .presign_upload(&json(r#"{"key": "k", "contentType": ""}"#), &params)
            .unwrap();
        expect_json(
            &up.headers,
            r#"{"Content-Type": "application/octet-stream"}"#,
        );
        // presignDownload without a publicUrl uses the request URL.
        let dl = service
            .presign_download(&json(r#"{"key": "k"}"#), &params)
            .unwrap();
        assert_eq!(
            dl.url.as_deref(),
            Some("http://localhost:3000/storage/download?key=k")
        );
        assert_eq!(form_encode("a b/c"), "a+b%2Fc");
    });
}

#[cfg(unix)]
#[test]
fn refuses_to_traverse_symlinks_and_non_directories() {
    run(async {
        let tmp = local(LocalOptions::default());
        let service = tmp.get();
        // A directory symlink pointing outside the base directory.
        let outside = std::env::temp_dir().join(format!("uflite-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(&outside, service.root().join("link")).unwrap();

        let e = service
            .get("link/secret.txt")
            .await
            .expect_err("through symlink");
        assert_eq!(e.status(), 400, "{e}");
        let e = service
            .download("link/secret.txt")
            .await
            .expect_err("through symlink");
        assert_eq!(e.status(), 400);
        let e = service
            .upload(
                UploadInput {
                    key: "link/new.txt".into(),
                    body: b"x".to_vec(),
                    content_type: None,
                },
                &Params::new(),
            )
            .await
            .expect_err("through symlink");
        assert_eq!(e.status(), 400);
        assert!(!outside.join("new.txt").exists());

        // The final component being a symlink is refused as "not a regular file".
        std::os::unix::fs::symlink(outside.join("secret.txt"), service.root().join("file-link"))
            .unwrap();
        let e = service
            .download("file-link")
            .await
            .expect_err("file symlink");
        assert_eq!(e.status(), 400);
        assert_eq!(e.to_string(), "key must refer to a regular file");

        // A file where a directory is needed.
        upload(service, "blocker", "x");
        let e = service
            .get("blocker/child.txt")
            .await
            .expect_err("file as dir");
        assert_eq!(e.to_string(), "key resolves through a non-directory path");

        // Symlinked entries are skipped by `find`.
        let page = service.find(&StorageQuery::default()).await.unwrap();
        assert_eq!(keys(&page), ["blocker"]);

        let _ = std::fs::remove_dir_all(&outside);
    });
}

// -- S3: fake client -------------------------------------------------------------

/// Stands in for `mockClient()`: records every command and answers from a
/// script, throwing the configured provider error where one is set.
struct FakeS3 {
    calls: Mutex<Vec<(String, Value)>>,
    head: Option<Result<HeadObject>>,
    list: Option<Result<ListResult>>,
    presign_base: String,
    delete_error: Option<Error>,
}

impl FakeS3 {
    fn new() -> Self {
        FakeS3 {
            calls: Mutex::new(Vec::new()),
            head: Some(Ok(HeadObject {
                key: String::new(),
                size: Some(42),
                content_type: Some("text/plain".into()),
                etag: Some("\"abc\"".into()),
                last_modified: Some("2026-10-08T00:00:00.000Z".into()),
                metadata: Some(json(r#"{"owner": "client-metadata"}"#)),
            })),
            list: None,
            presign_base: "https://signed.example.test".into(),
            delete_error: None,
        }
    }

    fn record(&self, call: &str, args: Value) {
        self.calls.lock().unwrap().push((call.into(), args));
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

impl S3Client for FakeS3 {
    async fn head_object(&self, bucket: &str, key: &str) -> Result<HeadObject> {
        self.record("HeadObject", pair("Bucket", bucket, "Key", key, &[]));
        self.head.clone().unwrap()
    }
    async fn delete_object(&self, bucket: &str, key: &str) -> Result<()> {
        self.record("DeleteObject", pair("Bucket", bucket, "Key", key, &[]));
        match &self.delete_error {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
    async fn list_objects(
        &self,
        bucket: &str,
        prefix: Option<&str>,
        max_keys: i64,
        continuation_token: Option<&str>,
    ) -> Result<ListResult> {
        let mut o = mosql::Object::new();
        o.set("Bucket", Value::str(bucket));
        if let Some(p) = prefix {
            o.set("Prefix", Value::str(p));
        }
        o.set("MaxKeys", Value::Int(max_keys));
        if let Some(c) = continuation_token {
            o.set("ContinuationToken", Value::str(c));
        }
        self.record("ListObjectsV2", Value::Object(o));
        self.list.clone().unwrap_or(Ok(ListResult::default()))
    }
    async fn presign(&self, command: PresignCommand<'_>, expires_in: i64) -> Result<String> {
        let (name, extra): (&str, Vec<(&str, Value)>) = match &command {
            PresignCommand::Put { content_type, .. } => {
                let mut extra: Vec<(&str, Value)> = content_type
                    .iter()
                    .map(|ct| ("ContentType", Value::str(*ct)))
                    .collect();
                extra.push(("expiresIn", Value::Int(expires_in)));
                ("PutObject", extra)
            }
            PresignCommand::Get { .. } => ("GetObject", vec![]),
        };
        self.record(
            name,
            pair("Bucket", command.bucket(), "Key", command.key(), &extra),
        );
        Ok(format!(
            "{}/{}/{}?X-Amz-Expires={expires_in}",
            self.presign_base,
            name,
            command.key()
        ))
    }
}

/// `{ a: b, c: d, ...extras }` — a command's input.
fn pair(a: &str, b: &str, c: &str, d: &str, extras: &[(&str, Value)]) -> Value {
    let mut o = mosql::Object::new();
    o.set(a, Value::str(b));
    o.set(c, Value::str(d));
    for (k, v) in extras {
        o.set(*k, v.clone());
    }
    Value::Object(o)
}

fn s3(overrides: &str) -> S3Options {
    S3Options::from_value(&json(&format!(
        r#"{{"bucket": "uploads", "region": "us-east-1", "accessKeyId": "test-access-key", "secretAccessKey": "test-secret-key"{overrides}}}"#
    )))
    .unwrap()
}

fn s3_value(overrides: &str) -> Value {
    json(&format!(
        r#"{{"bucket": "uploads", "region": "us-east-1", "accessKeyId": "test-access-key", "secretAccessKey": "test-secret-key"{overrides}}}"#
    ))
}

#[test]
fn s3_requires_bucket_region_and_static_credentials() {
    for (name, config) in [
        (
            "bucket",
            r#"{"region": "r", "accessKeyId": "a", "secretAccessKey": "s"}"#.to_string(),
        ),
        (
            "region",
            r#"{"bucket": "b", "accessKeyId": "a", "secretAccessKey": "s"}"#.to_string(),
        ),
        (
            "accessKeyId",
            r#"{"bucket": "b", "region": "r", "secretAccessKey": "s"}"#.to_string(),
        ),
        (
            "secretAccessKey",
            r#"{"bucket": "b", "region": "r", "accessKeyId": "a"}"#.to_string(),
        ),
        (
            "bucket",
            r#"{"bucket": "  ", "region": "r", "accessKeyId": "a", "secretAccessKey": "s"}"#
                .to_string(),
        ),
        (
            "region",
            r#"{"bucket": "b", "region": "", "accessKeyId": "a", "secretAccessKey": "s"}"#
                .to_string(),
        ),
        (
            "accessKeyId",
            r#"{"bucket": "b", "region": "r", "accessKeyId": "", "secretAccessKey": "s"}"#
                .to_string(),
        ),
    ] {
        expect_err(
            S3Options::from_value(&json(&config)),
            &format!("[createStorageService] '{name}' is required"),
        );
    }
    // endpoint, session token and path-style configuration are accepted.
    let o = s3(
        r#", "endpoint": "http://localhost:9000", "sessionToken": "temporary-token", "forcePathStyle": true"#,
    );
    assert_eq!(o.endpoint.as_deref(), Some("http://localhost:9000"));
    assert_eq!(o.session_token.as_deref(), Some("temporary-token"));
    assert!(o.force_path_style);
    // forcePathStyle defaults to Boolean(endpoint).
    assert!(s3(r#", "endpoint": "http://x""#).force_path_style);
    assert!(!s3("").force_path_style);
    assert!(!s3(r#", "endpoint": "http://x", "forcePathStyle": false"#).force_path_style);
    let _ = docs("[]");
}

#[test]
fn s3_expiration_defaults_and_validation() {
    assert_eq!(s3("").upload_expires_in, 900);
    assert_eq!(s3("").download_expires_in, 900);
    let o = s3(r#", "uploadExpiresIn": 300, "downloadExpiresIn": 60"#);
    assert_eq!((o.upload_expires_in, o.download_expires_in), (300, 60));
    for value in ["0", "-1", "604801", "1.5", "\"forever\"", "null"] {
        let e = expect_err(
            S3Options::from_value(&s3_value(&format!(r#", "uploadExpiresIn": {value}"#))),
            "uploadExpiresIn must be an integer between 1 and 604800 seconds",
        );
        assert_eq!(e.status(), 400);
    }
    expect_err(
        S3Options::from_value(&s3_value(r#", "downloadExpiresIn": 0"#)),
        "downloadExpiresIn",
    );
    // `presign` is the JavaScript way of passing the signing function; here
    // it is `S3Client::presign`, and the key only helps detect an S3 config.
    // …and an S3 config selected that way is validated like any other.
    expect_err(
        StorageKind::from_value(&json(r#"{"presign": "fn"}"#)),
        "[createStorageService] 'bucket' is required",
    );
}

#[test]
fn s3_presigned_urls() {
    run(async {
        let client = FakeS3::new();
        let service = S3Storage::new(
            &client,
            s3(r#", "endpoint": "https://objects.example.test""#),
        );
        assert_eq!(service.type_name(), "s3");

        let upload = service
            .presign_upload(&json(r#"{"key": "users/123/avatar.jpg"}"#))
            .await
            .unwrap();
        assert_eq!(upload.expires_in, Some(900));
        assert!(upload.url.as_deref().unwrap().contains("X-Amz-Expires=900"));
        assert!(upload
            .url
            .as_deref()
            .unwrap()
            .contains("users/123/avatar.jpg"));
        let download = service
            .presign_download(&json(
                r#"{"key": "users/123/avatar.jpg", "expiresIn": 120}"#,
            ))
            .await
            .unwrap();
        assert_eq!(download.expires_in, Some(120));
        assert!(download
            .url
            .as_deref()
            .unwrap()
            .contains("X-Amz-Expires=120"));
        assert_eq!(download.key, "users/123/avatar.jpg");
        expect_json(
            &download.to_value_download(),
            &format!(
                r#"{{"key": "users/123/avatar.jpg", "url": "{}", "expiresIn": 120}}"#,
                download.url.clone().unwrap()
            ),
        );

        // Configured defaults apply to `create` (a presigned PUT) and downloads.
        let service = S3Storage::new(
            &client,
            s3(r#", "uploadExpiresIn": 300, "downloadExpiresIn": 60"#),
        );
        let created = service
            .create(&json(r#"{"key": "docs/report.pdf"}"#))
            .await
            .unwrap();
        assert_eq!(created.expires_in, Some(300));
        assert_eq!(
            service
                .presign_download(&json(r#"{"key": "docs/report.pdf"}"#))
                .await
                .unwrap()
                .expires_in,
            Some(60)
        );

        // PUT with a content type: header to send, and a signed content-type.
        let result = service
            .presign_upload(&json(r#"{"key": "users/123/avatar image.jpg", "contentType": "image/jpeg", "expiresIn": 75}"#))
            .await
            .unwrap();
        assert_eq!(result.key, "users/123/avatar image.jpg");
        assert_eq!(result.method, "PUT");
        expect_json(&result.headers, r#"{"Content-Type": "image/jpeg"}"#);
        assert_eq!(result.expires_in, Some(75));
        let body = result.to_value_upload();
        assert!(!mosql::to_json(&body).contains("test-secret-key"));
        assert!(body.get("accessKeyId").is_none() && body.get("secretAccessKey").is_none());
        // The client saw a PutObject with the content type, then a 75s expiry.
        let calls = client.calls();
        assert_eq!(calls[calls.len() - 1].0, "PutObject");
        assert_eq!(
            calls[calls.len() - 1]
                .1
                .get("ContentType")
                .unwrap()
                .as_str(),
            Some("image/jpeg")
        );
        assert_eq!(
            calls[calls.len() - 1].1.get("expiresIn").unwrap().as_f64(),
            Some(75.0)
        );

        // GET downloads without credential fields.
        let result = service
            .presign_download(&json(r#"{"key": "private/user-1/report.pdf"}"#))
            .await
            .unwrap();
        assert_eq!(result.expires_in, Some(60));
        assert!(!mosql::to_json(&result.to_value_download()).contains("test-secret-key"));
        let calls = client.calls();
        assert_eq!(calls[calls.len() - 1].0, "GetObject");
    });
}

#[test]
fn s3_rejects_invalid_presign_input() {
    run(async {
        let client = FakeS3::new();
        let service = S3Storage::new(&client, s3(""));
        expect_err(
            service.presign_upload(&json("{}")).await,
            "key must be a non-empty string",
        );
        expect_err(
            service.presign_download(&json(r#"{"key": ""}"#)).await,
            "key must be a non-empty string",
        );
        expect_err(
            service
                .presign_upload(&json(r#"{"key": "a", "contentType": " "}"#))
                .await,
            "contentType must be a non-empty string",
        );
        expect_err(
            service
                .presign_upload(&json(r#"{"key": "a", "contentType": 5}"#))
                .await,
            "contentType must be a non-empty string",
        );
        expect_err(
            service
                .presign_download(&json(r#"{"key": "a", "expiresIn": 604801}"#))
                .await,
            "expiresIn must be an integer between 1 and 604800 seconds",
        );
        expect_err(
            service
                .presign_upload(&json(r#"{"key": "a", "expiresIn": 0}"#))
                .await,
            "expiresIn must be an integer between 1 and 604800 seconds",
        );
        expect_err(
            service
                .presign_upload(&json(r#"{"key": "a", "expiresIn": null}"#))
                .await,
            "expiresIn must be an integer between 1 and 604800 seconds",
        );
        expect_err(
            service.presign_upload(&json("5")).await,
            "upload input must be an object",
        );
        expect_err(
            service.presign_download(&json("[]")).await,
            "download input must be an object",
        );
        // contentLength cannot be enforced through a presigned PUT: a plain Error (500).
        let e = expect_err(
            service
                .presign_upload(&json(r#"{"key": "large.bin", "contentLength": 100}"#))
                .await,
            "contentLength cannot be enforced with a presigned PUT URL",
        );
        assert_eq!(e.status(), 500);
        assert_eq!(e.name(), "Error");
        assert!(
            client.calls().is_empty(),
            "nothing is signed for invalid input"
        );
    });
}

#[test]
fn s3_metadata_list_and_delete() {
    run(async {
        let client = FakeS3::new();
        let service = S3Storage::new(&client, s3(""));

        // HeadObject → stable metadata, opaque keys preserved, no extra fields.
        let meta = service
            .get(&json(r#"{"key": "users/123/a file.txt"}"#))
            .await
            .unwrap();
        expect_json(
            &meta.to_value_s3_head(),
            r#"{"key": "users/123/a file.txt", "size": 42, "contentType": "text/plain", "etag": "\"abc\"",
                "lastModified": "2026-10-08T00:00:00.000Z", "metadata": {"owner": "client-metadata"}}"#,
        );
        assert_eq!(client.calls()[0].0, "HeadObject");
        // A string key means the same thing.
        service.get(&Value::str("k")).await.unwrap();

        // Provider errors are propagated untouched.
        let failing = FakeS3 {
            head: Some(Err(Error::internal("Not Found"))),
            ..FakeS3::new()
        };
        let service = S3Storage::new(&failing, s3(""));
        let e = expect_err(
            service.get(&json(r#"{"key": "missing.txt"}"#)).await,
            "Not Found",
        );
        assert_eq!(e.status(), 500);

        // ListObjectsV2 with prefix, limit and opaque continuation token.
        let client = FakeS3 {
            list: Some(Ok(ListResult {
                contents: vec![ObjectInfo {
                    key: "users/123/avatar.jpg".into(),
                    size: Some(25),
                    content_type: None,
                    etag: Some("\"etag\"".into()),
                    last_modified: Some("2026-10-07T12:00:00.000Z".into()),
                    metadata: None,
                }],
                is_truncated: true,
                next_token: Some("next-page-token".into()),
            })),
            ..FakeS3::new()
        };
        let service = S3Storage::new(&client, s3(""));
        let result = service
            .find(&StorageQuery::from_value(&json(
                r#"{"query": {"prefix": "users/123/", "limit": 1, "cursor": "current-token"}}"#,
            )))
            .await
            .unwrap();
        let calls = client.calls();
        assert_eq!(calls[0].0, "ListObjectsV2");
        expect_json(
            &calls[0].1.clone(),
            r#"{"Bucket": "uploads", "Prefix": "users/123/", "MaxKeys": 1, "ContinuationToken": "current-token"}"#,
        );
        expect_json(
            &result.to_value_s3(),
            r#"{"data": [{"key": "users/123/avatar.jpg", "size": 25, "contentType": null, "etag": "\"etag\"",
                "lastModified": "2026-10-07T12:00:00.000Z"}], "nextCursor": "next-page-token"}"#,
        );

        // Empty bucket → `{ data: [], nextCursor: null }`.
        let client = FakeS3::new();
        let service = S3Storage::new(&client, s3(""));
        expect_json(
            &service
                .find(&StorageQuery::default())
                .await
                .unwrap()
                .to_value_s3(),
            r#"{"data": [], "nextCursor": null}"#,
        );
        let calls = client.calls();
        expect_json(&calls[0].1, r#"{"Bucket": "uploads", "MaxKeys": 100}"#);
        // A prefix of "" is still sent (it is defined), an empty cursor is not.
        service
            .find(&StorageQuery::from_value(&json(
                r#"{"prefix": "", "cursor": ""}"#,
            )))
            .await
            .unwrap();
        expect_json(
            &client.calls()[1].1,
            r#"{"Bucket": "uploads", "Prefix": "", "MaxKeys": 100}"#,
        );

        // Validation happens without fetching.
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"prefix": 5}"#)))
                .await,
            "prefix must be a string",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"cursor": {}}"#)))
                .await,
            "cursor must be a string",
        );
        expect_err(
            service
                .find(&StorageQuery::from_value(&json(r#"{"limit": 1001}"#)))
                .await,
            "limit must be an integer between 1 and 1000",
        );
        assert_eq!(client.calls().len(), 2);

        // Delete is idempotent and returns `{ key, deleted: true }`.
        let client = FakeS3::new();
        let service = S3Storage::new(&client, s3(""));
        expect_json(
            &service
                .remove(&json(r#"{"key": "documents/2026/report.pdf"}"#))
                .await
                .unwrap()
                .into(),
            r#"{"key": "documents/2026/report.pdf", "deleted": true}"#,
        );
        expect_json(
            &client.calls()[0].1,
            r#"{"Bucket": "uploads", "Key": "documents/2026/report.pdf"}"#,
        );
        // Provider delete errors propagate.
        let client = FakeS3 {
            delete_error: Some(Error::internal("Access Denied")),
            ..FakeS3::new()
        };
        let service = S3Storage::new(&client, s3(""));
        expect_err(
            service.remove(&Value::str("secret.txt")).await,
            "Access Denied",
        );
        expect_err(
            service.remove(&json("{}")).await,
            "key must be a non-empty string",
        );
    });
}

// -- dispatcher -------------------------------------------------------------------

#[test]
fn create_storage_service_dispatches_on_type() {
    run(async {
        // Local, via the shared surface.
        let tmp_dir = std::env::temp_dir().join(format!("uflite-dispatch-{}", std::process::id()));
        let storage = Storage::new(&json(&format!(
            r#"{{"type": "local", "basePath": {}}}"#,
            mosql::to_json(&Value::str(tmp_dir.to_string_lossy()))
        )))
        .unwrap();
        assert!(matches!(storage, Storage::Local(_)));
        assert_eq!(storage.type_name(), "local");
        let params = Params::new();
        expect_match(
            &storage
                .create(&json(r#"{"key": "a/b.txt", "body": "hi"}"#), &params)
                .await
                .unwrap(),
            r#"{"key": "a/b.txt", "size": 2, "contentType": "application/octet-stream"}"#,
        );
        expect_match(
            &storage.get(&Value::str("a/b.txt")).await.unwrap(),
            r#"{"key": "a/b.txt", "size": 2, "contentType": "application/octet-stream"}"#,
        );
        assert_eq!(
            storage
                .find(&StorageQuery::default())
                .await
                .unwrap()
                .data
                .len(),
            1
        );
        let up = storage
            .presign_upload(&json(r#"{"key": "a/b.txt"}"#), &params)
            .await
            .unwrap();
        assert_eq!(up.method, "PUT");
        // `{ key }` is accepted everywhere a key is, like `keyOrInput`.
        expect_match(
            &storage.get(&json(r#"{"key": "a/b.txt"}"#)).await.unwrap(),
            r#"{"key": "a/b.txt"}"#,
        );
        expect_json(
            &storage
                .remove(&json(r#"{"key": "a/b.txt"}"#))
                .await
                .unwrap(),
            r#"{"key": "a/b.txt", "deleted": true}"#,
        );
        let e = storage.get(&json("{}")).await.expect_err("no key");
        assert_eq!(
            e.to_string(),
            "key must be a relative file key using forward slashes"
        );
        let _ = std::fs::remove_dir_all(&tmp_dir);

        // S3 without a client: the constructor says so.
        expect_err(
            Storage::new(&s3_value("")),
            "[createStorageService] an S3Client is required for type 's3'",
        );
        // S3 with a client.
        let client = FakeS3::new();
        let storage = Storage::with_client(&s3_value(r#", "type": "s3""#), &client).unwrap();
        assert!(matches!(storage, Storage::S3(_)));
        assert_eq!(storage.type_name(), "s3");
        expect_match(
            &storage
                .create(&json(r#"{"key": "a/b.txt"}"#), &params)
                .await
                .unwrap(),
            r#"{"key": "a/b.txt", "method": "PUT", "expiresIn": 900}"#,
        );
        expect_json(
            &storage.remove(&Value::str("a/b.txt")).await.unwrap(),
            r#"{"key": "a/b.txt", "deleted": true}"#,
        );
        // A local configuration ignores the client entirely.
        let storage = Storage::with_client(
            &json(r#"{"type": "local", "basePath": "./data/files"}"#),
            &client,
        )
        .unwrap();
        assert!(matches!(storage, Storage::Local(_)));
        expect_err(
            Storage::new(&json(r#"{"type": 5}"#)),
            "unsupported storage type '5'",
        );
        expect_err(
            Storage::new(&json("[]")),
            "storage options must be an object",
        );
    });
}

#[test]
fn hooks_can_authorize_with_the_acl_exactly_like_the_spec() {
    run(async {
        let acl = uflite::acl::Acl::new(&json(
            r#"{"enabled": true, "rules": [{"id": "owner",
                "permissions": ["user:storage:presignUpload", "user:storage:presignDownload", "user:storage:remove"],
                "check": {"user.active": true}}]}"#,
        ))
        .unwrap();
        let client = FakeS3::new();
        let storage = S3Storage::new(&client, s3(""));
        // A `before` hook in Rust is a check before the call, with the params
        // as the ACL context (`{ user }`).
        let context = |params: &Params| {
            params
                .user
                .clone()
                .map(|u| json(&format!(r#"{{"user": {}}}"#, mosql::to_json(&u))))
                .unwrap_or_else(Value::object)
        };
        let authorize = |params: &Params, method: &str| -> Result<()> {
            if !acl
                .can(&context(params), &format!("user:storage:{method}"))
                .unwrap()
            {
                return Err(Error::internal("Forbidden"));
            }
            Ok(())
        };
        let inactive = Params::new().user(json(r#"{"active": false}"#));
        for method in ["presignUpload", "presignDownload", "remove"] {
            assert_eq!(
                authorize(&inactive, method).unwrap_err().to_string(),
                "Forbidden"
            );
        }
        let active = Params::new().user(json(r#"{"active": true}"#));
        authorize(&active, "presignUpload").unwrap();
        let result = storage
            .presign_upload(&json(r#"{"key": "private/a"}"#))
            .await
            .unwrap();
        expect_match(
            &result.to_value_upload(),
            r#"{"key": "private/a", "method": "PUT"}"#,
        );
    });
}

// -- helpers ----------------------------------------------------------------------

#[test]
fn paging_and_encoding_helpers() {
    let info = |key: &str| ObjectInfo {
        key: key.into(),
        size: None,
        content_type: None,
        etag: None,
        last_modified: None,
        metadata: None,
    };
    let all = || vec![info("a"), info("b"), info("c"), info("d")];
    let page = StoragePage::from_sorted(all(), None, 2);
    assert_eq!(
        (page.data.len(), page.next_cursor.as_deref()),
        (2, Some("b"))
    );
    // `remaining.length > limit` is false when the page exactly fits.
    let page = StoragePage::from_sorted(all(), Some("b"), 2);
    assert_eq!((page.data.len(), page.next_cursor.as_deref()), (2, None));
    let page = StoragePage::from_sorted(all(), Some("b"), 5);
    assert_eq!((page.data.len(), page.next_cursor.as_deref()), (2, None));
    // A cursor that sorts past everything yields an empty page.
    let page = StoragePage::from_sorted(all(), Some("z"), 5);
    assert_eq!((page.data.len(), page.next_cursor.as_deref()), (0, None));
    let page = StoragePage::from_sorted(vec![], Some(""), DEFAULT_LIMIT);
    assert_eq!(page.next_cursor, None);

    assert_eq!(encode_component("a b/c+dé"), "a%20b%2Fc%2Bd%C3%A9");
    assert_eq!(form_encode("a b+c&d"), "a+b%2Bc%26d");
    assert_eq!(MAX_LIMIT, 1000);
}

#[test]
fn temp_directories_never_collide() {
    // The macOS runner's clock has microsecond resolution, so naming the
    // directory by timestamp alone let two parallel tests share one directory;
    // the first to finish removed the other's files mid-test.
    let roots: Vec<std::path::PathBuf> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..32)
            .map(|_| {
                s.spawn(|| {
                    TempStorage::new(LocalOptions::default())
                        .get()
                        .root()
                        .to_path_buf()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut unique = roots.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), roots.len(), "temporary directories collided");
}
