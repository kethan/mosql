//! The service layer: events, hooks and custom methods.
//!
//! uflite's JavaScript has a service wrapper. `app.service('tasks', impl)`
//! hands back something you can hang events on — `tasks.on('created', fn)`,
//! with `updated`, `patched` and `removed` beside it — hooks around every
//! call, and any extra function on the implementation becomes a method of
//! its own.
//!
//! This is the same thing in Rust, over the platform's own operations. It
//! is the extension point: a program that embeds nanobase registers a
//! listener, a hook or a method, and every change that goes through the
//! service — from the command line, from the runtime API, or from the
//! program itself — passes it.
//!
//! The events fire *after* a successful change, never before and never
//! after a failure, so a listener can rely on the record it is handed
//! being the one that was stored.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use mosql::{Object, Value};
use uflite::db::block_on;
use uflite::error::{Error, Result};

use crate::clone::{CloneRequest, CloneSummary};
use crate::config::AppDoc;
use crate::db::AppDb;
use crate::platform::{AppRecord, Platform, PlatformStore};

/// What happened.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Event {
    /// A record was made.
    Created,
    /// A record was replaced.
    Updated,
    /// A record was changed in part.
    Patched,
    /// A record was removed.
    Removed,
}

impl Event {
    /// The name the JavaScript side uses.
    pub fn name(&self) -> &'static str {
        match self {
            Event::Created => "created",
            Event::Updated => "updated",
            Event::Patched => "patched",
            Event::Removed => "removed",
        }
    }

    /// Every event, in the order they are listed in the docs.
    pub fn all() -> [Event; 4] {
        [
            Event::Created,
            Event::Updated,
            Event::Patched,
            Event::Removed,
        ]
    }
}

/// What a listener is told.
#[derive(Clone, Debug, Default)]
pub struct EventContext {
    /// The service the change happened in.
    pub service: String,
    /// The method that made the change: `create`, `update`, `patch`,
    /// `remove`.
    pub method: String,
    /// The record's id, when there is one.
    pub id: Option<String>,
    /// The app whose rows changed, when the change was to an app's rows.
    ///
    /// A row has no place in the platform's own list of apps, so without
    /// this a listener could not tell whose rows changed.
    pub app: Option<String>,
    /// The record as it stands after the change.
    pub record: Option<Value>,
    /// What went wrong, for the error hooks.
    pub error: Option<String>,
}

/// A listener: run when an event fires.
///
/// An `Arc`, so the list can be cloned and the listeners can run without
/// the lock held — a listener is free to call back into the service.
pub type Listener = Arc<dyn Fn(&EventContext) + Send + Sync>;

/// When a hook runs.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Phase {
    /// Before the method does anything.
    Before,
    /// After the method has succeeded.
    After,
    /// After the method has failed.
    Error,
}

impl Phase {
    pub fn name(&self) -> &'static str {
        match self {
            Phase::Before => "before",
            Phase::After => "after",
            Phase::Error => "error",
        }
    }

    /// The key a hook is filed under when it is not for one method in
    /// particular.
    pub const ALL: &'static str = "all";
}

/// A hook: run around a method.
pub type Hook = Arc<dyn Fn(&EventContext) + Send + Sync>;

/// The hooks, by phase and by method.
#[derive(Default)]
pub struct Hooks {
    before: BTreeMap<String, Vec<Hook>>,
    after: BTreeMap<String, Vec<Hook>>,
    error: BTreeMap<String, Vec<Hook>>,
}

impl Hooks {
    /// A hook set with nothing in it.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a hook for one method, or for every method with
    /// [`Phase::ALL`].
    pub fn add(&mut self, phase: Phase, method: &str, hook: Hook) -> &mut Self {
        let map = match phase {
            Phase::Before => &mut self.before,
            Phase::After => &mut self.after,
            Phase::Error => &mut self.error,
        };
        map.entry(method.to_string()).or_default().push(hook);
        self
    }

