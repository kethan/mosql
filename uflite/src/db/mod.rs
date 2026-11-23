//! `services/db.js` — a Feathers-style CRUD service over any [`Collection`].
//!
//! The service implements the five Feathers methods (`find`, `get`, `create`,
//! `update`, `patch`, `remove`) with Feathers pagination and `multi` rules:
//!
//! * `find` returns `{ total, limit, skip, data }`, or a bare array when
//!   `params.paginate == Some(false)`. `$limit`, `$skip`, `$sort` and `$select`
//!   are read from `params.query` and never reach the collection as filters.
//! * `get`, `update`, `patch` and `remove` on a missing id return
//!   [`Error::record_not_found`] (404).
//! * `update` is a full replacement: fields of the old document missing from
//!   the new one are `$unset`; the id is preserved.
//! * Passing `Value::Null` as the id to `patch`/`remove` — or an array to
//!   `create` — is the multi form, allowed only if `multi` permits it.
//!
//! ```no_run
//! # use uflite::{DbOptions, DbService, Params};
//! # async fn run() -> Result<(), uflite::Error> {
//! let users = DbService::memory(vec![], DbOptions::default());
//! let created = users
//!     .create(mosql::parse_json(r#"{"name": "Ada"}"#)?, &Params::new())
//!     .await?;
//! let again = users.get(created.get("_id").unwrap(), &Params::new()).await?;
//! # Ok(())
//! # }
//! ```

pub mod collection;
mod memory;

use core::future::Future;

use mosql::{Object, Value};

pub use collection::{Collection, FindOptions};
pub use memory::MemoryCollection;

use crate::error::{Error, Result};
use crate::params::{to_projection, Params};

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

/// `paginate: { default, max }` — `$limit` defaults to `default` and is capped
/// at `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paginate {
    pub default: i64,
    pub max: i64,
}

impl Default for Paginate {
    fn default() -> Self {
        Paginate {
            default: 10,
            max: 100,
        }
    }
}

/// Which multi-document operations are allowed. JavaScript accepts `true`,
/// an object (`{ create: true }`) or an array of method names; [`Multi::from_value`]
/// accepts the same shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Multi {
    pub create: bool,
    pub patch: bool,
    pub remove: bool,
}

impl Multi {
    pub const fn none() -> Self {
        Multi {
            create: false,
            patch: false,
            remove: false,
        }
    }

    pub const fn all() -> Self {
        Multi {
            create: true,
            patch: true,
            remove: true,
        }
    }

    /// `multiAllowed(option, method)` for every method at once.
    pub fn from_value(v: &Value) -> Multi {
        match v {
            Value::Bool(b) if *b => Multi::all(),
            Value::Object(o) => Multi {
                create: o.get("create").is_some_and(Value::is_truthy),
                patch: o.get("patch").is_some_and(Value::is_truthy),
                remove: o.get("remove").is_some_and(Value::is_truthy),
            },
            Value::Array(items) => {
                let has = |m: &str| items.iter().any(|i| i.as_str() == Some(m));
                Multi {
                    create: has("create"),
                    patch: has("patch"),
                    remove: has("remove"),
                }
            }
            _ => Multi::none(),
        }
    }

    fn allowed(&self, method: &str) -> bool {
        match method {
            "create" => self.create,
            "patch" => self.patch,
            _ => self.remove,
        }
    }
}

impl From<bool> for Multi {
    fn from(b: bool) -> Self {
        if b {
            Multi::all()
        } else {
            Multi::none()
        }
    }
}

/// `createDBService(getCollection, options)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbOptions {
    /// The id field documents use. `_id` by default.
    pub id_field: String,
    pub paginate: Paginate,
    pub multi: Multi,
}

impl Default for DbOptions {
    fn default() -> Self {
        DbOptions {
            id_field: "_id".to_string(),
            paginate: Paginate::default(),
            multi: Multi::none(),
        }
    }
}

impl DbOptions {
    pub fn id_field(mut self, field: impl Into<String>) -> Self {
        self.id_field = field.into();
        self
    }

    pub fn paginate(mut self, paginate: Paginate) -> Self {
        self.paginate = paginate;
        self
    }

    pub fn multi(mut self, multi: impl Into<Multi>) -> Self {
        self.multi = multi.into();
        self
    }

