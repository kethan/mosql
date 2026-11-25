//! API keys, in the app's own database.
//!
//! A key belongs to an app. It is made for that app, it may only see
//! that app, and it lives in that app's database — in the collection
//! named here. The platform keeps a directory of which keys exist and
//! which app they belong to, and nothing else: the hash, the scopes
//! and the expiry are in the app's own rows.
//!
//! The collection is internal. It is not in the app's collection list
//! and the row routes refuse it, so the only way to see a key is
//! through the key routes — which never show the hash.

use mosql::Value;

use crate::apikey::ApiKey;

/// The collection an app's API keys live in.
pub const COLLECTION: &str = "_api_keys";

/// The key's row, as the app's database holds it.
///
/// The same shape the platform used to store: everything but the key
/// itself, which was never stored anywhere.
pub fn to_row(key: &ApiKey) -> Value {
    key.to_stored_value()
}

/// The key's row, read back.
pub fn from_row(row: &Value) -> Option<ApiKey> {
    ApiKey::from_value(row)
}

/// The key as the world sees it: never the hash.
pub fn to_public_row(key: &ApiKey) -> Value {
    key.to_value()
}

/// A stored row, without the hash.
///
/// The row as it comes back from the database has the hash in it, so
/// it is taken out again here: the answer to a question about a key
/// never carries the hash of the key.
pub fn to_public_value(row: &Value) -> Value {
    let mut out = match row.as_object() {
        Some(o) => o.clone(),
        None => return row.clone(),
    };
    out.remove("hash");
    Value::Object(out)
}

/// Is this the collection an app's keys live in?
///
/// The collection is internal: it is not in the app's list of
/// collections and the row routes refuse it, so the only way in is
/// through the key routes.
pub fn is_internal(collection: &str) -> bool {
    collection == COLLECTION
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_survives_the_round_trip_through_a_row() {
        let (key, _) = ApiKey::create(
            "deploy",
            "app_1",
            vec!["rows:read".to_string()],
            Some(1_800_000_000),
            1_700_000_000,
        );
        let row = to_row(&key);
        let back = from_row(&row).expect("the key");
        assert_eq!(back.id, key.id);
        assert_eq!(back.hash, key.hash);
        assert_eq!(back.app, "app_1");
        assert_eq!(back.scopes, key.scopes);
    }

    #[test]
    fn the_public_row_has_no_hash() {
        let (key, _) = ApiKey::create("deploy", "app_1", vec![], None, 0);
        let public = mosql::to_json(&to_public_row(&key));
        assert!(!public.contains(&key.hash), "the hash is not shown");
        assert!(public.contains(&key.preview), "the preview is");
    }
}