    fn run(&self, phase: Phase, method: &str, ctx: &EventContext) {
        let map = match phase {
            Phase::Before => &self.before,
            Phase::After => &self.after,
            Phase::Error => &self.error,
        };
        for key in [Phase::ALL, method] {
            if let Some(list) = map.get(key) {
                for hook in list {
                    hook(ctx);
                }
            }
        }
    }
}

/// A method the embedder added.
///
/// It is handed the service, so it can call the service's own methods,
/// and the arguments it was called with.
pub type CustomMethod = Arc<dyn Fn(&Service, &[String]) -> Result<Value> + Send + Sync>;

/// A service: the platform's operations, with events, hooks and extra
/// methods around them.
///
/// Cloning is cheap: the state is shared.
#[derive(Clone)]
pub struct Service {
    inner: Arc<Inner>,
}

struct Inner {
    name: String,
    store: Arc<dyn PlatformStore>,
    platform: Mutex<Platform>,
    listeners: Mutex<BTreeMap<Event, Vec<Listener>>>,
    hooks: Mutex<Hooks>,
    methods: Mutex<BTreeMap<String, CustomMethod>>,
    /// How an app's rows are reached.
    dbs: Mutex<Option<DbResolver>>,
}

/// How the service finds an app's rows.
///
/// The app's document says which engine the rows are in; this opens it.
/// The platform hands the service one of these, so the service never has
/// to know where an app's data lives.
pub type DbResolver = Arc<dyn Fn(&AppRecord) -> Result<Arc<dyn AppDb>> + Send + Sync>;

impl Service {
    /// A service over a store. The name is what the events and the hooks
    /// are filed under — `apps`, say.
    pub fn new(name: impl Into<String>, store: Arc<dyn PlatformStore>) -> Result<Self> {
        let platform = block_on(store.load())?;
        Ok(Service {
            inner: Arc::new(Inner {
                name: name.into(),
                store,
                platform: Mutex::new(platform),
                listeners: Mutex::new(BTreeMap::new()),
                hooks: Mutex::new(Hooks::new()),
                methods: Mutex::new(BTreeMap::new()),
                dbs: Mutex::new(None),
            }),
        })
    }

    /// The service's name.
    pub fn name(&self) -> &str {
        &self.inner.name
    }

    /// Say how an app's rows are reached.
    pub fn with_dbs(self, resolver: DbResolver) -> Self {
        *self.inner.dbs.lock().expect("not poisoned") = Some(resolver);
        self
    }

    // ------------------------------------------------------------ events --

    /// Listen for an event. Listeners run in the order they were added.
    pub fn on(&self, event: Event, listener: Listener) {
        self.inner
            .listeners
            .lock()
            .expect("not poisoned")
            .entry(event)
            .or_default()
            .push(listener);
    }

