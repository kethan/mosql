//! `params` — the per-call context uflite passes to every service method.
//!
//! In JavaScript `params` is an open object (`{ query, paginate, user, request }`).
//! The services in this crate only read `query`, `paginate` and (for storage
//! URLs) the request's base URL, so that is what [`Params`] carries.

use mosql::{Object, Value};

use crate::error::{Error, Result};

/// Per-call parameters. `query` holds the Mongo-style filter plus the
/// Feathers modifiers `$select`, `$sort`, `$limit`, `$skip`.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    /// Always an object; `{}` by default.
    pub query: Value,
    /// `Some(false)` turns pagination off for `find`. `None` means "use the
    /// service default", which is paginated.
    pub paginate: Option<bool>,
    /// Absolute URL of the incoming request, used by storage services to build
    /// download links. JavaScript reads this off `params.request.url`.
    pub base_url: Option<String>,
    /// The authenticated user, as uflite's hooks would attach it. Not read by
    /// any service here, but carried so adapters and hooks can use it.
    pub user: Option<Value>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            query: Value::object(),
            paginate: None,
            base_url: None,
            user: None,
        }
    }
}

impl Params {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the query. Non-object values are rejected the way the JavaScript
    /// services would misbehave on them — loudly, instead of silently.
    pub fn query(mut self, query: Value) -> Result<Self> {
        if !query.is_object() {
            return Err(Error::bad_request("params.query must be an object"));
        }
        self.query = query;
        Ok(self)
    }

    /// Merge extra keys into the query (for building `$select`/`$sort`/…).
    pub fn query_with(mut self, extra: Value) -> Result<Self> {
        let Value::Object(mut q) = std::mem::replace(&mut self.query, Value::Null) else {
            unreachable!("query is always an object");
        };
        let Value::Object(e) = extra else {
            return Err(Error::bad_request("query additions must be an object"));
        };
        for (k, v) in e.iter() {
            q.set(k.clone(), v.clone());
        }
        self.query = Value::Object(q);
        Ok(self)
    }

    pub fn paginate(mut self, on: bool) -> Self {
        self.paginate = Some(on);
        self
    }

    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    pub fn user(mut self, user: Value) -> Self {
        self.user = Some(user);
        self
    }

    /// `params.query.$select` etc. — read a modifier out of the query.
    pub fn modifier(&self, name: &str) -> Option<&Value> {
        self.query.get(name)
    }

    /// Build from the JSON shape a client would send:
    /// `{ "query": {...}, "paginate": false, "baseUrl": "...", "user": {...} }`.
    pub fn from_value(v: &Value) -> Result<Params> {
        if !v.is_object() {
            return Err(Error::bad_request("params must be an object"));
        }
        let mut p = Params::new();
        if let Some(q) = v.get("query") {
            if !q.is_null() {
                if !q.is_object() {
                    return Err(Error::bad_request("params.query must be an object"));
                }
                p.query = q.clone();
            }
        }
        match v.get("paginate") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(b)) => p.paginate = Some(*b),
            Some(_) => return Err(Error::bad_request("params.paginate must be a boolean")),
        }
        if let Some(u) = v.get("baseUrl").or_else(|| v.get("base_url")) {
            if !u.is_null() {
                match u.as_str() {
                    Some(s) => p.base_url = Some(s.to_string()),
                    None => return Err(Error::bad_request("params.baseUrl must be a string")),
                }
            }
        }
        if let Some(u) = v.get("user") {
            if !u.is_null() {
                p.user = Some(u.clone());
            }
        }
        Ok(p)
    }

    /// The query without the Feathers modifiers — what actually reaches the
    /// collection as a filter. (`cleanQuery` in db.js.)
    pub fn clean_query(&self) -> Value {
        let mut out = Object::new();
        if let Value::Object(q) = &self.query {
            for (k, v) in q.iter() {
                if matches!(k.as_str(), "$select" | "$sort" | "$limit" | "$skip") {
                    continue;
                }
                out.set(k.clone(), v.clone());
            }
        }
        Value::Object(out)
    }

    /// `$select` as a projection object, or `None`. (`toProjection` in db.js.)
    pub fn projection(&self) -> Option<Value> {
        to_projection(self.modifier("$select"))
    }
}

/// `toProjection`: whitespace-separated string or array of field names become
/// `{ field: 1 }`; an object is taken as-is; anything falsy means "no projection".
pub fn to_projection(value: Option<&Value>) -> Option<Value> {
    let v = value?;
    if !v.is_truthy() {
        return None;
    }
    match v {
        Value::Str(s) => {
            let mut o = Object::new();
            for field in s.split_whitespace() {
                o.set(field, Value::Int(1));
            }
            Some(Value::Object(o))
        }
        Value::Array(items) => {
            let mut o = Object::new();
            for field in items {
                o.set(field.to_js_string(), Value::Int(1));
            }
            Some(Value::Object(o))
        }
        other if other.is_truthy() => Some(other.clone()),
        _ => None,
    }
}
