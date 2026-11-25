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

/// The platform's API: a service, and the routes over it.
pub struct Api {
    service: Arc<Service>,
    options: HttpOptions,
    db_tester: Option<DbTester>,
}

impl Api {
    pub fn new(service: Arc<Service>) -> Self {
        Api {
            service,
            options: HttpOptions::default_closed(),
            db_tester: None,
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
                let owner = self.owner(req)?;
                let mut body = Object::new();
                body.set("user", owner.to_value());
                Ok(json(200, &Value::Object(body)))
            }
            ["api", "events"] => {
                self.authenticate(req)?;
                Ok(events())
            }
            ["api", "apps"] => match req.method.as_str() {
                "GET" => {
                    self.authenticate(req)?;
                    self.list_apps(req)
                }
                "POST" => {
                    self.authenticate(req)?;
                    self.create_app(req)
                }
                _ => Err(method_not_allowed("GET, POST")),
            },
            ["api", "apps", key] => {
                self.authenticate(req)?;
                match req.method.as_str() {
                    "GET" => self.show_app(key),
                    "PUT" => self.update_app(req, key),
                    "PATCH" => self.patch_app(req, key),
                    "DELETE" => self.remove_app(req, key),
                    _ => Err(method_not_allowed("GET, PUT, PATCH, DELETE")),
                }
            }
            ["api", "apps", key, "methods"] => {
                self.authenticate(req)?;
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
                self.authenticate(req)?;
                if req.method != "GET" {
                    return Err(method_not_allowed("GET"));
                }
                self.require_app(key)?;
                self.collections(key)
            }
            ["api", "apps", key, "db", "test"] => {
                self.authenticate(req)?;
                self.test_database(req, key)
            }
            ["api", "apps", key, "db", collection] => {
                self.authenticate(req)?;
                self.require_app(key)?;
                match req.method.as_str() {
                    "GET" => self.rows(req, key, collection),
                    "POST" => self.insert_row(req, key, collection),
                    "PATCH" => self.update_row(req, key, collection),
                    "DELETE" => self.remove_rows(req, key, collection),
                    _ => Err(method_not_allowed("GET, POST, PATCH, DELETE")),
                }
            }
            ["api", "apps", key, "methods", name] => {
                self.authenticate(req)?;
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

    /// The owner the request's bearer token belongs to.
    fn owner(&self, req: &Request) -> Result<User, ApiError> {
        let token = req
            .bearer_token()
            .ok_or_else(ApiError::unauthorized_plain)?;
        self.authenticate_token(token)
    }

    /// Check the token and return the owner.
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

    /// Require a valid token; the value is the owner.
    fn authenticate(&self, req: &Request) -> Result<User, ApiError> {
        self.owner(req)
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
    fn rows(&self, req: &Request, key: &str, collection: &str) -> Result<Response, ApiError> {
        let filter = filter_of(req)?;
        let limit = number_of(req, "limit")?;
        let offset = number_of(req, "offset")?;
        let rows = self
            .service
            .find_rows(key, collection, &filter, limit, offset)
            .map_err(db_error)?;
        let total = self
            .service
            .count_rows(key, collection, &filter)
            .map_err(db_error)?;
        let mut body = Object::new();
        body.set("items", Value::Array(rows));
        body.set("total", Value::Int(total as i64));
        Ok(json(200, &Value::Object(body)))
    }

    /// Put a row in a collection.
    fn insert_row(&self, req: &Request, key: &str, collection: &str) -> Result<Response, ApiError> {
        let body = req.json()?;
        let row = body.get("row").cloned().unwrap_or_else(|| body.clone());
        let inserted = self
            .service
            .insert_row(key, collection, &row)
            .map_err(db_error)?;
        Ok(json(200, &inserted))
    }

    /// Change the rows that match a filter.
    fn update_row(&self, req: &Request, key: &str, collection: &str) -> Result<Response, ApiError> {
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
            .update_row(key, collection, &filter, &changes)
            .map_err(db_error)?;
        Ok(json(200, &updated))
    }

    /// Take the rows that match a filter out.
    fn remove_rows(
        &self,
        req: &Request,
        key: &str,
        collection: &str,
    ) -> Result<Response, ApiError> {
        let filter = filter_of(req)?;
        let removed = self
            .service
            .remove_rows(key, collection, &filter)
            .map_err(db_error)?;
        let mut body = Object::new();
        body.set("removed", Value::Int(removed as i64));
        Ok(json(200, &Value::Object(body)))
    }

    // -------------------------------------------------------------- apps --

    /// The apps, filtered and paged.
    ///
    /// `search` matches the name or the id, case-insensitively, so a
    /// console's search box has one thing to ask for. `page` is a page
    /// number and `perPage` how many apps are on it; `limit` and
    /// `offset` say the same thing the other way round, and are what the
    /// rows routes use.
    fn list_apps(&self, req: &Request) -> Result<Response, ApiError> {
        let apps = self
            .service
            .list()
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?;
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
        let config = match body.get("database") {
            Some(value) if !value.is_null() => DatabaseConfig::from_value(value).map_err(|e| {
                ApiError::invalid_request(format!("the configuration is not valid: {e}"))
            })?,
            _ => {
                let app = self
                    .service
                    .find(key)
                    .map_err(|e| ApiError::internal(format!("platform: {e}")))?
                    .ok_or_else(|| ApiError::not_found("NOT_FOUND", "no such app"))?;
                let value = app
                    .get("doc")
                    .and_then(|d| d.get("config"))
                    .and_then(|c| c.get("database"))
                    .cloned()
                    .unwrap_or(Value::Null);
                DatabaseConfig::from_value(&value).map_err(|e| {
                    ApiError::invalid_request(format!("the configuration is not valid: {e}"))
                })?
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

    fn show_app(&self, key: &str) -> Result<Response, ApiError> {
        let app = self
            .service
            .find(key)
            .map_err(|e| ApiError::internal(format!("platform: {e}")))?
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
        status => ApiError::new(status, "DATABASE_ERROR", format!("database: {e}")),
    }
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