    /// Build from the JSON options a JavaScript caller would pass:
    /// `{ idField, paginate: { default, max }, multi }`.
    pub fn from_value(v: &Value) -> Result<DbOptions> {
        if !v.is_object() {
            return Err(Error::bad_request("options must be an object"));
        }
        let mut o = DbOptions::default();
        if let Some(Value::Str(s)) = v.get("idField") {
            o.id_field = s.clone();
        }
        if let Some(p) = v.get("paginate") {
            if !p.is_null() {
                let Some(po) = p.as_object() else {
                    return Err(Error::bad_request("paginate must be an object"));
                };
                let num = |k: &str, d: i64| -> Result<i64> {
                    match po.get(k) {
                        None | Some(Value::Null) => Ok(d),
                        Some(x) => x.to_number().map(|n| n as i64).ok_or_else(|| {
                            Error::bad_request(format!("paginate.{k} must be a number"))
                        }),
                    }
                };
                o.paginate = Paginate {
                    default: num("default", 10)?,
                    max: num("max", 100)?,
                };
            }
        }
        if let Some(m) = v.get("multi") {
            o.multi = Multi::from_value(m);
        }
        Ok(o)
    }
}

// ---------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------

/// Feathers page: `{ total, limit, skip, data }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub total: i64,
    pub limit: i64,
    pub skip: i64,
    pub data: Vec<Value>,
}

impl From<Page> for Value {
    fn from(p: Page) -> Value {
        let mut o = Object::new();
        o.set("total", Value::Int(p.total));
        o.set("limit", Value::Int(p.limit));
        o.set("skip", Value::Int(p.skip));
        o.set("data", Value::Array(p.data));
        Value::Object(o)
    }
}

/// `find` returns a page, or every matching document when pagination is off.
#[derive(Debug, Clone, PartialEq)]
pub enum FindResult {
    Paginated(Page),
    All(Vec<Value>),
}

impl From<FindResult> for Value {
    fn from(r: FindResult) -> Value {
        match r {
            FindResult::Paginated(p) => p.into(),
            FindResult::All(v) => Value::Array(v),
        }
    }
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// Where a service gets its collection from. JavaScript takes a
/// `getCollection(params)` function that may return `null` to mean "this
/// service is disabled"; returning `None` means the same here.
///
/// Every [`Collection`] is its own provider (blanket impl), so
/// `DbService::new(collection, options)` works directly. Implement this for
/// multi-tenant lookup.
pub trait CollectionProvider: Send + Sync {
    type Collection: Collection;

    fn collection(&self, params: &Params) -> Option<&Self::Collection>;
}

impl<C: Collection> CollectionProvider for C {
    type Collection = C;

    fn collection(&self, _params: &Params) -> Option<&C> {
        Some(self)
    }
}

/// A provider that always answers `None`: the "service disabled" mode.
#[derive(Debug, Clone, Copy, Default)]
pub struct Disabled;

impl CollectionProvider for Disabled {
    type Collection = MemoryCollection;

    fn collection(&self, _params: &Params) -> Option<&MemoryCollection> {
        None
    }
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// The service. Generic over the provider so a memory-backed service and a
/// SQL-backed one are the same type-level story.
pub struct DbService<P: CollectionProvider = MemoryCollection> {
    provider: P,
    options: DbOptions,
}

impl DbService<MemoryCollection> {
    /// `createDBService()` / `createDBService(null, { initialData })`:
    /// an isolated in-memory collection seeded with `docs`.
    pub fn memory(docs: Vec<Value>, options: DbOptions) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let coll = MemoryCollection::new(format!("uflite_memory_{n}"), docs);
        DbService {
            provider: coll,
            options,
        }
    }
}

impl<P: CollectionProvider> DbService<P> {
    /// `createDBService(getCollection, options)` with any provider.
    pub fn new(provider: P, options: DbOptions) -> Self {
        DbService { provider, options }
    }

    pub fn options(&self) -> &DbOptions {
        &self.options
    }

    pub fn provider(&self) -> &P {
        &self.provider
    }

    pub fn into_provider(self) -> P {
        self.provider
    }

    fn id_filter(&self, id: &Value) -> Value {
        let mut o = Object::new();
        o.set(self.options.id_field.clone(), id.clone());
        Value::Object(o)
    }

    fn projection_of(params: &Params) -> Option<Value> {
        to_projection(params.modifier("$select"))
    }

    // -- find ---------------------------------------------------------------