    /// The events this service knows about.
    pub fn events(&self) -> Vec<&'static str> {
        Event::all().iter().map(|e| e.name()).collect()
    }

    fn fire(&self, event: Event, ctx: &EventContext) {
        let listeners = {
            let map = self.inner.listeners.lock().expect("not poisoned");
            match map.get(&event) {
                Some(list) => list.clone(),
                None => return,
            }
        };
        for listener in listeners {
            listener(ctx);
        }
    }

    // ------------------------------------------------------------- hooks --

    /// Add a hook. `method` is a method name, or [`Phase::ALL`] for every
    /// method.
    pub fn hook(&self, phase: Phase, method: &str, hook: Hook) {
        self.inner
            .hooks
            .lock()
            .expect("not poisoned")
            .add(phase, method, hook);
    }

    // ---------------------------------------------------- custom methods --

    /// Register a method of your own. It can then be called with
    /// [`Service::call`].
    pub fn method(&self, name: &str, method: CustomMethod) {
        self.inner
            .methods
            .lock()
            .expect("not poisoned")
            .insert(name.to_string(), method);
    }

    /// The names of the registered methods, the service's own included.
    pub fn method_names(&self) -> Vec<String> {
        let names: Vec<String> = self
            .inner
            .methods
            .lock()
            .expect("not poisoned")
            .keys()
            .cloned()
            .collect();
        names
    }

    /// Call a registered method by name.
    pub fn call(&self, name: &str, args: &[String]) -> Result<Value> {
        let method = self
            .inner
            .methods
            .lock()
            .expect("not poisoned")
            .get(name)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("no method named {name}")))?;
        method(self, args)
    }

    // -------------------------------------------------------- operations --

    /// Re-read the platform from the store.
    ///
    /// The command line and a running server share the platform's file,
    /// so a key made with `nanobase key create` is not in the server's
    /// memory until the server looks again. This is how it looks again.
    /// The apps are re-read too, which is the same bargain.
    pub fn reload(&self) -> Result<()> {
        let platform = block_on(self.inner.store.load())?;
        *self.inner.platform.lock().expect("not poisoned") = platform;
        Ok(())
    }

    /// The platform as it stands.
    pub fn platform(&self) -> Result<Platform> {
        Ok(self.inner.platform.lock().expect("not poisoned").clone())
    }

    // -------------------------------------------------------------- keys --

    /// Make an API key. Returns the record and the key itself.
    ///
    /// The key is shown once. The platform keeps a hash of it, so there
    /// is no second chance to read it.
    pub fn create_key(
        &self,
        name: String,
        app: Option<String>,
        scopes: Vec<String>,
        expires_at: Option<i64>,
    ) -> Result<(Value, String)> {
        let now = crate::platform::now_secs();
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        let (record, secret) = crate::apikey::ApiKey::create(&name, app, scopes, expires_at, now);
        let value = record.to_value();
        platform.api_keys.push(record);
        block_on(self.inner.store.save(&platform))?;
        Ok((value, secret))
    }

    /// Change an API key.
    pub fn patch_key(&self, id: &str, changes: &Value) -> Result<Value> {
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        let index = platform
            .api_keys
            .iter()
            .position(|key| key.id == id)
            .ok_or_else(|| Error::not_found("no such API key"))?;
        {
            let key = &mut platform.api_keys[index];
            if let Some(name) = changes.get("name").and_then(|v| v.as_str()) {
                let name = name.trim();
                if name.is_empty() {
                    return Err(Error::bad_request("the name of a key may not be empty"));
                }
                key.name = name.to_string();
            }
            if let Some(Value::Array(list)) = changes.get("scopes") {
                let scopes: Vec<String> = list
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect();
                if let Some(unknown) = crate::apikey::first_unknown_scope(&scopes) {
                    return Err(Error::bad_request(format!(
                        "there is no scope called {unknown}"
                    )));
                }
                if scopes.is_empty() {
                    return Err(Error::bad_request("a key needs at least one scope"));
                }
                key.scopes = scopes;
            }
            match changes.get("expiresAt") {
                Some(Value::Int(seconds)) => key.expires_at = Some(*seconds),
                Some(Value::Null) => key.expires_at = None,
                _ => {}
            }
            if let Some(Value::Bool(revoked)) = changes.get("revoked") {
                key.revoked = *revoked;
            }
        }
        let value = platform.api_keys[index].to_value();
        block_on(self.inner.store.save(&platform))?;
        Ok(value)
    }

    /// Remove an API key.
    pub fn remove_key(&self, id: &str) -> Result<Value> {
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        let index = platform
            .api_keys
            .iter()
            .position(|key| key.id == id)
            .ok_or_else(|| Error::not_found("no such API key"))?;
        let removed = platform.api_keys.remove(index);
        block_on(self.inner.store.save(&platform))?;
        Ok(removed.to_value())
    }

    /// Remember that a key was used.
    ///
    /// The write is at most one a minute: a key used a thousand times a
    /// second must not write a thousand times a second, and "when was
    /// this last used" does not need to be exact to the second.
    pub fn touch_key(&self, id: &str, now: i64) -> Result<()> {
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        let Some(key) = platform.api_keys.iter_mut().find(|key| key.id == id) else {
            return Ok(());
        };
        if let Some(last) = key.last_used_at {
            if now - last < 60 {
                return Ok(());
            }
        }
        if let Some(key) = platform.api_keys.iter_mut().find(|key| key.id == id) {
            key.last_used_at = Some(now);
        }
        block_on(self.inner.store.save(&platform))?;
        Ok(())
    }

    /// The platform's apps, as values.
    pub fn list(&self) -> Result<Vec<Value>> {
        let platform = self.platform()?;
        Ok(platform.apps.iter().map(record_value).collect())
    }

    /// One app, by id or by name.
    pub fn find(&self, key: &str) -> Result<Option<Value>> {
        let platform = self.platform()?;
        Ok(platform.find_app(key).map(record_value))
    }

    /// Create an app. Fires [`Event::Created`].
    pub fn create(&self, doc: AppDoc) -> Result<Value> {
        let ctx = self.context("create", None);
        self.run_before(&ctx);
        let now = crate::platform::now_secs();
        let record = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let created = platform.create_app(doc, now)?;
            let value = record_value(created);
            block_on(self.inner.store.save(&platform))?;
            value
        };
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Created, &ctx);
        Ok(record)
    }

    /// Clone an app.
    ///
    /// The configuration and the schema come along; the rows come only
    /// when the request asks for them. Where the rows go — the clone's
    /// own database, the source's, or none yet — is worked out by
    /// [`crate::clone`] before anything is written.
    ///
    /// Fires [`Event::Created`] for the clone.
    pub fn clone_app(&self, key: &str, request: &CloneRequest) -> Result<CloneSummary> {
        let ctx = self.context("clone", Some(key));
        self.run_before(&ctx);
        let now = crate::platform::now_secs();
        let (record, plan) = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let (record, plan) = platform.clone_app(key, request, now)?;
            block_on(self.inner.store.save(&platform))?;
            (record, plan)
        };
        // The rows, when they are asked for. The catalog is not held
        // here: opening a database asks it for an app.
        let mut copied = 0usize;
        if plan.copy_rows {
            let source = self.db(&plan.source_id)?;
            let destination = self.db(&record.id)?;
            copied = crate::clone::copy_rows(source.as_ref(), destination.as_ref())?;
        }
        let summary = CloneSummary {
            id: record.id.clone(),
            name: record.doc.app.name.clone(),
            rows_copied: copied,
            shares_database: plan.shares_database,
            has_database: record.doc.config.database.is_configured(),
        };
        let ctx = EventContext {
            id: Some(summary.id.clone()),
            record: Some(record_value(&record)),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Created, &ctx);
        Ok(summary)
    }

    /// Replace an app's document. Fires [`Event::Updated`].
    pub fn update(&self, key: &str, doc: AppDoc) -> Result<Value> {
        let ctx = self.context("update", Some(key));
        self.run_before(&ctx);
        let record = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let updated = platform.replace_app(key, doc)?;
            let value = record_value(updated);
            block_on(self.inner.store.save(&platform))?;
            value
        };
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Updated, &ctx);
        Ok(record)
    }

    /// Change part of an app's document. Fires [`Event::Patched`].
    ///
    /// The changes are given as a JSON object and are merged into the
    /// document the app has now; a field that is not there yet is added.
    pub fn patch(&self, key: &str, changes: &Value) -> Result<Value> {
        let ctx = self.context("patch", Some(key));
        self.run_before(&ctx);
        let record = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let current = platform
                .find_app(key)
                .ok_or_else(|| Error::not_found(format!("no app named {key}")))?;
            let mut doc_value = current.doc.to_value();
            merge(&mut doc_value, changes);
            let doc = AppDoc::from_value(&doc_value).map_err(|e| {
                Error::unprocessable(format!("the patched document is not valid: {e}"))
            })?;
            let updated = platform.replace_app(key, doc)?;
            let value = record_value(updated);
            block_on(self.inner.store.save(&platform))?;
            value
        };
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Patched, &ctx);
        Ok(record)
    }

    /// Remove an app. Fires [`Event::Removed`].
    ///
    /// `confirm` has to be the app's id: a removal is not something to do
    /// by accident.
    pub fn remove(&self, key: &str, confirm: &str) -> Result<Value> {
        let ctx = self.context("remove", Some(key));
        self.run_before(&ctx);
        let record = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let removed = platform.remove_app(key, confirm)?;
            let value = record_value(&removed);
            block_on(self.inner.store.save(&platform))?;
            value
        };
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Removed, &ctx);
        Ok(record)
    }

    // ------------------------------------------------------------- rows --

    /// The database an app's rows live in.
    pub fn db(&self, app: &str) -> Result<Arc<dyn AppDb>> {
        let record = self.app_record(app)?;
        let resolver = self
            .inner
            .dbs
            .lock()
            .expect("not poisoned")
            .clone()
            .ok_or_else(|| Error::internal("no database resolver is configured"))?;
        resolver(&record)
    }

    /// An app's record, by id or by name.
    fn app_record(&self, app: &str) -> Result<AppRecord> {
        let platform = self.platform()?;
        platform
            .find_app(app)
            .cloned()
            .ok_or_else(|| Error::not_found(format!("no app named {app}")))
    }

    /// The collections an app has.
    pub fn collections(&self, app: &str) -> Result<Vec<String>> {
        self.db(app)?.collections()
    }

    /// Every row of a collection.
    pub fn rows(&self, app: &str, collection: &str) -> Result<Vec<Value>> {
        self.db(app)?.rows(collection)
    }

    /// The rows that match a filter.
    ///
    /// The filter is a document, the same one the engine understands.
    pub fn find_rows(
        &self,
        app: &str,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Value>> {
        self.db(app)?.find(collection, query, limit, offset)
    }

    /// How many rows match a filter.
    pub fn count_rows(&self, app: &str, collection: &str, query: &Value) -> Result<u64> {
        self.db(app)?.count(collection, query)
    }

    /// Put a row in. Fires [`Event::Created`].
    pub fn insert_row(&self, app: &str, collection: &str, row: &Value) -> Result<Value> {
        let ctx = self.row_context("db.insert", app, collection);
        self.run_before(&ctx);
        let db = self.db(app)?;
        let inserted = match db.insert(collection, row.clone()) {
            Ok(inserted) => inserted,
            Err(e) => {
                self.report_row_error("db.insert", app, collection, &e);
                return Err(e);
            }
        };
        let record = inserted;
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Created, &ctx);
        Ok(record)
    }

    /// Change the row that matches a filter. Fires [`Event::Updated`].
    ///
    /// The changes are merged into the matching row.
    pub fn update_row(
        &self,
        app: &str,
        collection: &str,
        query: &Value,
        changes: &Value,
    ) -> Result<Value> {
        let ctx = self.row_context("db.update", app, collection);
        self.run_before(&ctx);
        let db = self.db(app)?;
        let updated = match db.update(collection, query, changes) {
            Ok(updated) => updated,
            Err(e) => {
                self.report_row_error("db.update", app, collection, &e);
                return Err(e);
            }
        };
        let Some(record) = updated else {
            let e = Error::not_found("no row matches the filter");
            self.report_row_error("db.update", app, collection, &e);
            return Err(e);
        };
        let ctx = EventContext {
            id: record
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            record: Some(record.clone()),
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Updated, &ctx);
        Ok(record)
    }

    /// Take out the rows that match a filter. Fires [`Event::Removed`].
    pub fn remove_rows(&self, app: &str, collection: &str, query: &Value) -> Result<u64> {
        let ctx = self.row_context("db.remove", app, collection);
        self.run_before(&ctx);
        let db = self.db(app)?;
        let removed = match db.remove(collection, query) {
            Ok(removed) => removed,
            Err(e) => {
                self.report_row_error("db.remove", app, collection, &e);
                return Err(e);
            }
        };
        let ctx = EventContext {
            id: None,
            record: None,
            ..ctx
        };
        self.run_after(&ctx);
        self.fire(Event::Removed, &ctx);
        Ok(removed)
    }

    /// The context of a change to an app's rows.
    ///
    /// The app is named, and the collection is in the method name's path.
    fn row_context(&self, method: &str, app: &str, collection: &str) -> EventContext {
        EventContext {
            service: self.inner.name.clone(),
            method: format!("{method}:{collection}"),
            id: Some(app.to_string()),
            app: Some(app.to_string()),
            record: None,
            error: None,
        }
    }

    // ------------------------------------------------------------ inside --

    fn context(&self, method: &str, key: Option<&str>) -> EventContext {
        EventContext {
            service: self.inner.name.clone(),
            method: method.to_string(),
            id: key.map(str::to_string),
            app: None,
            record: None,
            error: None,
        }
    }

    fn run_before(&self, ctx: &EventContext) {
        let hooks = self.inner.hooks.lock().expect("not poisoned");
        hooks.run(Phase::Before, &ctx.method, ctx);
    }

    fn run_after(&self, ctx: &EventContext) {
        let hooks = self.inner.hooks.lock().expect("not poisoned");
        hooks.run(Phase::After, &ctx.method, ctx);
    }

    /// Tell the error hooks that a change to an app's rows went wrong.
    fn report_row_error(&self, method: &str, app: &str, collection: &str, error: &Error) {
        let ctx = EventContext {
            service: self.inner.name.clone(),
            method: format!("{method}:{collection}"),
            id: Some(app.to_string()),
            app: Some(app.to_string()),
            record: None,
            error: Some(error.to_string()),
        };
        let hooks = self.inner.hooks.lock().expect("not poisoned");
        hooks.run(Phase::Error, &ctx.method, &ctx);
    }

    /// Tell the error hooks that something went wrong.
    pub fn report_error(&self, method: &str, key: Option<&str>, error: &Error) {
        let ctx = EventContext {
            service: self.inner.name.clone(),
            method: method.to_string(),
            id: key.map(str::to_string),
            app: None,
            record: None,
            error: Some(error.to_string()),
        };
        let hooks = self.inner.hooks.lock().expect("not poisoned");
        hooks.run(Phase::Error, method, &ctx);
    }
}

