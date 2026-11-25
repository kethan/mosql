//! The rules, and who they let in.
//!
//! An app's rules live in its document, under `config.acl`: a list of
//! permissions and, optionally, a check. The evaluator is `uflite`'s —
//! the same one the JavaScript services use — so a rule written for one
//! means the same thing here.
//!
//! # The permission
//!
//! A permission is `role:resource:method`:
//!
//! * **role** — who is asking: `owner`, `admin`, `user`, or `*` for
//!   anybody;
//! * **resource** — which collection, or `*` for all of them;
//! * **method** — what they want: `list`, `view`, `create`, `update`,
//!   `delete`.
//!
//! A rule matches when one of its permissions matches and its check — a
//! filter over the row — accepts the row. Rules are OR-ed; a rule with
//! `"effect": "deny"` refuses whatever the others say, and nothing
//! matching means denied.
//!
//! # The check
//!
//! The check is a filter over the row the operation is about, so a rule
//! can say "the owner may read their own rows":
//!
//! ```json
//! { "id": "own", "permissions": ["user:*:view"], "check": { "owner": "{{user}}" } }
//! ```
//!
//! `{{user}}` and `{{role}}` in a check are replaced with the caller's
//! own values before the check is read, so the check is a plain filter
//! over the row by the time the matcher sees it.
//!
//! # Lists
//!
//! A list is not one decision but one per row: a row is in the answer
//! when a rule allows that row. So the rules filter a list rather than
//! hiding it, which is what makes a rule like "anyone may read a row
//! that is not private" useful.

use mosql::Value;
pub use uflite::acl::Acl;

use crate::config::AclConfig;

/// Who is asking for a row.
///
/// The role is what the rules are written against. A platform owner is
/// `owner`; an API key with the admin scope is `admin`; any other
/// credential is a `user`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// The platform owner.
    Owner,
    /// A credential with the admin scope.
    Admin,
    /// Any other authenticated caller.
    User {
        /// The caller's identifier, for a rule that checks it.
        id: String,
    },
    /// Nobody: no credential at all.
    Anonymous,
}

impl Caller {
    /// The role a rule is written against.
    pub fn role(&self) -> &'static str {
        match self {
            Caller::Owner => "owner",
            Caller::Admin => "admin",
            Caller::User { .. } => "user",
            Caller::Anonymous => "anonymous",
        }
    }

    /// The caller's identifier, if there is one.
    pub fn id(&self) -> Option<&str> {
        match self {
            Caller::User { id } => Some(id.as_str()),
            _ => None,
        }
    }
}

/// An app's rules.
///
/// The policy is built from the app's own configuration. The evaluator
/// is built per caller, because the caller's own values are what a
/// check may ask about — and building it is cheap: the rules are
/// parsed, not the rows.
pub struct Policy {
    config: AclConfig,
}

impl Policy {
    /// The policy for an app's configuration.
    ///
    /// A configuration with no rules, or with the rules switched off,
    /// allows everything: the app has not said otherwise.
    pub fn new(config: &AclConfig) -> Policy {
        Policy {
            config: config.clone(),
        }
    }

    /// The evaluator for this caller.
    ///
    /// `None` when the app has no rules, or has them switched off: the
    /// caller is let through without a decision, which is the difference
    /// between "no rules" and "rules that refuse this".
    pub fn evaluator(&self, caller: &Caller) -> Result<Option<Acl>, String> {
        if !self.config.enabled || self.config.rules.is_empty() {
            return Ok(None);
        }
        let rules: Vec<Value> = self
            .config
            .rules
            .iter()
            .map(|rule| substitute(rule, caller))
            .collect();
        let mut policy = mosql::Object::new();
        policy.set("enabled", Value::Bool(true));
        policy.set("rules", Value::Array(rules));
        Acl::new(&Value::Object(policy))
            .map(Some)
            .map_err(|e| e.to_string())
    }

    /// The permission a decision is about.
    pub fn permission(role: &str, collection: &str, method: &str) -> String {
        format!("{role}:{collection}:{method}")
    }

    /// Whether the caller is above the rules.
    ///
    /// The platform owner and a credential with the admin scope are not
    /// subject to an app's rules: the owner wrote them, and an admin
    /// credential is the platform's own root key. Every other caller is.
    pub fn is_superuser(caller: &Caller) -> bool {
        matches!(caller, Caller::Owner | Caller::Admin)
    }

    /// May this caller do this to this row?
    pub fn allows(
        acl: Option<&Acl>,
        caller: &Caller,
        collection: &str,
        method: &str,
        row: &Value,
    ) -> bool {
        let Some(acl) = acl else {
            return true;
        };
        if Policy::is_superuser(caller) {
            return true;
        }
        let permission = Policy::permission(caller.role(), collection, method);
        acl.can(row, &permission).unwrap_or(false)
    }

    /// The rows of a list the caller may see.
    ///
    /// A list is one decision per row: a row is in the answer when a
    /// rule allows that row.
    pub fn filter(
        acl: Option<&Acl>,
        caller: &Caller,
        collection: &str,
        method: &str,
        rows: Vec<Value>,
    ) -> Vec<Value> {
        let Some(acl) = acl else {
            return rows;
        };
        if Policy::is_superuser(caller) {
            return rows;
        }
        let permission = Policy::permission(caller.role(), collection, method);
        rows.into_iter()
            .filter(|row| acl.can(row, &permission).unwrap_or(false))
            .collect()
    }
}