    /// `find(params)`. Paginated unless `params.paginate == Some(false)`.
    pub async fn find(&self, params: &Params) -> Result<FindResult> {
        let limit = get_limit(params, &self.options.paginate);
        let skip = get_skip(params);

        let Some(coll) = self.provider.collection(params) else {
            return Ok(if params.paginate == Some(false) {
                FindResult::All(Vec::new())
            } else {
                FindResult::Paginated(Page {
                    total: 0,
                    limit,
                    skip,
                    data: Vec::new(),
                })
            });
        };

        let query = params.clean_query();
        let projection = Self::projection_of(params);
        let sort = params.modifier("$sort").cloned();

        let total = coll.count_documents(&query).await?;

        if params.paginate == Some(false) {
            let data = coll
                .find(
                    &query,
                    projection.as_ref(),
                    FindOptions {
                        skip: 0,
                        limit: None,
                        sort,
                    },
                )
                .await?;
            return Ok(FindResult::All(data));
        }

        // Feathers semantics: $limit: 0 => no records, but still the total.
        if limit == 0 {
            return Ok(FindResult::Paginated(Page {
                total,
                limit: 0,
                skip,
                data: Vec::new(),
            }));
        }

        let data = coll
            .find(
                &query,
                projection.as_ref(),
                FindOptions {
                    skip,
                    limit: Some(limit),
                    sort,
                },
            )
            .await?;

        Ok(FindResult::Paginated(Page {
            total,
            limit,
            skip,
            data,
        }))
    }

    /// Convenience: `find` and expect a page (panics-free: returns the
    /// [`FindResult::All`] case as an error explaining pagination was off).
    pub async fn find_page(&self, params: &Params) -> Result<Page> {
        match self.find(params).await? {
            FindResult::Paginated(p) => Ok(p),
            FindResult::All(_) => Err(Error::internal(
                "find_page called with pagination disabled in params",
            )),
        }
    }

    // -- get ----------------------------------------------------------------

    /// `get(id, params)` — 404 if there is no such record.
    pub async fn get(&self, id: &Value, params: &Params) -> Result<Value> {
        let Some(coll) = self.provider.collection(params) else {
            return Err(Error::record_not_found(id));
        };
        let projection = Self::projection_of(params);
        let filter = self.id_filter(id);
        let found = coll.find_one(&filter, projection.as_ref()).await?;
        found.ok_or_else(|| Error::record_not_found(id))
    }

    // -- create -------------------------------------------------------------

    /// `create(data, params)`. An **array** `data` is the multi form: every
    /// document is inserted and the stored documents are returned as an array,
    /// or [`Error::internal("Multi create is not allowed")`] if `multi.create`
    /// is off. Returns `Value::Null` when the service is disabled.
    pub async fn create(&self, data: Value, params: &Params) -> Result<Value> {
        let Some(coll) = self.provider.collection(params) else {
            return Ok(Value::Null);
        };

        if let Value::Array(docs) = data {
            if !self.options.multi.allowed("create") {
                return Err(Error::internal("Multi create is not allowed"));
            }
            let ids = coll.insert_many(docs).await?;
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                out.push(self.get(&id, params).await?);
            }
            return Ok(Value::Array(out));
        }

        let id = coll.insert_one(data).await?;
        self.get(&id, params).await
    }

    /// Multi create as its own method — same rules as an array passed to
    /// [`create`](Self::create), clearer at the call site.
    pub async fn create_many(&self, docs: Vec<Value>, params: &Params) -> Result<Vec<Value>> {
        match self.create(Value::Array(docs), params).await? {
            Value::Array(v) => Ok(v),
            _ => Ok(Vec::new()),
        }
    }

    // -- update (full replacement) -----------------------------------------

    /// `update(id, data, params)`: replaces the record. Fields present on the
    /// old record but absent from `data` are removed; the id is kept.
    pub async fn update(&self, id: &Value, data: Value, params: &Params) -> Result<Value> {
        let Some(coll) = self.provider.collection(params) else {
            return Err(Error::record_not_found(id));
        };
        let filter = self.id_filter(id);
        let existing = coll
            .find_one(&filter, None)
            .await?
            .ok_or_else(|| Error::record_not_found(id))?;

        // { ...data, [idField]: existing[idField] }
        let mut replacement = match &data {
            Value::Object(o) => o.clone(),
            _ => Object::new(),
        };
        let id_value = existing
            .get(&self.options.id_field)
            .cloned()
            .unwrap_or(Value::Null);
        replacement.set(self.options.id_field.clone(), id_value);

        // Everything on the old record that the replacement does not mention
        // is unset — except the id.
        let mut unset = Object::new();
        if let Some(old) = existing.as_object() {
            for (key, _) in old.iter() {
                if key != &self.options.id_field && replacement.get(key).is_none() {
                    unset.set(key.clone(), Value::Int(1));
                }
            }
        }

        let mut update = Object::new();
        update.set("$set", Value::Object(replacement));
        if !unset.is_empty() {
            update.set("$unset", Value::Object(unset));
        }

        coll.update_one(&filter, &Value::Object(update)).await?;
        self.get(id, params).await
    }

