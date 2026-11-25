//! The Nanobase HTTP API.
//!
//! Everything the command line can do is here, over HTTP, and the command
//! line is a client of it: an operator with `curl` and an operator with the
//! `nanobase` binary reach exactly the same operations, and a program that
//! embeds the platform reaches them too.
//!
//! The shape follows the rest of the platform:
//!
//! * `GET    /api/health` — is it up.
//! * `POST   /api/auth/login` — email and password in, a bearer token out.
//! * `GET    /api/auth/me` — who the token belongs to.
//! * `GET    /api/apps` — the apps.
//! * `POST   /api/apps` — make one.
//! * `GET    /api/apps/{id}` — one app, by id or name.
//! * `PUT    /api/apps/{id}` — replace its document.
//! * `PATCH  /api/apps/{id}` — change part of its document.
//! * `DELETE /api/apps/{id}` — remove it.
//! * `GET    /api/apps/{id}/methods` — the custom methods.
//! * `POST   /api/apps/{id}/methods/{name}` — call one.
//! * `GET    /api/apps/{id}/db` — the collections an app has.
//! * `GET    /api/apps/{id}/db/{collection}` — the rows of a collection.
//! * `POST   /api/apps/{id}/db/{collection}` — put a row in.
//! * `PATCH  /api/apps/{id}/db/{collection}` — change the rows that match.
//! * `DELETE /api/apps/{id}/db/{collection}` — take the rows that match out.
//! * `GET    /api/events` — the events a change fires.
//!
//! Every app operation goes through the service, so a change made over HTTP
//! fires the same events as one made from the command line, and reaches the
//! same hooks.

use std::sync::Arc;

use mosql::{Object, Value};

use crate::apikey::{self, ApiKey};
use crate::config::{AppDoc, DatabaseConfig};
use crate::http::{ApiError, Request, Response};
use crate::jwt;
use crate::middleware::{self, CorsPolicy, HttpOptions};
use crate::platform::{now_secs, verify_login, User, ROLE_OWNER, SESSION_TTL_SECS};
use crate::service::{Event, Service};

/// Where the API hangs off the host.
pub const API_PREFIX: &str = "/api";

/// How the API opens a database, for a connection test.
///
/// The API does not know where an app's files live, so the host supplies
/// the way in: it opens the configuration — in the app's own directory —
/// and says whether it answered.
pub type DbTester = Arc<dyn Fn(&str, &DatabaseConfig) -> Result<Vec<String>, String> + Send + Sync>;

/// Who is asking.
///
/// Two kinds of caller reach the platform: a person, who signed in and
/// holds a session token, and a program, which was given an API key. The
/// owner may do everything. An API key may do what its scopes say, and
/// may only touch the app it was made for.
#[derive(Debug, Clone)]
pub struct Principal {
    /// The owner the credential belongs to.
    pub user: User,
    /// The API key, when the credential is one.
    pub key: Option<ApiKey>,
}

impl Principal {
    /// May this caller do this?
    ///
    /// The owner may do everything. A key may do what its scopes say:
    /// `admin` is everything, `*` is everything, and an exact scope is
    /// that scope and nothing else.
    pub fn allows(&self, scope: &str) -> bool {
        match &self.key {
            None => true,
            Some(key) => key.allows(scope),
        }
    }

    /// The app this caller is limited to.
    ///
    /// A key belongs to exactly one app, so this is always an app for
    /// a key caller and never an app for a person.
    pub fn app_limit(&self) -> Option<&str> {
        self.key.as_ref().map(|key| key.app.as_str())
    }

    /// The role this caller's requests are judged by.
    ///
    /// The platform owner is above an app's rules, and so is a
    /// credential with the admin scope: the owner wrote the rules, and
    /// an admin credential is the platform's own root key. Every other
    /// credential is a `user`, and the app's rules decide what it may
    /// do.
    pub fn caller(&self) -> crate::acl::Caller {
        match &self.key {
            None => crate::acl::Caller::Owner,
            Some(key) if key.allows(apikey::SCOPE_ADMIN) => crate::acl::Caller::Admin,
            Some(key) => crate::acl::Caller::User { id: key.id.clone() },
        }
    }

    /// Refuse unless this caller may do this.
    pub fn require(&self, scope: &str, what: &str) -> Result<(), ApiError> {
        if self.allows(scope) {
            return Ok(());
        }
        Err(ApiError::forbidden(
            "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
            format!("this credential may not {what}"),
        ))
    }
}

/// The platform's API: a service, and the routes over it.
pub struct Api {
    service: Arc<Service>,
    options: HttpOptions,
    db_tester: Option<DbTester>,
    /// When the platform was last re-read from the store, so that a key
    /// which is not known does not make the server read its state on
    /// every request.
    reloaded_at: std::sync::atomic::AtomicU64,
}