/// Replace the caller's own values in a rule.
///
/// `{{user}}` is the caller's identifier and `{{role}}` is the role the
/// rule is written against. They are replaced wherever they appear as
/// a whole string, so a check is still a plain filter over the row by
/// the time the matcher sees it.
fn substitute(value: &Value, caller: &Caller) -> Value {
    match value {
        Value::Str(text) => match text.as_str() {
            "{{user}}" => match caller.id() {
                Some(id) => Value::str(id),
                None => Value::Null,
            },
            "{{role}}" => Value::str(caller.role()),
            _ => value.clone(),
        },
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| substitute(item, caller)).collect())
        }
        Value::Object(fields) => {
            let mut out = mosql::Object::new();
            for (name, item) in fields.iter() {
                out.set(name, substitute(item, caller));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(rules: &str) -> AclConfig {
        let parsed = mosql::parse_json(rules).unwrap();
        AclConfig {
            enabled: true,
            rules: parsed.as_array().unwrap().clone(),
        }
    }

    fn row(text: &str) -> Value {
        mosql::parse_json(text).unwrap()
    }

    #[test]
    fn an_app_with_no_rules_lets_everybody_in() {
        let policy = Policy::new(&AclConfig::default());
        let caller = Caller::User { id: "u1".into() };
        assert!(policy.evaluator(&caller).unwrap().is_none());
        assert!(Policy::allows(None, &caller, "todos", "list", &row("{}")));
    }

    #[test]
    fn a_rule_grants_one_role() {
        let policy = Policy::new(&config(
            r#"[{"id":"r1","permissions":["user:todos:list"]}]"#,
        ));
        let caller = Caller::User { id: "u1".into() };
        let acl = policy.evaluator(&caller).unwrap().unwrap();
        assert!(Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "list",
            &row("{}")
        ));
        assert!(!Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "delete",
            &row("{}")
        ));
        assert!(!Policy::allows(
            Some(&acl),
            &caller,
            "notes",
            "list",
            &row("{}")
        ));
        // The owner is above the rules, so this one is not refused.
        assert!(Policy::allows(
            Some(&acl),
            &Caller::Owner,
            "todos",
            "list",
            &row("{}")
        ));
    }

    #[test]
    fn a_wildcard_grants_every_role() {
        let policy = Policy::new(&config(r#"[{"id":"r1","permissions":["*:todos:*"]}]"#));
        let caller = Caller::Owner;
        let acl = policy.evaluator(&caller).unwrap().unwrap();
        assert!(Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "delete",
            &row("{}")
        ));
        assert!(Policy::allows(
            Some(&acl),
            &Caller::Anonymous,
            "todos",
            "list",
            &row("{}")
        ));
    }

    #[test]
    fn a_deny_beats_an_allow() {
        let policy = Policy::new(&config(
            r#"[{"id":"a","permissions":["*:todos:*"]},
                {"id":"d","permissions":["user:todos:delete"],"effect":"deny"}]"#,
        ));
        let caller = Caller::User { id: "u1".into() };
        let acl = policy.evaluator(&caller).unwrap().unwrap();
        assert!(Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "list",
            &row("{}")
        ));
        assert!(!Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "delete",
            &row("{}")
        ));
    }

    #[test]
    fn a_check_looks_at_the_row() {
        let policy = Policy::new(&config(
            r#"[{"id":"r1","permissions":["user:*:view"],"check":{"owner":"{{user}}"}}]"#,
        ));
        let caller = Caller::User { id: "u1".into() };
        let acl = policy.evaluator(&caller).unwrap().unwrap();
        assert!(Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "view",
            &row(r#"{"owner":"u1"}"#)
        ));
        assert!(!Policy::allows(
            Some(&acl),
            &caller,
            "todos",
            "view",
            &row(r#"{"owner":"u2"}"#)
        ));
    }

    #[test]
    fn a_list_is_filtered_row_by_row() {
        let policy = Policy::new(&config(
            r#"[{"id":"r1","permissions":["user:todos:list"],"check":{"public":true}}]"#,
        ));
        let caller = Caller::User { id: "u1".into() };
        let acl = policy.evaluator(&caller).unwrap().unwrap();
        let rows = vec![
            row(r#"{"id":"u1","public":true}"#),
            row(r#"{"id":"u2","public":false}"#),
        ];
        let kept = Policy::filter(Some(&acl), &caller, "todos", "list", rows);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].get("id").and_then(|v| v.as_str()), Some("u1"));
    }

    #[test]
    fn the_owner_is_above_the_rules() {
        let policy = Policy::new(&config(
            r#"[{"id":"r1","permissions":["user:todos:list"]}]"#,
        ));
        let owner = Caller::Owner;
        let acl = policy.evaluator(&owner).unwrap().unwrap();
        assert!(Policy::allows(
            Some(&acl),
            &owner,
            "todos",
            "list",
            &row("{}")
        ));
        // And so is an admin credential.
        assert!(Policy::allows(
            Some(&acl),
            &Caller::Admin,
            "todos",
            "list",
            &row("{}")
        ));
    }

    #[test]
    fn a_bad_policy_is_reported_rather_than_ignored() {
        let policy = Policy::new(&AclConfig {
            enabled: true,
            rules: vec![row(r#"{"permissions":["a:b"]}"#)],
        });
        let caller = Caller::Owner;
        assert!(policy.evaluator(&caller).is_err());
    }
}
