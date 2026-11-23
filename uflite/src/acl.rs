//! `services/acl.js` — policy validation and `can(context, permission)`.
//!
//! A policy is `{ enabled, rules: [{ id, name?, permissions, check? }] }`.
//! Permissions are `role:resource:method` strings; each segment may be `*`.
//! A rule grants when one of its permissions matches the asked one and its
//! optional `check` — a [uqry](crate::matcher) filter — accepts the context.
//! Rules are OR-ed; `enabled: false` grants everything.
//!
//! Every validation message is the JavaScript `TypeError` text, and lands as
//! [`Error::BadRequest`] (the REST layer maps `TypeError` to 400).

use mosql::Value;

use crate::error::{Error, Result};
use crate::matcher;

/// One policy rule. `check` is evaluated with [`matcher::filter`].
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub name: Option<String>,
    pub permissions: Vec<String>,
    pub check: Option<Value>,
}

/// A validated policy.
#[derive(Debug, Clone, PartialEq)]
pub struct Acl {
    enabled: bool,
    rules: Vec<Rule>,
}

/// `a:b:c` — exactly three non-empty segments.
pub fn is_permission(value: &str) -> bool {
    let parts: Vec<&str> = value.split(':').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty())
}

/// Segment-wise wildcard match: `*` matches any segment. A segment missing
/// from either side (a malformed pattern) never matches, as in JavaScript.
pub fn matches_permission(pattern: &str, permission: &str) -> bool {
    let p: Vec<&str> = pattern.split(':').collect();
    let v: Vec<&str> = permission.split(':').collect();
    let seg = |i: usize| {
        p.get(i)
            .is_some_and(|p| *p == "*" || Some(*p) == v.get(i).copied())
    };
    seg(0) && seg(1) && seg(2)
}

impl Acl {
    /// `createACL(policy)`: validates, or returns the first `TypeError`.
    pub fn new(policy: &Value) -> Result<Acl> {
        let Some(po) = policy.as_object() else {
            return Err(Error::bad_request("Invalid ACL policy"));
        };
        let enabled = match po.get("enabled") {
            Some(Value::Bool(b)) => *b,
            _ => return Err(Error::bad_request("ACL enabled must be a boolean")),
        };
        let Some(Value::Array(rules)) = po.get("rules") else {
            return Err(Error::bad_request("ACL rules must be an array"));
        };
        let mut out = Vec::with_capacity(rules.len());
        for r in rules {
            out.push(validate_rule(r)?);
        }
        Ok(Acl {
            enabled,
            rules: out,
        })
    }

    /// Build directly from typed rules; same validation as [`Acl::new`].
    pub fn from_parts(enabled: bool, rules: Vec<Rule>) -> Result<Acl> {
        let mut o = mosql::Object::new();
        o.set("enabled", Value::Bool(enabled));
        o.set(
            "rules",
            Value::Array(rules.into_iter().map(rule_to_value).collect()),
        );
        Acl::new(&Value::Object(o))
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// `can(context, permission)`. `Err` only when a rule `check` uses an
    /// operator uqry would throw on.
    pub fn can(&self, context: &Value, permission: &str) -> Result<bool> {
        if !self.enabled {
            return Ok(true);
        }
        if !is_permission(permission) {
            return Ok(false);
        }
        for rule in &self.rules {
            if !rule
                .permissions
                .iter()
                .any(|p| matches_permission(p, permission))
            {
                continue;
            }
            match &rule.check {
                None => return Ok(true),
                Some(check) => {
                    if matcher::filter(check, Some(context))? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }
}

fn validate_rule(rule: &Value) -> Result<Rule> {
    let Some(ro) = rule.as_object() else {
        return Err(Error::bad_request("Invalid ACL rule"));
    };
    let id = match ro.get("id") {
        Some(Value::Str(s)) if !s.is_empty() => s.clone(),
        _ => return Err(Error::bad_request("ACL rule id is required")),
    };
    // `rule.name !== undefined && typeof rule.name !== 'string'`: present but
    // not a string (null included) is an error; absent is fine.
    let name = match ro.get("name") {
        None => None,
        Some(Value::Str(s)) => Some(s.clone()),
        Some(_) => return Err(Error::bad_request("ACL rule name must be a string")),
    };
    // JS: `!Array.isArray(rule.permissions) || !rule.permissions.length`
    let permissions = match ro.get("permissions") {
        Some(Value::Array(items)) if !items.is_empty() => items,
        _ => return Err(Error::bad_request("ACL rule permissions are required")),
    };
    let mut perms = Vec::with_capacity(permissions.len());
    for p in permissions {
        match p.as_str() {
            Some(s) if is_permission(s) => perms.push(s.to_string()),
            _ => return Err(Error::bad_request("Invalid ACL permission")),
        }
    }
    let check = match ro.get("check") {
        None => None,
        Some(c) if c.is_object() => Some(c.clone()),
        Some(_) => return Err(Error::bad_request("ACL rule check must be an object")),
    };
    Ok(Rule {
        id,
        name,
        permissions: perms,
        check,
    })
}

fn rule_to_value(r: Rule) -> Value {
    let mut o = mosql::Object::new();
    o.set("id", Value::Str(r.id));
    if let Some(n) = r.name {
        o.set("name", Value::Str(n));
    }
    o.set(
        "permissions",
        Value::Array(r.permissions.into_iter().map(Value::Str).collect()),
    );
    if let Some(c) = r.check {
        o.set("check", c);
    }
    Value::Object(o)
}
