//! `services/acl.js` — policy validation and `can(context, permission)`.
//!
//! A policy is `{ enabled, rules: [{ id, name?, permissions, check? }] }`.
//! Permissions are `role:resource:method` strings; each segment may be `*`.
//! A rule matches when one of its permissions matches the asked one and its
//! optional `check` — a [uqry](crate::matcher) filter — accepts the context.
//!
//! Rules are OR-ed, with one exception: a rule with `"effect": "deny"` refuses
//! the permission no matter what any other rule says. Nothing matching means
//! denied. `enabled: false` grants everything. See [`Acl::decide`] for the
//! decision itself and [`Acl::can`] for the boolean form.
//!
//! Every validation message is the JavaScript `TypeError` text, and lands as
//! [`Error::BadRequest`] (the REST layer maps `TypeError` to 400).

use mosql::Value;

use crate::error::{Error, Result};
use crate::matcher;

/// What a rule does when it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Effect {
    /// Grant the permission. The default, and what uflite's own rules do.
    #[default]
    Allow,
    /// Refuse it, whatever the other rules say.
    Deny,
}

/// What [`Acl::decide`] concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// An `allow` rule matched and no `deny` rule did.
    Allow,
    /// A `deny` rule matched.
    Deny,
    /// No rule matched; the default is to deny.
    DefaultDeny,
}

impl Decision {
    /// Whether the caller may proceed.
    pub fn is_allowed(self) -> bool {
        matches!(self, Decision::Allow)
    }
}

/// One policy rule. `check` is evaluated with [`matcher::filter`].
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub id: String,
    pub name: Option<String>,
    pub permissions: Vec<String>,
    pub check: Option<Value>,
    pub effect: Effect,
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

    /// `can(context, permission)` — the boolean form of [`decide`](Acl::decide).
    pub fn can(&self, context: &Value, permission: &str) -> Result<bool> {
        Ok(self.decide(context, permission)?.is_allowed())
    }

    /// The decision itself, so a caller can tell "denied by a rule" from
    /// "denied because nothing granted it".
    ///
    /// A matching `deny` wins over any number of matching `allow` rules, and it
    /// short-circuits the scan. `Err` only when a rule `check` uses an operator
    /// uqry would throw on.
    pub fn decide(&self, context: &Value, permission: &str) -> Result<Decision> {
        if !self.enabled {
            return Ok(Decision::Allow);
        }
        if !is_permission(permission) {
            return Ok(Decision::DefaultDeny);
        }
        let mut allowed = false;
        for rule in &self.rules {
            if !rule
                .permissions
                .iter()
                .any(|p| matches_permission(p, permission))
            {
                continue;
            }
            let matched = match &rule.check {
                None => true,
                Some(check) => matcher::filter(check, Some(context))?,
            };
            if !matched {
                continue;
            }
            if rule.effect == Effect::Deny {
                return Ok(Decision::Deny);
            }
            allowed = true;
        }
        Ok(if allowed {
            Decision::Allow
        } else {
            Decision::DefaultDeny
        })
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
    // Absent means `allow`, so uflite's own policies mean what they always did.
    let effect = match ro.get("effect") {
        None => Effect::Allow,
        Some(Value::Str(s)) if s == "allow" => Effect::Allow,
        Some(Value::Str(s)) if s == "deny" => Effect::Deny,
        Some(_) => {
            return Err(Error::bad_request(
                "ACL rule effect must be 'allow' or 'deny'",
            ))
        }
    };
    Ok(Rule {
        id,
        name,
        permissions: perms,
        check,
        effect,
    })
}

impl Rule {
    /// Back to the JSON shape `Acl::new` accepts. `effect` is written only
    /// when it is `deny`, since `allow` is the default.
    pub fn to_value(&self) -> Value {
        let mut o = mosql::Object::new();
        o.set("id", Value::Str(self.id.clone()));
        if let Some(n) = &self.name {
            o.set("name", Value::Str(n.clone()));
        }
        o.set(
            "permissions",
            Value::Array(self.permissions.iter().cloned().map(Value::Str).collect()),
        );
        if let Some(c) = &self.check {
            o.set("check", c.clone());
        }
        if self.effect == Effect::Deny {
            o.set("effect", Value::str("deny"));
        }
        Value::Object(o)
    }
}

fn rule_to_value(r: Rule) -> Value {
    r.to_value()
}
