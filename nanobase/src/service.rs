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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mosql::{Object, Value};
use uflite::db::block_on;
use uflite::error::{Error, Result};

use crate::acl::{Acl, Caller, Policy};
use crate::clone::{CloneRequest, CloneSummary};
use crate::config::AppDoc;
use crate::db::AppDb;
use crate::platform::{AppRecord, Platform, PlatformStore};

/// Refuse the collection an app's keys live in.
///
/// The keys are the platform's, not the app's: they live in the app's
/// database because that is where the app's own rows live, but they are
/// not one of the app's collections. The only way in is through the key
/// routes, which never show the hash — so a collection called this is
/// not there, as far as anybody else is concerned.
fn refuse_internal(collection: &str) -> Result<()> {
    if crate::keys::is_internal(collection) {
        return Err(Error::not_found("no such collection"));
    }
    Ok(())
}

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

/// What a key migration did.
///
/// The platform used to keep every key in one list of its own. A key
/// now belongs to an app and lives in that app's database, so the keys
/// of an older platform are moved into their apps — and the ones that
/// cannot be moved are said out loud, rather than being lost.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyMigration {
    /// The keys that were moved: the app, the key, and its name.
    pub moved: Vec<(String, String, String)>,
    /// The keys that named no app.
    ///
    /// They were made for the whole platform, which a key no longer
    /// is. They wait until somebody says which app they are for.
    pub unbound: Vec<String>,
    /// The keys whose app is no longer here.
    pub orphaned: Vec<String>,
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
    /// Whether the keys of an older platform have been moved yet.
    migrated: AtomicBool,
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
                migrated: AtomicBool::new(false),
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
        // The platform that has just come in may have keys of an older
        // platform in it, so the migration is due again.
        self.inner.migrated.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// The platform as it stands.
    pub fn platform(&self) -> Result<Platform> {
        Ok(self.inner.platform.lock().expect("not poisoned").clone())
    }

    // -------------------------------------------------------------- keys ---

    /// The keys an app has.
    ///
    /// The keys live in the app's own database, so this reads them from
    /// there. The hash is never shown: a list is for seeing which keys
    /// exist, not what they are.
    pub fn list_keys(&self, app: &str) -> Result<Vec<Value>> {
        let db = self.db(app)?;
        let rows = db.rows(crate::keys::COLLECTION)?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            if let Some(key) = crate::keys::from_row(&row) {
                out.push(crate::keys::to_public_row(&key));
            }
        }
        out.sort_by(|a, b| {
            let name = |v: &Value| {
                v.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            };
            name(a).cmp(&name(b))
        });
        Ok(out)
    }

    /// Make an API key for an app. Returns the record and the key itself.
    ///
    /// The key is shown once. The app's database keeps a hash of it, so
    /// there is no second chance to read it.
    pub fn create_key(
        &self,
        app: &str,
        name: String,
        scopes: Vec<String>,
        expires_at: Option<i64>,
    ) -> Result<(Value, String)> {
        // The app must be there: a key belongs to an app, and the app's
        // database is where the key lives.
        let record = self.app_record(app)?;
        let now = crate::platform::now_secs();
        let (key, secret) =
            crate::apikey::ApiKey::create(&name, record.id.clone(), scopes, expires_at, now);
        let value = key.to_value();
        let db = self.db(app)?;
        db.insert(crate::keys::COLLECTION, crate::keys::to_row(&key))?;
        // The platform's directory says which app the key belongs to.
        // It is written after the key itself, so a failure here leaves
        // a key that cannot be found rather than a directory that
        // points at nothing — and the row is taken back out again.
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        platform.register_key(&record.id, &key.preview, &key.id);
        if let Err(e) = block_on(self.inner.store.save(&platform)) {
            platform.unregister_key(&record.id, &key.id);
            let mut filter = Object::new();
            filter.set("id", Value::str(key.id.clone()));
            let _ = db.remove(crate::keys::COLLECTION, &Value::Object(filter));
            return Err(e);
        }
        Ok((value, secret))
    }

    /// Change an API key.
    pub fn patch_key(&self, app: &str, id: &str, changes: &Value) -> Result<Value> {
        let key = self.find_key(app, id)?;
        let mut key = key;
        // What the record was before the change, so the version is
        // moved only when something really changed.
        let before = (
            key.name.clone(),
            key.scopes.clone(),
            key.expires_at,
            key.revoked,
        );
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
        // The version goes up with the change it describes, in the same
        // write: the key is in the app's own database, and a reader
        // that compares the version cannot be told a change that did
        // not happen, nor miss one that did.
        if before
            != (
                key.name.clone(),
                key.scopes.clone(),
                key.expires_at,
                key.revoked,
            )
        {
            key.version += 1;
        }
        let db = self.db(app)?;
        let mut filter = Object::new();
        filter.set("id", Value::str(id));
        let updated = db.update(
            crate::keys::COLLECTION,
            &Value::Object(filter),
            &crate::keys::to_row(&key),
        )?;
        let Some(updated) = updated else {
            return Err(Error::not_found("no such API key"));
        };
        Ok(crate::keys::to_public_value(&updated))
    }

    /// Remove an API key.
    pub fn remove_key(&self, app: &str, id: &str) -> Result<Value> {
        let key = self.find_key(app, id)?;
        let db = self.db(app)?;
        let mut filter = Object::new();
        filter.set("id", Value::str(id));
        db.remove(crate::keys::COLLECTION, &Value::Object(filter))?;
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        platform.unregister_key(&key.app, id);
        block_on(self.inner.store.save(&platform))?;
        Ok(crate::keys::to_public_row(&key))
    }

    /// Move the keys an older platform kept into their apps, once.
    ///
    /// This is what the platform does when it starts, and what a
    /// command does before it reads the keys. A platform with no old
    /// keys has nothing to do, and the flag says so.
    ///
    /// Without a way to open an app's database there is nowhere to move
    /// a key to, so the keys wait rather than being lost.
    pub fn ensure_migrated(&self) -> Result<KeyMigration> {
        if self.inner.migrated.load(Ordering::SeqCst) {
            return Ok(KeyMigration::default());
        }
        if self.inner.dbs.lock().expect("not poisoned").is_none() {
            return Ok(KeyMigration::default());
        }
        let report = self.migrate_keys()?;
        self.inner.migrated.store(true, Ordering::SeqCst);
        Ok(report)
    }

    /// Move the keys an older platform kept into the apps they belong to.
    ///
    /// The platform used to keep every key in one list of its own. A key
    /// now belongs to an app and lives in that app's database, so a
    /// platform file from before that change is read into the
    /// platform's legacy list and moved here:
    ///
    /// * a key that named an app is moved into that app's rows, and
    ///   the app's directory points at it. The hash comes with it, so
    ///   the secret that was shown once still works.
    /// * a key that named no app had authority over the whole
    ///   platform, which a key no longer has. It waits, and says so,
    ///   rather than being dropped or quietly given an app it was
    ///   never made for.
    pub fn migrate_keys(&self) -> Result<KeyMigration> {
        let mut report = KeyMigration::default();
        // The keys are taken out of the platform first, and the lock
        // is let go: an app's database is opened while the keys are
        // being moved, and opening one asks the platform for the app.
        let waiting = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            std::mem::take(&mut platform.legacy_keys)
        };
        if waiting.is_empty() {
            return Ok(report);
        }
        let mut left = Vec::new();
        let mut moved = Vec::new();
        for legacy in waiting {
            let Some(app) = legacy.app.clone() else {
                // A key for the whole platform. It waits for an owner
                // to say which app it is for.
                report.unbound.push(legacy.id.clone());
                left.push(legacy);
                continue;
            };
            let record = {
                let platform = self.inner.platform.lock().expect("not poisoned");
                platform.find_app(&app).cloned()
            };
            let Some(record) = record else {
                // The app it was made for is not here any more, so the
                // key can never be used again. It waits with the
                // others rather than being thrown away.
                report.orphaned.push(legacy.id.clone());
                left.push(legacy);
                continue;
            };
            let bound = legacy.bind(&record.id);
            let db = self.db(&record.id)?;
            let mut filter = Object::new();
            filter.set("id", Value::str(bound.id.clone()));
            if db
                .find_one(crate::keys::COLLECTION, &Value::Object(filter))?
                .is_none()
            {
                db.insert(crate::keys::COLLECTION, crate::keys::to_row(&bound))?;
            }
            moved.push((record.id.clone(), bound.preview.clone(), bound.id.clone()));
            report.moved.push((record.id, bound.id, bound.name));
        }
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        for (app, preview, id) in moved {
            platform.register_key(&app, &preview, &id);
        }
        platform.legacy_keys = left;
        block_on(self.inner.store.save(&platform))?;
        Ok(report)
    }

    /// Bind a key an older platform kept to an app.
    ///
    /// The key is moved into the app's own database, and the app's
    /// directory points at it. This is how a key that was made for the
    /// whole platform becomes a key of one app.
    pub fn bind_legacy_key(&self, app: &str, id: &str) -> Result<Value> {
        let record = self.app_record(app)?;
        // The key is taken out of the platform first, and the lock is
        // let go: the app's database is opened on the way in.
        let legacy = {
            let mut platform = self.inner.platform.lock().expect("not poisoned");
            let Some(index) = platform.legacy_keys.iter().position(|k| k.id == id) else {
                return Err(Error::not_found("no such API key"));
            };
            platform.legacy_keys.remove(index)
        };
        let bound = legacy.bind(&record.id);
        let db = self.db(&record.id)?;
        db.insert(crate::keys::COLLECTION, crate::keys::to_row(&bound))?;
        let mut platform = self.inner.platform.lock().expect("not poisoned");
        platform.register_key(&record.id, &bound.preview, &bound.id);
        block_on(self.inner.store.save(&platform))?;
        Ok(crate::keys::to_public_row(&bound))
    }

    /// The keys an older platform kept, and that have not been moved.
    pub fn legacy_keys(&self) -> Result<Vec<crate::apikey::LegacyKey>> {
        let platform = self.platform()?;
        Ok(platform.legacy_keys.clone())
    }

    /// One of an app's keys, by id.
    pub fn find_key(&self, app: &str, id: &str) -> Result<crate::apikey::ApiKey> {
        let db = self.db(app)?;
        let mut filter = Object::new();
        filter.set("id", Value::str(id));
        let row = db
            .find_one(crate::keys::COLLECTION, &Value::Object(filter))?
            .ok_or_else(|| Error::not_found("no such API key"))?;
        crate::keys::from_row(&row).ok_or_else(|| Error::internal("the API key cannot be read"))
    }

    /// Remember that a key was used.
    ///
    /// The write is at most one a minute: a key used a thousand times a
    /// second must not write a thousand times a second, and "when was
    /// this last used" does not need to be exact to the second.
    ///
    /// The version does **not** go up. The time a key was last used is
    /// not a permission — it is a fact about the past, it is
    /// approximate, and a reader that cached it would be caching a
    /// number that is wrong by up to a minute anyway.
    pub fn touch_key(&self, app: &str, id: &str, now: i64) -> Result<()> {
        let Ok(mut key) = self.find_key(app, id) else {
            return Ok(());
        };
        if let Some(last) = key.last_used_at {
            if now - last < 60 {
                return Ok(());
            }
        }
        key.last_used_at = Some(now);
        let db = self.db(app)?;
        let mut filter = Object::new();
        filter.set("id", Value::str(id));
        db.update(
            crate::keys::COLLECTION,
            &Value::Object(filter),
            &crate::keys::to_row(&key),
        )?;
        Ok(())
    }

    /// The key a secret belongs to, read from the app's own database.
    ///
    /// The app is found from the platform's directory — the preview says
    /// which app — and the key is read from that app's rows. The hash
    /// is compared in constant time, and the answer says whether the
    /// secret is this key.
    pub fn verify_key(
        &self,
        app: &str,
        id: &str,
        secret: &str,
    ) -> Result<Option<crate::apikey::ApiKey>> {
        let key = match self.find_key(app, id) {
            Ok(key) => key,
            // A key that is not there is simply not this secret. Any
            // other trouble — a database that will not open, a row that
            // cannot be read — is said out loud, so a broken database
            // is not mistaken for a wrong key.
            Err(e) if e.is_not_found() => return Ok(None),
            Err(e) => return Err(e),
        };
        if key.verify(secret) {
            return Ok(Some(key));
        }
        Ok(None)
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

    /// An app's own database configuration, with its secrets.
    ///
    /// The document the world sees has the password taken out of it, but
    /// the platform still holds it: this is the platform's own copy,
    /// and it is what a connection is opened with.
    pub fn database_config(&self, app: &str) -> Result<crate::config::DatabaseConfig> {
        let record = self.app_record(app)?;
        Ok(record.doc.config.database.clone())
    }

    /// One app, with its secrets.
    ///
    /// The owner may see what the platform holds; a program may not.
    pub fn find_with_secrets(&self, key: &str) -> Result<Option<Value>> {
        let platform = self.platform()?;
        Ok(platform.find_app(key).map(record_value_with_secrets))
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
            // The app's keys live in the app's own database and go with
            // it; the directory must stop pointing at them.
            platform.forget_app_keys(&removed.id);
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
        let mut names = self.db(app)?.collections()?;
        // The keys live in a collection of their own, and it is not
        // one of the app's: it is the platform's, and the only way in
        // is through the key routes.
        names.retain(|name| !crate::keys::is_internal(name));
        Ok(names)
    }

    /// The rules that apply to an app, and the evaluator for this caller.
    ///
    /// `None` when the app has no rules, or has them switched off.
    fn policy_for(&self, app: &str, caller: &Caller) -> Result<Option<Acl>> {
        let record = self.app_record(app)?;
        let policy = Policy::new(&record.doc.config.acl);
        policy
            .evaluator(caller)
            .map_err(|e| Error::internal(format!("the app's rules are not usable: {e}")))
    }

    /// The rows that match a filter, as this caller may see them.
    ///
    /// The rules filter a list row by row, so the page is taken after
    /// the filter: the page a caller sees is a page of rows they may
    /// see, not a page that happens to contain some they may not.
    pub fn find_rows_as(
        &self,
        caller: &Caller,
        app: &str,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Value>> {
        let acl = self.policy_for(app, caller)?;
        let Some(acl) = acl else {
            return self.find_rows(app, collection, query, limit, offset);
        };
        let rows = self.db(app)?.find(collection, query, None, None)?;
        let allowed = Policy::filter(Some(&acl), caller, collection, "list", rows);
        let offset = offset.unwrap_or(0) as usize;
        let mut page: Vec<Value> = allowed.into_iter().skip(offset).collect();
        if let Some(limit) = limit {
            page.truncate(limit as usize);
        }
        Ok(page)
    }

    /// How many rows match a filter, as this caller may see them.
    pub fn count_rows_as(
        &self,
        caller: &Caller,
        app: &str,
        collection: &str,
        query: &Value,
    ) -> Result<u64> {
        let acl = self.policy_for(app, caller)?;
        let Some(acl) = acl else {
            return self.count_rows(app, collection, query);
        };
        let rows = self.db(app)?.find(collection, query, None, None)?;
        Ok(Policy::filter(Some(&acl), caller, collection, "list", rows).len() as u64)
    }

    /// Put a row in, as this caller may. Fires [`Event::Created`].
    ///
    /// The rule is about the row being created, so a rule that says
    /// "the owner may create a row they own" refuses a row that claims
    /// somebody else.
    pub fn insert_row_as(
        &self,
        caller: &Caller,
        app: &str,
        collection: &str,
        row: &Value,
    ) -> Result<Value> {
        if let Some(acl) = self.policy_for(app, caller)? {
            if !Policy::allows(Some(&acl), caller, collection, "create", row) {
                return Err(Error::forbidden(
                    "the app's rules do not allow this row to be created",
                ));
            }
        }
        self.insert_row(app, collection, row)
    }

    /// Change a row, as this caller may. Fires [`Event::Updated`].
    ///
    /// The rule is about the row as it is, so a rule that says "you may
    /// change a row that is not done" is about the row's state now.
    pub fn update_row_as(
        &self,
        caller: &Caller,
        app: &str,
        collection: &str,
        query: &Value,
        changes: &Value,
    ) -> Result<Value> {
        let acl = self.policy_for(app, caller)?;
        let Some(acl) = acl else {
            return self.update_row(app, collection, query, changes);
        };
        let matching = self.db(app)?.find(collection, query, None, None)?;
        let allowed = Policy::filter(Some(&acl), caller, collection, "update", matching);
        let Some(row) = allowed.first() else {
            // A row the rules refuse looks like a row that is not
            // there: the answer must not say which it was.
            let e = Error::not_found("no row matches the filter");
            self.report_row_error("db.update", app, collection, &e);
            return Err(e);
        };
        let id = row
            .get("id")
            .cloned()
            .ok_or_else(|| Error::internal("a row needs an id"))?;
        let mut filter = Object::new();
        filter.set("id", id);
        self.update_row(app, collection, &Value::Object(filter), changes)
    }

    /// Take rows out, as this caller may. Fires [`Event::Removed`].
    pub fn remove_rows_as(
        &self,
        caller: &Caller,
        app: &str,
        collection: &str,
        query: &Value,
    ) -> Result<u64> {
        let acl = self.policy_for(app, caller)?;
        let Some(acl) = acl else {
            return self.remove_rows(app, collection, query);
        };
        let matching = self.db(app)?.find(collection, query, None, None)?;
        let allowed = Policy::filter(Some(&acl), caller, collection, "delete", matching);
        let mut removed = 0u64;
        for row in allowed {
            let Some(id) = row.get("id").cloned() else {
                continue;
            };
            let mut filter = Object::new();
            filter.set("id", id);
            removed += self.remove_rows(app, collection, &Value::Object(filter))?;
        }
        Ok(removed)
    }

    /// Every row of a collection.
    pub fn rows(&self, app: &str, collection: &str) -> Result<Vec<Value>> {
        refuse_internal(collection)?;
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
        refuse_internal(collection)?;
        self.db(app)?.find(collection, query, limit, offset)
    }

    /// How many rows match a filter.
    pub fn count_rows(&self, app: &str, collection: &str, query: &Value) -> Result<u64> {
        refuse_internal(collection)?;
        self.db(app)?.count(collection, query)
    }

    /// Put a row in. Fires [`Event::Created`].
    pub fn insert_row(&self, app: &str, collection: &str, row: &Value) -> Result<Value> {
        refuse_internal(collection)?;
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
        refuse_internal(collection)?;
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
        refuse_internal(collection)?;
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

/// An app, as the world sees it.
///
/// The document is handed out without its secrets: the database
/// password, the token, the storage keys. The list of the fields that
/// hold one is beside it, so an editor can say "there is a token here"
/// without being told the token.
fn record_value(record: &AppRecord) -> Value {
    let mut o = Object::new();
    o.set("id", Value::str(record.id.clone()));
    o.set("createdAt", Value::str(record.created_at.clone()));
    o.set("doc", record.doc.redacted_value());
    o.set(
        "secrets",
        Value::Array(
            record
                .doc
                .secret_paths()
                .into_iter()
                .map(Value::str)
                .collect(),
        ),
    );
    Value::Object(o)
}

/// An app, with its secrets.
///
/// For the owner, who is the platform's own root credential and may see
/// what the platform holds. A program with a key may not.
fn record_value_with_secrets(record: &AppRecord) -> Value {
    let mut o = Object::new();
    o.set("id", Value::str(record.id.clone()));
    o.set("createdAt", Value::str(record.created_at.clone()));
    o.set("doc", record.doc.to_value());
    o.set(
        "secrets",
        Value::Array(
            record
                .doc
                .secret_paths()
                .into_iter()
                .map(Value::str)
                .collect(),
        ),
    );
    Value::Object(o)
}

/// Merge `changes` into `target`, one level deep for objects.
pub(crate) fn merge(target: &mut Value, changes: &Value) {
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
