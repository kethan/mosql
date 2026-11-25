//! Blobs, over R2.
//!
//! R2 is optional. When the deployment binds a bucket, the Worker keeps
//! arbitrary bodies there and serves them at `/api/blob/<key>`. Nothing
//! else in the engine depends on it: the authentication routes work
//! exactly the same with or without a bucket.
//!
//! The bucket is reached through the host, the same way the database is:
//! the engine asks, the host does, the answer comes back.

use mosql::Value;

use crate::bus::Bus;

/// Where the blob routes live.
pub const PREFIX: &str = "/api/blob/";

/// The blob adapter.
pub struct Blobs {
    bus: Bus,
}

/// What a blob is: its body, and what kind of thing it is.
#[derive(Debug, Clone)]
pub struct Blob {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

impl Blobs {
    /// An adapter that asks the host to reach the bucket.
    pub fn new(bus: Bus) -> Blobs {
        Blobs { bus }
    }

    /// Read a blob.
    pub async fn get(&self, key: &str) -> Result<Option<Blob>, String> {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("blobGet"));
        op.set("key", Value::str(key));
        let answer = self.bus.call(Value::Object(op)).await?;
        if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
            return Err(message.to_string());
        }
        let Some(body) = answer.get("body").and_then(|v| v.as_str()) else {
            return Ok(None);
        };
        Ok(Some(Blob {
            body: body.as_bytes().to_vec(),
            content_type: answer
                .get("contentType")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }))
    }

    /// Write a blob.
    pub async fn put(
        &self,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
    ) -> Result<(), String> {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("blobPut"));
        op.set("key", Value::str(key));
        op.set(
            "body",
            Value::str(String::from_utf8_lossy(body).into_owned()),
        );
        if let Some(content_type) = content_type {
            op.set("contentType", Value::str(content_type));
        }
        let answer = self.bus.call(Value::Object(op)).await?;
        if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
            return Err(message.to_string());
        }
        Ok(())
    }

    /// Remove a blob.
    pub async fn delete(&self, key: &str) -> Result<(), String> {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("blobDelete"));
        op.set("key", Value::str(key));
        let answer = self.bus.call(Value::Object(op)).await?;
        if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
            return Err(message.to_string());
        }
        Ok(())
    }

    /// The keys under a prefix.
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("blobList"));
        op.set("prefix", Value::str(prefix));
        let answer = self.bus.call(Value::Object(op)).await?;
        if let Some(message) = answer.get("error").and_then(|v| v.as_str()) {
            return Err(message.to_string());
        }
        Ok(answer
            .get("keys")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }
}

/// The key a blob route names, if the path is one.
pub fn key_of(path: &str) -> Option<&str> {
    path.strip_prefix(PREFIX)
        .map(|key| key.trim_end_matches('/'))
        .filter(|key| !key.is_empty())
}