impl Api {
    pub fn new(service: Arc<Service>) -> Self {
        Api {
            service,
            options: HttpOptions::default_closed(),
            db_tester: None,
            reloaded_at: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// An API that can test a database connection.
    pub fn with_db_tester(mut self, tester: DbTester) -> Self {
        self.db_tester = Some(tester);
        self
    }

    /// An API with a CORS policy and, perhaps, a rate limit.
    pub fn with_options(service: Arc<Service>, options: HttpOptions) -> Self {
        Api {
            service,
            options,
            db_tester: None,
            reloaded_at: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// The service the API is over.
    pub fn service(&self) -> &Arc<Service> {
        &self.service
    }

    /// Answer one request.
    ///
    /// The order matters: the rate limit is checked first, because a
    /// client that is over its allowance should not be able to make the
    /// server do work to say so. The security headers and the CORS
    /// headers are added last, because they must be on every answer —
    /// including the errors.
    pub fn handle(&self, req: Request) -> Response {
        // The keys of an older platform are moved into the apps they
        // belong to the first time somebody asks. A failure here is not
        // the request's fault: the keys wait, and the platform says so
        // when it starts.
        let _ = self.service.ensure_migrated();
        let cors = self.cors_for(&req);
        if let Some(limiter) = &self.options.rate_limit {
            if let Err(retry) = limiter.check(&req.client_ip()) {
                let response = Response::error(&ApiError::new(
                    429,
                    "TOO_MANY_REQUESTS",
                    "too many requests: slow down",
                ))
                .with_header("retry-after", &retry.seconds.to_string());
                return middleware::secure(cors.apply(&req, response), false);
            }
        }
        if req.method == "OPTIONS" {
            let response = cors.preflight(&req);
            return middleware::secure(response, false);
        }
        let response = match self.route(&req) {
            Ok(response) => response,
            Err(e) => Response::error(&e),
        };
        middleware::secure(cors.apply(&req, response), false)
    }

    /// The CORS policy that applies to a request.
    ///
    /// A request that names an app uses that app's own policy when it has
    /// one, and the platform's otherwise. An app may therefore open its
    /// API to a browser the platform has not been opened to, and the
    /// platform's policy is the floor.
    fn cors_for(&self, req: &Request) -> CorsPolicy {
        let platform = &self.options.cors;
        let key = app_key_of(req);
        let Some(key) = key else {
            return platform.clone();
        };
        let Ok(Some(app)) = self.service.find(key) else {
            return platform.clone();
        };
        // The app record holds the document under "doc".
        let Some(cors) = app
            .get("doc")
            .and_then(|d| d.get("config"))
            .and_then(|c| c.get("cors"))
        else {
            return platform.clone();
        };
        if !cors
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return platform.clone();
        }
        let origins: Vec<String> = cors
            .get("origins")
            .and_then(|v| v.as_array())
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        CorsPolicy {
            origins,
            allow_credentials: cors
                .get("allowCredentials")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            max_age: match cors.get("maxAge") {
                Some(Value::Int(seconds)) => (*seconds).max(0) as u64,
                _ => 600,
            },
        }
    }

    // ------------------------------------------------------------- routes --

    fn route(&self, req: &Request) -> Result<Response, ApiError> {
        let path = req.path.trim_end_matches('/');
        let path = if path.is_empty() { "/" } else { path };
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        match segments.as_slice() {
            ["api", "health"] => Ok(health()),
            ["api", "auth", "login"] => self.login(req),
            ["api", "auth", "me"] => {
                let principal = self.authenticate(req)?;
                let mut body = Object::new();
                body.set("user", principal.user.to_value());
                // The credential itself, so a client can tell a person
                // from a program without decoding a token.
                body.set(
                    "credential",
                    match &principal.key {
                        Some(key) => {
                            let mut c = Object::new();
                            c.set("type", Value::str("apiKey"));
                            c.set("id", Value::str(key.id.clone()));
                            c.set("name", Value::str(key.name.clone()));
                            Value::Object(c)
                        }
                        None => {
                            let mut c = Object::new();
                            c.set("type", Value::str("session"));
                            Value::Object(c)
                        }
                    },
                );
                Ok(json(200, &Value::Object(body)))
            }
            ["api", "events"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_APPS_READ, "read the platform's events")?;
                Ok(events())
            }
            ["api", "apps"] => match req.method.as_str() {
                "GET" => {
                    let principal = self.authenticate(req)?;
                    principal.require(apikey::SCOPE_APPS_READ, "read the apps")?;
                    self.list_apps(req, &principal)
                }
                "POST" => {
                    let principal = self.authenticate(req)?;
                    principal.require(apikey::SCOPE_APPS_WRITE, "create apps")?;
                    self.create_app(req)
                }
                _ => Err(method_not_allowed("GET, POST")),
            },
            ["api", "apps", key] => {
                let principal = self.authenticate(req)?;
                self.require_app_access(&principal, key)?;
                match req.method.as_str() {
                    "GET" => {
                        principal.require(apikey::SCOPE_APPS_READ, "read the apps")?;
                        self.show_app(req, key, &principal)
                    }
                    "PUT" => {
                        principal.require(apikey::SCOPE_APPS_WRITE, "change the apps")?;
                        self.update_app(req, key)
                    }
                    "PATCH" => {
                        principal.require(apikey::SCOPE_APPS_WRITE, "change the apps")?;
                        self.patch_app(req, key)
                    }
                    "DELETE" => {
                        principal.require(apikey::SCOPE_APPS_WRITE, "remove the apps")?;
                        self.remove_app(req, key)
                    }
                    _ => Err(method_not_allowed("GET, PUT, PATCH, DELETE")),
                }
            }
            ["api", "apps", key, "keys"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_ADMIN, "manage API keys")?;
                self.require_app_access(&principal, key)?;
                match req.method.as_str() {
                    "GET" => self.list_keys(key),
                    "POST" => self.create_key(req, key),
                    _ => Err(method_not_allowed("GET, POST")),
                }
            }
            ["api", "apps", key, "keys", key_id] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_ADMIN, "manage API keys")?;
                self.require_app_access(&principal, key)?;
                match req.method.as_str() {
                    "GET" => self.show_key(key, key_id),
                    "PATCH" => self.patch_key(req, key, key_id),
                    "DELETE" => self.remove_key(key, key_id),
                    _ => Err(method_not_allowed("GET, PATCH, DELETE")),
                }
            }
            ["api", "apps", key, "clone"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_APPS_WRITE, "create apps")?;
                self.require_app_access(&principal, key)?;
                if req.method != "POST" {
                    return Err(method_not_allowed("POST"));
                }
                self.require_app(key)?;
                self.clone_app(req, key)
            }
            ["api", "apps", key, "methods"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_APPS_READ, "read the apps")?;
                self.require_app_access(&principal, key)?;
                if req.method != "GET" {
                    return Err(method_not_allowed("GET"));
                }
                self.require_app(key)?;
                let mut body = Object::new();
                body.set(
                    "methods",
                    Value::Array(
                        self.service
                            .method_names()
                            .into_iter()
                            .map(Value::str)
                            .collect(),
                    ),
                );
                Ok(json(200, &Value::Object(body)))
            }
            ["api", "apps", key, "db"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_ROWS_READ, "read rows")?;
                self.require_app_access(&principal, key)?;
                if req.method != "GET" {
                    return Err(method_not_allowed("GET"));
                }
                self.require_app(key)?;
                self.collections(key)
            }
            ["api", "apps", key, "db", "test"] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_APPS_WRITE, "change the apps")?;
                self.require_app_access(&principal, key)?;
                self.test_database(req, key)
            }
            ["api", "apps", key, "db", collection] => {
                let principal = self.authenticate(req)?;
                self.require_app_access(&principal, key)?;
                match req.method.as_str() {
                    "GET" => {
                        principal.require(apikey::SCOPE_ROWS_READ, "read rows")?;
                        self.require_app(key)?;
                        self.rows(req, key, collection, &principal.caller())
                    }
                    "POST" => {
                        principal.require(apikey::SCOPE_ROWS_WRITE, "write rows")?;
                        self.require_app(key)?;
                        self.insert_row(req, key, collection, &principal.caller())
                    }
                    "PATCH" => {
                        principal.require(apikey::SCOPE_ROWS_WRITE, "write rows")?;
                        self.require_app(key)?;
                        self.update_row(req, key, collection, &principal.caller())
                    }
                    "DELETE" => {
                        principal.require(apikey::SCOPE_ROWS_WRITE, "write rows")?;
                        self.require_app(key)?;
                        self.remove_rows(req, key, collection, &principal.caller())
                    }
                    _ => Err(method_not_allowed("GET, POST, PATCH, DELETE")),
                }
            }
            ["api", "apps", key, "methods", name] => {
                let principal = self.authenticate(req)?;
                principal.require(apikey::SCOPE_APPS_WRITE, "call an app's methods")?;
                self.require_app_access(&principal, key)?;
                if req.method != "POST" {
                    return Err(method_not_allowed("POST"));
                }
                self.require_app(key)?;
                self.call_method(req, name)
            }
            ["api", "auth", "logout"] => {
                // Tokens are stateless; there is nothing to invalidate on the
                // server. The call is here so a client has a route to call.
                self.authenticate(req)?;
                Ok(json(200, &Value::Bool(true)))
            }
            _ => Err(ApiError::not_found("NOT_FOUND", "no such route")),
        }
    }

    // -------------------------------------------------------------- auth --

    /// Who is asking.
    ///
    /// The credential is a bearer token — which may be a session token or
    /// an API key — or, for a client that would rather not put the key
    /// in the `Authorization` header, the `X-Nanobase-Key` header.
    fn principal(&self, req: &Request) -> Result<Principal, ApiError> {
        let secret = req
            .bearer_token()
            .or_else(|| req.header("x-nanobase-key"))
            .ok_or_else(ApiError::unauthorized_plain)?;
        self.authenticate_secret(secret)
    }

    /// Check a credential and say who it belongs to.
    fn authenticate_secret(&self, secret: &str) -> Result<Principal, ApiError> {
        if apikey::looks_like_a_key(secret) {
            return self.authenticate_key(secret);
        }
        let user = self.authenticate_token(secret)?;
        Ok(Principal { user, key: None })
    }

    /// Check a session token and return the owner.
    fn authenticate_token(&self, token: &str) -> Result<User, ApiError> {
        let platform = self
            .service
            .platform()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        let claims = jwt::decode(token, platform.jwt_key(), now_secs())
            .map_err(|_| ApiError::unauthorized("INVALID_TOKEN", "the token is not valid"))?;
        if claims.role != ROLE_OWNER {
            return Err(ApiError::forbidden(
                "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
                "this token cannot manage the platform",
            ));
        }
        platform
            .owner()
            .filter(|owner| owner.id == claims.sub)
            .cloned()
            .ok_or_else(|| ApiError::unauthorized("INVALID_TOKEN", "the token is not valid"))
    }

    /// The key a secret belongs to, if it is one.
    ///
    /// The preview says which app to ask, and the app's own rows say
    /// whether the secret is the key. Two keys may share a preview, so
    /// every app the preview points at is asked.
    fn key_for(
        &self,
        preview: &str,
        secret: &str,
    ) -> Result<Option<crate::apikey::ApiKey>, ApiError> {
        let platform = self
            .service
            .platform()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        for (app, id) in platform.find_keys(preview) {
            match self.service.verify_key(&app, &id, secret) {
                Ok(Some(key)) => return Ok(Some(key)),
                Ok(None) => continue,
                Err(e) => return Err(ApiError::internal(format!("the key cannot be read: {e}"))),
            }
        }
        Ok(None)
    }

    /// Check an API key and say what it may do.
    ///
    /// The key belongs to an app and lives in that app's own database.
    /// The platform's directory says which app a presented key belongs
    /// to — the preview is the first few characters of the key, which
    /// the console shows anyway — and the key itself is read from that
    /// app's rows.
    ///
    /// The hash is compared in constant time, and the answer says
    /// whether the secret is this key, whether the key has been revoked
    /// and whether it has expired.
    fn authenticate_key(&self, secret: &str) -> Result<Principal, ApiError> {
        let platform = self
            .service
            .platform()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        let preview = apikey::preview_of(secret);
        let mut found = self.key_for(&preview, secret)?;
        if found.is_none() && self.may_reload() {
            // The key may have been made by the command line a moment
            // ago, in which case the server has not seen it yet.
            self.service
                .reload()
                .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
            found = self.key_for(&preview, secret)?;
        }
        let key = found
            .ok_or_else(|| ApiError::unauthorized("INVALID_KEY", "the API key is not valid"))?;
        if key.revoked {
            return Err(ApiError::unauthorized(
                "REVOKED_KEY",
                "this API key has been revoked",
            ));
        }
        if key.is_expired(now_secs()) {
            return Err(ApiError::unauthorized(
                "EXPIRED_KEY",
                "this API key has expired",
            ));
        }
        let user = platform
            .owner()
            .cloned()
            .ok_or_else(|| ApiError::unauthorized("INVALID_KEY", "the API key is not valid"))?;
        // Remember that the key was used, so a list can say when. The
        // write is at most one a minute: a key used a thousand times a
        // second should not write a thousand times a second.
        self.service.touch_key(&key.app, &key.id, now_secs())?;
        Ok(Principal {
            user,
            key: Some(key),
        })
    }

    /// Require a valid credential; the value is who is asking.
    fn authenticate(&self, req: &Request) -> Result<Principal, ApiError> {
        self.principal(req)
    }

    /// May the platform's state be read again just now?
    ///
    /// A key that is not known is usually a key that is wrong, and a
    /// wrong key should not make the server read its state on every
    /// request. One read a second is enough to pick up a key made by the
    /// command line and slow enough not to be a lever.
    fn may_reload(&self) -> bool {
        let now = now_secs().max(0) as u64;
        let last = self.reloaded_at.load(std::sync::atomic::Ordering::Relaxed);
        if now.saturating_sub(last) < 1 {
            return false;
        }
        self.reloaded_at
            .store(now, std::sync::atomic::Ordering::Relaxed);
        true
    }

    /// Refuse unless the caller may touch this app.
    ///
    /// A key made for one app may not touch another, whatever its
    /// scopes say.
    fn require_app_access(&self, principal: &Principal, key: &str) -> Result<(), ApiError> {
        let Some(limit) = principal.app_limit() else {
            return Ok(());
        };
        let platform = self
            .service
            .platform()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        let wanted = platform
            .find_app(key)
            .map(|record| record.id.clone())
            .unwrap_or_else(|| key.to_string());
        if wanted == limit || key == limit {
            return Ok(());
        }
        Err(ApiError::forbidden(
            "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
            "this API key may only be used with the app it was made for",
        ))
    }

    /// Sign in with an email and a password.
    fn login(&self, req: &Request) -> Result<Response, ApiError> {
        if req.method != "POST" {
            return Err(method_not_allowed("POST"));
        }
        let body = req.json()?;
        let email = body
            .get("email")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::invalid_request("email is required"))?
            .to_string();
        let password = body
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::invalid_request("password is required"))?
            .to_string();
        let platform = self
            .service
            .platform()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        let user = verify_login(&platform, &email, &password)
            .map_err(|_| {
                ApiError::unauthorized("INVALID_CREDENTIALS", "email or password is wrong")
            })?
            .clone();
        let token =
            crate::platform::issue_token(&user, platform.jwt_key(), now_secs(), SESSION_TTL_SECS);
        let mut out = Object::new();
        out.set("token", Value::str(token));
        out.set("user", user.to_value());
        Ok(json(200, &Value::Object(out)))
    }

    // ----------------------------------------------------------- app rows --

    /// The collections an app has.
    fn collections(&self, key: &str) -> Result<Response, ApiError> {
        let names = self.service.collections(key).map_err(db_error)?;
        let mut body = Object::new();
        body.set(
            "collections",
            Value::Array(names.into_iter().map(Value::str).collect()),
        );
        Ok(json(200, &Value::Object(body)))
    }

    /// The rows of a collection, filtered by the query string.
    ///
    /// `filter` is a JSON document, `limit` and `offset` are numbers.
    /// The app's rules decide which of the rows this caller may see.
    fn rows(
        &self,
        req: &Request,
        key: &str,
        collection: &str,
        caller: &crate::acl::Caller,
    ) -> Result<Response, ApiError> {
        let filter = filter_of(req)?;
        let limit = number_of(req, "limit")?;
        let offset = number_of(req, "offset")?;
        let rows = self
            .service
            .find_rows_as(caller, key, collection, &filter, limit, offset)
            .map_err(db_error)?;
        let total = self
            .service
            .count_rows_as(caller, key, collection, &filter)
            .map_err(db_error)?;
        let mut body = Object::new();
        body.set("items", Value::Array(rows));
        body.set("total", Value::Int(total as i64));
        Ok(json(200, &Value::Object(body)))
    }

    /// Put a row in a collection.
    fn insert_row(
        &self,
        req: &Request,
        key: &str,
        collection: &str,
        caller: &crate::acl::Caller,
    ) -> Result<Response, ApiError> {
        let body = req.json()?;
        let row = body.get("row").cloned().unwrap_or_else(|| body.clone());
        let inserted = self
            .service
            .insert_row_as(caller, key, collection, &row)
            .map_err(db_error)?;
        Ok(json(200, &inserted))
    }

    /// Change the rows that match a filter.
    fn update_row(
        &self,
        req: &Request,
        key: &str,
        collection: &str,
        caller: &crate::acl::Caller,
    ) -> Result<Response, ApiError> {
        let body = req.json()?;
        let filter = body
            .get("filter")
            .cloned()
            .unwrap_or_else(|| Value::Object(Object::new()));
        let changes = body
            .get("changes")
            .cloned()
            .ok_or_else(|| ApiError::invalid_request("changes is required"))?;
        let updated = self
            .service
            .update_row_as(caller, key, collection, &filter, &changes)
            .map_err(db_error)?;
        Ok(json(200, &updated))
    }

    /// Take the rows that match a filter out.
    fn remove_rows(
        &self,
        req: &Request,
        key: &str,
        collection: &str,
        caller: &crate::acl::Caller,
    ) -> Result<Response, ApiError> {
        let filter = filter_of(req)?;
        let removed = self
            .service
            .remove_rows_as(caller, key, collection, &filter)
            .map_err(db_error)?;
        let mut body = Object::new();
        body.set("removed", Value::Int(removed as i64));
        Ok(json(200, &Value::Object(body)))
    }

    // -------------------------------------------------------------- keys --

    /// An app's API keys, as values.
    ///
    /// The keys live in the app's own database, and the hash is never
    /// shown.
    fn list_keys(&self, app: &str) -> Result<Response, ApiError> {
        let mut items = self.service.list_keys(app).map_err(db_error)?;
        // Each key says which app it is for, by name as well as by id:
        // a list is read by a person, and a person wants to know which
        // app it is for.
        let name = self.app_name(app);
        for item in &mut items {
            if let Value::Object(row) = item {
                row.set("appName", Value::str(name.clone()));
            }
        }
        let mut body = Object::new();
        body.set("items", Value::Array(items.clone()));
        body.set("total", Value::Int(items.len() as i64));
        Ok(json(200, &Value::Object(body)))
    }

    /// One of an app's API keys.
    fn show_key(&self, app: &str, key: &str) -> Result<Response, ApiError> {
        let found = self.service.find_key(app, key).map_err(db_error)?;
        let mut row = crate::keys::to_public_row(&found);
        if let Value::Object(o) = &mut row {
            o.set("appName", Value::str(self.app_name(app)));
        }
        Ok(json(200, &row))
    }

    /// An app's name, for the answers that are read by a person.
    ///
    /// The id is the app's own, and the name is what the owner called
    /// it. An app that is not there has no name, and the id is shown
    /// instead.
    fn app_name(&self, app: &str) -> String {
        self.service
            .platform()
            .ok()
            .and_then(|platform| {
                platform
                    .find_app(app)
                    .map(|record| record.doc.app.name.clone())
            })
            .unwrap_or_else(|| app.to_string())
    }

    /// Make an API key.
    ///
    /// The key itself is in the answer and nowhere else: the platform
    /// keeps a hash of it, so this is the one time it can be read.
    fn create_key(&self, req: &Request, app: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| ApiError::invalid_request("a name is required"))?
            .to_string();
        let scopes: Vec<String> = match body.get("scopes") {
            Some(Value::Array(list)) => list
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Vec::new(),
        };
        if let Some(unknown) = apikey::first_unknown_scope(&scopes) {
            return Err(ApiError::invalid_request(format!(
                "there is no scope called {unknown}"
            )));
        }
        if scopes.is_empty() {
            return Err(ApiError::invalid_request("a key needs at least one scope"));
        }
        let expires_at = match body.get("expiresAt") {
            Some(Value::Int(seconds)) => Some(*seconds),
            Some(Value::Str(text)) => match text.trim().parse::<i64>() {
                Ok(seconds) => Some(seconds),
                Err(_) => {
                    return Err(ApiError::invalid_request(
                        "expiresAt must be a number of seconds since the epoch",
                    ))
                }
            },
            _ => None,
        };
        let (record, secret) = self
            .service
            .create_key(app, name, scopes, expires_at)
            .map_err(db_error)?;
        let mut out = Object::new();
        out.set("key", record);
        // The only time the key itself is ever written down.
        out.set("secret", Value::str(secret));
        Ok(json(201, &Value::Object(out)))
    }

    /// Change an API key: its name, its scopes, its expiry, or whether it
    /// is revoked.
    fn patch_key(&self, req: &Request, app: &str, key: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let updated = self.service.patch_key(app, key, &body).map_err(db_error)?;
        Ok(json(200, &updated))
    }

    /// Remove an API key.
    fn remove_key(&self, app: &str, key: &str) -> Result<Response, ApiError> {
        let removed = self.service.remove_key(app, key).map_err(db_error)?;
        Ok(json(200, &removed))
    }

    // -------------------------------------------------------------- apps --

    /// The apps, filtered and paged.
    ///
    /// `search` matches the name or the id, case-insensitively, so a
    /// console's search box has one thing to ask for. `page` is a page
    /// number and `perPage` how many apps are on it; `limit` and
    /// `offset` say the same thing the other way round, and are what the
    /// rows routes use.
    fn list_apps(&self, req: &Request, principal: &Principal) -> Result<Response, ApiError> {
        let mut apps = self
            .service
            .list()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        // A key made for one app sees one app. Not an error — the key is
        // working; it simply cannot see the rest.
        if let Some(limit) = principal.app_limit() {
            apps.retain(|app| {
                app.get("id").and_then(|v| v.as_str()) == Some(limit)
                    || app
                        .get("doc")
                        .and_then(|d| d.get("app"))
                        .and_then(|a| a.get("name"))
                        .and_then(|v| v.as_str())
                        == Some(limit)
            });
        }
        let search = req
            .query_param("search")
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());
        let matching: Vec<Value> = apps
            .into_iter()
            .filter(|app| match &search {
                Some(needle) => app_matches(app, needle),
                None => true,
            })
            .collect();
        let total = matching.len();
        let (page, per_page) = paging(req)?;
        let offset = match number_of(req, "offset")? {
            Some(offset) => offset as usize,
            None => (page - 1) * per_page,
        };
        let limit = match number_of(req, "limit")? {
            Some(limit) => limit as usize,
            None => per_page,
        };
        let items: Vec<Value> = matching.into_iter().skip(offset).take(limit).collect();
        let pages = if per_page == 0 {
            1
        } else {
            total.div_ceil(per_page).max(1)
        };
        let mut body = Object::new();
        body.set("items", Value::Array(items));
        body.set("total", Value::Int(total as i64));
        body.set("page", Value::Int(page as i64));
        body.set("perPage", Value::Int(per_page as i64));
        body.set("pages", Value::Int(pages as i64));
        Ok(json(200, &Value::Object(body)))
    }

    /// Try a database configuration and say whether it answered.
    ///
    /// The configuration in the body is the one to test, so a console can
    /// try a URL before it is saved. Without one, the app's own
    /// configuration is tested — which is what "is this app still
    /// connected?" means.
    fn test_database(&self, req: &Request, key: &str) -> Result<Response, ApiError> {
        let Some(tester) = &self.db_tester else {
            return Err(ApiError::internal(
                "this build cannot test a database connection",
            ));
        };
        let body = req.json()?;
        let given = body.get("database").filter(|value| !value.is_null());
        // The app's own configuration is the starting point: the
        // password is the platform's, and a client that was handed the
        // document without its secrets can still test the connection.
        // The fields the request names win.
        let stored = self.service.database_config(key).ok();
        let invalid =
            |e: String| ApiError::invalid_request(format!("the configuration is not valid: {e}"));
        let config = match (given, stored) {
            (Some(value), Some(stored)) => {
                let mut merged = stored.to_value();
                crate::service::merge(&mut merged, value);
                DatabaseConfig::from_value(&merged).map_err(invalid)?
            }
            (Some(value), None) => DatabaseConfig::from_value(value).map_err(invalid)?,
            (None, Some(stored)) => stored,
            (None, None) => {
                return Err(ApiError::not_found("NOT_FOUND", "no such app"));
            }
        };
        let mut body = Object::new();
        body.set("type", Value::str(config.kind.as_str()));
        match tester(key, &config) {
            Ok(collections) => {
                body.set("ok", Value::Bool(true));
                body.set(
                    "collections",
                    Value::Array(collections.into_iter().map(Value::str).collect()),
                );
                Ok(json(200, &Value::Object(body)))
            }
            Err(e) => {
                body.set("ok", Value::Bool(false));
                body.set("error", Value::str(e));
                Ok(json(200, &Value::Object(body)))
            }
        }
    }

    /// Fail with a 404 unless the app is there.
    fn require_app(&self, key: &str) -> Result<(), ApiError> {
        match self
            .service
            .find(key)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?
        {
            Some(_) => Ok(()),
            None => Err(ApiError::not_found("NOT_FOUND", "no such app")),
        }
    }

    /// One app.
    ///
    /// The document comes without its secrets — the database password,
    /// the token, the storage keys — because anybody who may read the
    /// app list may read every app's credentials. The owner may ask for
    /// them with `?includeSecrets=true`: the owner is the platform's own
    /// root credential. A program with a key may not, whatever it asks
    /// for.
    fn show_app(
        &self,
        req: &Request,
        key: &str,
        principal: &Principal,
    ) -> Result<Response, ApiError> {
        let wanted = wants_secrets(req);
        if wanted && principal.key.is_some() {
            return Err(ApiError::forbidden(
                "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
                "the secrets of an app are for the owner to see",
            ));
        }
        let app = if wanted {
            self.service
                .find_with_secrets(key)
                .map_err(|e| ApiError::internal(format!("platform: {e}")))?
        } else {
            self.service
                .find(key)
                .map_err(|e| ApiError::internal(format!("platform: {e}")))?
        }
        .ok_or_else(|| ApiError::not_found("NOT_FOUND", "no such app"))?;
        Ok(json(200, &app))
    }

    fn create_app(&self, req: &Request) -> Result<Response, ApiError> {
        let body = req.json()?;
        // A bare `{"name": "todos"}` is the common case; a full document is
        // accepted too.
        let doc = if body.get("app").is_some() || body.get("config").is_some() {
            AppDoc::from_value(&body).map_err(|e| {
                ApiError::new(
                    422,
                    "INVALID_DOCUMENT",
                    format!("the document is not valid: {e}"),
                )
            })?
        } else {
            let name = body
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| ApiError::invalid_request("name is required"))?;
            AppDoc::empty(name)
        };
        let record = self
            .service
            .create(doc)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        Ok(json(201, &record))
    }

    /// Copy an app.
    ///
    /// The body says what the copy is: a name, what happens to the rows
    /// — `omit`, `copy` or `share` — and, for a copy, the database the
    /// rows go in.
    fn clone_app(&self, req: &Request, key: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::invalid_request("name is required"))?;
        let data = match body.get("data").and_then(|v| v.as_str()) {
            Some(text) => crate::clone::CloneData::parse(text).ok_or_else(|| {
                ApiError::invalid_request("data must be one of: omit, copy, share")
            })?,
            None => crate::clone::CloneData::Omit,
        };
        let request = crate::clone::CloneRequest {
            name: name.to_string(),
            data,
            database: body
                .get("database")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            token: body
                .get("token")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        };
        let summary = self
            .service
            .clone_app(key, &request)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        Ok(json(201, &summary.to_value()))
    }

    fn update_app(&self, req: &Request, key: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let doc = AppDoc::from_value(&body).map_err(|e| {
            ApiError::new(
                422,
                "INVALID_DOCUMENT",
                format!("the document is not valid: {e}"),
            )
        })?;
        let record = self
            .service
            .update(key, doc)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        Ok(json(200, &record))
    }

    fn patch_app(&self, req: &Request, key: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let record = self
            .service
            .patch(key, &body)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        Ok(json(200, &record))
    }

    fn remove_app(&self, req: &Request, key: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let confirm = body
            .get("confirm")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ApiError::invalid_request("confirm is required"))?
            .to_string();
        let record = self
            .service
            .remove(key, &confirm)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
        Ok(json(200, &record))
    }

    // ----------------------------------------------------------- methods --

    fn call_method(&self, req: &Request, name: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let args: Vec<String> = body
            .get("args")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|v| match v {
                        Value::Str(s) => s.clone(),
                        other => mosql::to_json(other),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let value = self
            .service
            .call(name, &args)
            .map_err(|e| ApiError::internal(format!("{name}: {e}")))?;
        Ok(json(200, &value))
    }
}