fn record_value(record: &AppRecord) -> Value {
    let mut o = Object::new();
    o.set("id", Value::str(record.id.clone()));
    o.set("createdAt", Value::str(record.created_at.clone()));
    o.set("doc", record.doc.to_value());
    Value::Object(o)
}

/// Merge `changes` into `target`, one level deep for objects.
fn merge(target: &mut Value, changes: &Value) {
    let (Value::Object(target), Value::Object(changes)) = (&mut *target, changes) else {
        *target = changes.clone();
        return;
    };
    for (key, value) in changes.iter() {
        match (target.get(key), value) {
            (Some(Value::Object(existing)), Value::Object(new)) => {
                let mut merged = Value::Object(existing.clone());
                merge(&mut merged, &Value::Object(new.clone()));
                target.set(key, merged);
            }
            _ => target.set(key, value.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn service() -> Service {
        Service::new("apps", Arc::new(crate::platform::MemoryStore::new())).unwrap()
    }

    #[test]
    fn a_created_event_fires_after_the_change() {
        let svc = service();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        svc.on(
            Event::Created,
            Arc::new(move |ctx| {
                sink.lock().unwrap().push((
                    ctx.method.clone(),
                    ctx.record
                        .as_ref()
                        .and_then(|r| r.get("id"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                ));
            }),
        );

        let doc = AppDoc::empty("todos");
        let record = svc.create(doc).unwrap();
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "create");
        assert_eq!(seen[0].1.as_deref(), Some(id.as_str()));
    }

    #[test]
    fn no_event_fires_when_the_change_fails() {
        let svc = service();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        svc.on(
            Event::Created,
            Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        // Two apps with the same name: the second is refused.
        svc.create(AppDoc::empty("todos")).unwrap();
        assert!(svc.create(AppDoc::empty("todos")).is_err());
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn updated_patched_and_removed_each_have_their_own_event() {
        let svc = service();
        let seen = Arc::new(Mutex::new(Vec::new()));
        for event in Event::all() {
            let sink = Arc::clone(&seen);
            svc.on(
                event,
                Arc::new(move |_| {
                    sink.lock().unwrap().push(event.name());
                }),
            );
        }

        let record = svc.create(AppDoc::empty("todos")).unwrap();
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();

        let mut doc = AppDoc::empty("todos");
        doc.app.name = "todos-renamed".to_string();
        svc.update(&id, doc).unwrap();

        let mut changes = Object::new();
        changes.set("app", Value::str("ignored"));
        // A patch that changes nothing structural still fires.
        let mut nested = Object::new();
        nested.set("name", Value::str("todos"));
        changes.set("app", Value::Object(nested));
        svc.patch(&id, &Value::Object(changes)).unwrap();

        svc.remove(&id, &id).unwrap();

        assert_eq!(
            *seen.lock().unwrap(),
            vec!["created", "updated", "patched", "removed"]
        );
    }

    #[test]
    fn hooks_run_around_the_call() {
        let svc = service();
        let order = Arc::new(Mutex::new(Vec::new()));

        let before = Arc::clone(&order);
        svc.hook(
            Phase::Before,
            "create",
            Arc::new(move |_| {
                before.lock().unwrap().push("before");
            }),
        );
        let after = Arc::clone(&order);
        svc.hook(
            Phase::After,
            "create",
            Arc::new(move |_| {
                after.lock().unwrap().push("after");
            }),
        );

        svc.create(AppDoc::empty("todos")).unwrap();
        assert_eq!(*order.lock().unwrap(), vec!["before", "after"]);
    }

    #[test]
    fn a_hook_for_every_method_fires_for_every_method() {
        let svc = service();
        let count = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&count);
        svc.hook(
            Phase::Before,
            Phase::ALL,
            Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );

        svc.create(AppDoc::empty("todos")).unwrap();
        let record = svc.list().unwrap();
        let id = record[0]
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap()
            .to_string();
        svc.remove(&id, &id).unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_custom_method_can_be_called() {
        let svc = service();
        svc.method(
            "count",
            Arc::new(|svc, _args| {
                let apps = svc.list()?;
                Ok(Value::Int(apps.len() as i64))
            }),
        );
        assert_eq!(svc.method_names(), vec!["count".to_string()]);
        svc.create(AppDoc::empty("todos")).unwrap();
        assert_eq!(svc.call("count", &[]).unwrap(), Value::Int(1));
        assert!(svc.call("nope", &[]).is_err());
    }

    #[test]
    fn a_change_is_stored() {
        let store = Arc::new(crate::platform::MemoryStore::new());
        let svc = Service::new("apps", Arc::clone(&store) as Arc<dyn PlatformStore>).unwrap();
        svc.create(AppDoc::empty("todos")).unwrap();
        // A second service over the same store sees it.
        let again = Service::new("apps", store as Arc<dyn PlatformStore>).unwrap();
        assert_eq!(again.list().unwrap().len(), 1);
    }
}
