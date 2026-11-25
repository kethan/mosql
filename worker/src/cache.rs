//! The cache, over KV.
//!
//! KV is optional. When the deployment binds one, the sessions it holds
//! are cached there, so a request that only needs to know whether a token
//! is live does not have to reach the database. The cache is keyed by
//! session id — which is what every authenticated request looks the
//! session up by — and it is invalidated whenever the session table is
//! written to.
//!
//! The invalidation has to be exact, because a stale session is a revoked
//! session that still works. A write that names the session invalidates
//! that one session; a write that does not — a bulk revocation by user,
//! say — invalidates every cached session, because it may have touched
//! any of them.
//!
//! The cache never changes an answer. It only avoids asking the same
//! question twice.

use std::sync::Arc;

use better_auth_core::error::AuthResult;
use better_auth_core::models::{RecordMap, MODEL_SESSION};
use better_auth_core::store::{AuthStore, BoxFuture, FindManyQuery, WhereClause};
use mosql::Value;

use crate::bus::Bus;
use crate::d1::D1Store;

/// The prefix every cached session is stored under.
const PREFIX: &str = "sess:";

/// How long a cached session lives, in seconds.
///
/// A session is revoked by deleting its row, and the cache is told. The
/// TTL is the backstop for the case where the invalidation itself did not
/// happen — an isolate that was asleep, a write that went to another
/// machine. A minute is short enough to be harmless and long enough to
/// matter, and it is the shortest lifetime the cache will accept.
const TTL_SECS: u64 = 60;

/// The cache key for a session id.
fn key_for(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// The session id a clause names, when it names exactly one.
///
/// The cache is keyed by the session's id, because that is what the
/// engine looks a session up by. A clause that names a different field
/// names nothing the cache can use.
fn session_id(r#where: &[WhereClause]) -> Option<String> {
    r#where
        .iter()
        .find(|clause| clause.field == "id" && clause.op == better_auth_core::store::FilterOp::Eq)
        .and_then(|clause| clause.value.as_str().map(str::to_string))
}

/// A store with a cache in front of it.
pub struct CachedStore {
    inner: Arc<D1Store>,
    bus: Bus,
}

impl CachedStore {
    /// Wrap a store, and cache its sessions.
    pub fn new(inner: Arc<D1Store>, bus: Bus) -> Arc<CachedStore> {
        Arc::new(CachedStore { inner, bus })
    }

    /// Read a cached session.
    async fn get(&self, id: &str) -> Option<RecordMap> {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("cacheGet"));
        op.set("key", Value::str(key_for(id)));
        let answer = self.bus.call(Value::Object(op)).await.ok()?;
        let text = answer.get("value").and_then(|v| v.as_str())?;
        let value = mosql::parse_json(text).ok()?;
        let mut record = RecordMap::new();
        if let Value::Object(fields) = value {
            for (key, value) in fields.iter() {
                record.set(key.clone(), value.clone());
            }
        }
        Some(record)
    }

    /// Remember a session.
    async fn put(&self, id: &str, record: &RecordMap) {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("cachePut"));
        op.set("key", Value::str(key_for(id)));
        op.set(
            "value",
            Value::str(mosql::to_json(&Value::Object(record.clone()))),
        );
        op.set("ttl", Value::Int(TTL_SECS as i64));
        let _ = self.bus.call(Value::Object(op)).await;
    }

    /// Forget a session.
    async fn forget(&self, id: &str) {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("cacheDelete"));
        op.set("key", Value::str(key_for(id)));
        let _ = self.bus.call(Value::Object(op)).await;
    }

    /// Forget every session.
    ///
    /// A write that does not name a session may touch any of them, so the
    /// whole prefix goes.
    async fn forget_all(&self) {
        let mut op = mosql::Object::new();
        op.set("kind", Value::str("cacheList"));
        op.set("prefix", Value::str(PREFIX));
        let Ok(answer) = self.bus.call(Value::Object(op)).await else {
            return;
        };
        let Some(keys) = answer.get("keys").and_then(|v| v.as_array()) else {
            return;
        };
        for key in keys {
            if let Some(key) = key.as_str() {
                let mut op = mosql::Object::new();
                op.set("kind", Value::str("cacheDelete"));
                op.set("key", Value::str(key));
                let _ = self.bus.call(Value::Object(op)).await;
            }
        }
    }

    /// Invalidate what a write to the session table may have changed.
    async fn invalidate(&self, model: &str, r#where: &[WhereClause]) {
        if model != MODEL_SESSION {
            return;
        }
        match session_id(r#where) {
            Some(id) => self.forget(&id).await,
            None => self.forget_all().await,
        }
    }
}

impl AuthStore for CachedStore {
    fn create<'a>(
        &'a self,
        model: &'a str,
        data: RecordMap,
    ) -> BoxFuture<'a, AuthResult<RecordMap>> {
        Box::pin(async move {
            // A new row has a new id, so nothing cached can be stale
            // because of it.
            self.inner.create(model, data).await
        })
    }

    fn find_one<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        Box::pin(async move {
            let id = if model == MODEL_SESSION {
                session_id(r#where)
            } else {
                None
            };
            if let Some(id) = &id {
                if let Some(record) = self.get(id).await {
                    return Ok(Some(record));
                }
            }
            let found = self.inner.find_one(model, r#where).await?;
            if let (Some(record), Some(id)) = (found.as_ref(), id.as_ref()) {
                self.put(id, record).await;
            }
            Ok(found)
        })
    }

    fn find_many<'a>(
        &'a self,
        model: &'a str,
        query: FindManyQuery,
    ) -> BoxFuture<'a, AuthResult<Vec<RecordMap>>> {
        self.inner.find_many(model, query)
    }

    fn count<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        self.inner.count(model, r#where)
    }

    fn update<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<Option<RecordMap>>> {
        Box::pin(async move {
            let found = self.inner.update(model, r#where, update).await?;
            self.invalidate(model, r#where).await;
            Ok(found)
        })
    }

    fn update_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
        update: RecordMap,
    ) -> BoxFuture<'a, AuthResult<u64>> {
        Box::pin(async move {
            let changed = self.inner.update_many(model, r#where, update).await?;
            self.invalidate(model, r#where).await;
            Ok(changed)
        })
    }

    fn delete<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<()>> {
        Box::pin(async move {
            self.inner.delete(model, r#where).await?;
            self.invalidate(model, r#where).await;
            Ok(())
        })
    }

    fn delete_many<'a>(
        &'a self,
        model: &'a str,
        r#where: &'a [WhereClause],
    ) -> BoxFuture<'a, AuthResult<u64>> {
        Box::pin(async move {
            let changed = self.inner.delete_many(model, r#where).await?;
            self.invalidate(model, r#where).await;
            Ok(changed)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use better_auth_core::store::WhereClause;

    #[test]
    fn a_clause_that_names_an_id_gives_a_key() {
        let clauses = vec![WhereClause::eq("id", Value::str("s1"))];
        assert_eq!(session_id(&clauses), Some("s1".to_string()));
    }

    #[test]
    fn a_clause_that_names_something_else_gives_no_key() {
        let clauses = vec![WhereClause::eq("userId", Value::str("u1"))];
        assert_eq!(session_id(&clauses), None);
    }
}