    // -- patch --------------------------------------------------------------

    /// `patch(id, data, params)`. `id == Value::Null` is the multi form: every
    /// document matching `params.query` is updated and the stored documents are
    /// returned as an array, or [`Error::internal("Multi patch is not allowed")].
    /// Disabled services return `Value::Null` for the multi form.
    pub async fn patch(&self, id: &Value, data: &Value, params: &Params) -> Result<Value> {
        let Some(coll) = self.provider.collection(params) else {
            return Ok(Value::Null);
        };

        if id.is_null() {
            if !self.options.multi.allowed("patch") {
                return Err(Error::internal("Multi patch is not allowed"));
            }
            let query = params.clean_query();
            let existing = coll.find(&query, None, FindOptions::default()).await?;
            coll.update_many(&query, data).await?;
            let mut out = Vec::with_capacity(existing.len());
            for item in existing {
                let item_id = item
                    .get(&self.options.id_field)
                    .cloned()
                    .unwrap_or(Value::Null);
                out.push(self.get(&item_id, params).await?);
            }
            return Ok(Value::Array(out));
        }

        let filter = self.id_filter(id);
        coll.find_one(&filter, None)
            .await?
            .ok_or_else(|| Error::record_not_found(id))?;
        coll.update_one(&filter, data).await?;
        self.get(id, params).await
    }

    /// Multi patch as its own method.
    pub async fn patch_many(&self, data: &Value, params: &Params) -> Result<Vec<Value>> {
        match self.patch(&Value::Null, data, params).await? {
            Value::Array(v) => Ok(v),
            _ => Ok(Vec::new()),
        }
    }

    // -- remove -------------------------------------------------------------

    /// `remove(id, params)`: deletes and returns the record. `id ==
    /// Value::Null` is the multi form: every match is deleted and returned as
    /// an array, or [`Error::internal("Multi remove is not allowed")].
    pub async fn remove(&self, id: &Value, params: &Params) -> Result<Value> {
        let Some(coll) = self.provider.collection(params) else {
            return Ok(Value::Null);
        };

        if id.is_null() {
            if !self.options.multi.allowed("remove") {
                return Err(Error::internal("Multi remove is not allowed"));
            }
            let query = params.clean_query();
            let existing = coll.find(&query, None, FindOptions::default()).await?;
            coll.delete_many(&query).await?;
            return Ok(Value::Array(existing));
        }

        let filter = self.id_filter(id);
        let existing = coll
            .find_one(&filter, None)
            .await?
            .ok_or_else(|| Error::record_not_found(id))?;
        coll.delete_one(&filter).await?;
        Ok(existing)
    }

    /// Multi remove as its own method.
    pub async fn remove_many(&self, params: &Params) -> Result<Vec<Value>> {
        match self.remove(&Value::Null, params).await? {
            Value::Array(v) => Ok(v),
            _ => Ok(Vec::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// $limit / $skip — JavaScript `Number()` semantics, then clamp
// ---------------------------------------------------------------------------

/// `getLimit`: `$limit` if it is a finite number, else `paginate.default`;
/// clamped to `0..=paginate.max`.
fn get_limit(params: &Params, paginate: &Paginate) -> i64 {
    match params.modifier("$limit") {
        None | Some(Value::Null) => paginate.default,
        Some(v) => match v.to_number() {
            // Math.max(0, Math.min(limit, max)); NaN is excluded first.
            Some(n) if n.is_finite() => n.min(paginate.max as f64).max(0.0) as i64,
            _ => paginate.default,
        },
    }
}

/// `getSkip`: `$skip` if it is a finite, non-negative number, else `0`.
fn get_skip(params: &Params) -> i64 {
    match params.modifier("$skip") {
        None | Some(Value::Null) => 0,
        Some(v) => match v.to_number() {
            Some(n) if n.is_finite() => (n.max(0.0)) as i64,
            _ => 0,
        },
    }
}

/// Run a future to completion on the current thread. uflite's own tests use
/// it; consumers have a runtime. Park-based, so it only works for futures that
/// never need another thread to make progress — which the memory engine never
/// does.
pub fn block_on<F: Future>(fut: F) -> F::Output {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    struct ThreadWaker(std::thread::Thread);

    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut fut = Box::pin(fut);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            Poll::Ready(v) => return v,
            Poll::Pending => std::thread::park(),
        }
    }
}