// ----------------------------------------------------------------- pieces --

/// `GET /api/health`.
fn health() -> Response {
    let mut body = Object::new();
    body.set("status", Value::str("ok"));
    body.set("version", Value::str(crate::VERSION));
    json(200, &Value::Object(body))
}

/// `GET /api/events`: the events a successful change fires.
fn events() -> Response {
    let mut body = Object::new();
    body.set(
        "events",
        Value::Array(Event::all().iter().map(|e| Value::str(e.name())).collect()),
    );
    json(200, &Value::Object(body))
}

/// Does this app record match a search needle?
///
/// The name and the id, case-insensitively: those are the two things a
/// person types to find an app.
fn app_matches(app: &Value, needle: &str) -> bool {
    let id = app.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if id.to_ascii_lowercase().contains(needle) {
        return true;
    }
    let name = app
        .get("doc")
        .and_then(|d| d.get("app"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    name.to_ascii_lowercase().contains(needle)
}

/// The page and the page size a request asks for.
///
/// `page` is a page number, counting from one, and `perPage` how many
/// apps are on it. The defaults are a page of twenty-five and the first
/// page, which is what a console shows before anyone asks for anything
/// else.
fn paging(req: &Request) -> Result<(usize, usize), ApiError> {
    let page = match number_of(req, "page")? {
        Some(page) => page.max(1) as usize,
        None => 1,
    };
    let per_page = match number_of(req, "perPage")? {
        Some(per_page) => per_page.clamp(1, 200) as usize,
        None => 25,
    };
    Ok((page, per_page))
}

/// The app a route names, if it names one.
///
/// The CORS policy of an app is the app's own, so the middleware has to
/// know which app a request is about before it can answer it.
fn app_key_of(req: &Request) -> Option<&str> {
    let mut segments = req.path.split('/').filter(|s| !s.is_empty());
    match (segments.next(), segments.next(), segments.next()) {
        (Some("api"), Some("apps"), Some(key)) => Some(key),
        _ => None,
    }
}

fn json(status: u16, value: &Value) -> Response {
    Response::json(status, value)
}

/// The filter a request asks for: the `filter` query parameter, or the
/// `filter` field of the body.
///
/// Either place will do, so a script can send the filter in the query
/// string and a client that has a body can put it there.
fn filter_of(req: &Request) -> Result<Value, ApiError> {
    if let Some(text) = req.query_param("filter") {
        return mosql::parse_json(text)
            .map_err(|e| ApiError::invalid_request(format!("filter is not JSON: {e}")));
    }
    let body = req.json()?;
    match body.get("filter") {
        Some(filter) => Ok(filter.clone()),
        None => Ok(Value::Object(Object::new())),
    }
}

/// A database failure, as the API answers it.
///
/// The status comes from the error itself, so a filter that matches no row
/// is a 404 and not a failure of the engine.
fn db_error(e: uflite::error::Error) -> ApiError {
    match e.status() {
        404 => ApiError::not_found("NOT_FOUND", format!("database: {e}")),
        // A refusal by the app's own rules, not a failure of the
        // database: the answer says so.
        403 => ApiError::forbidden(
            "YOU_ARE_NOT_ALLOWED_TO_ACCESS_THIS",
            format!("the app's rules: {e}"),
        ),
        status => ApiError::new(status, "DATABASE_ERROR", format!("database: {e}")),
    }
}

/// Does the request ask for the app's secrets?
fn wants_secrets(req: &Request) -> bool {
    matches!(
        req.query_param("includeSecrets"),
        Some(value) if value.eq_ignore_ascii_case("true") || value == "1"
    )
}

/// A number from the query string.
fn number_of(req: &Request, name: &str) -> Result<Option<u64>, ApiError> {
    match req.query_param(name) {
        None => Ok(None),
        Some(text) => text
            .parse::<u64>()
            .map(Some)
            .map_err(|_| ApiError::invalid_request(format!("{name} is not a number"))),
    }
}

fn method_not_allowed(allowed: &str) -> ApiError {
    ApiError::new(405, "METHOD_NOT_ALLOWED", format!("allowed: {allowed}"))
}
