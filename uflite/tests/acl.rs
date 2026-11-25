//! Port of uflite's `tests/acl.spec.js` (minus the one `flite()` hook test,
//! which exercises the service wrapper rather than the ACL).

mod common;

use common::{expect_err, json};
use mosql::Value;
use uflite::acl::{is_permission, matches_permission, Acl, Decision, Effect};

/// `acl(rules)` — an enabled policy with these rules.
fn acl(rules: &str) -> Acl {
    Acl::new(&json(&format!(r#"{{"enabled": true, "rules": {rules}}}"#))).expect("valid policy")
}

fn can(rules: &str, context: &str, permission: &str) -> bool {
    acl(rules).can(&json(context), permission).expect("can")
}

#[track_caller]
fn allow(rules: &str, context: &str, permission: &str) {
    assert!(
        can(rules, context, permission),
        "expected {permission} to be allowed"
    )
}

#[track_caller]
fn deny(rules: &str, context: &str, permission: &str) {
    assert!(
        !can(rules, context, permission),
        "expected {permission} to be denied"
    )
}

#[track_caller]
fn invalid(policy: &str) {
    assert!(
        Acl::new(&json(policy)).is_err(),
        "expected {policy} to be rejected"
    );
}

const ADMIN_DELETE: &str = r#"[{"id": "1", "permissions": ["admin:users:delete"]}]"#;
const OWNER: &str = r#"[{
    "id": "1", "name": "Post owner",
    "permissions": ["user:posts:update", "user:posts:remove"],
    "check": {"user.id": 10}
}]"#;

// -- basic authorization --------------------------------------------------------

#[test]
fn basic_authorization() {
    allow(ADMIN_DELETE, "{}", "admin:users:delete");
    deny(ADMIN_DELETE, "{}", "admin:users:update");
    deny(ADMIN_DELETE, "{}", "admin:posts:delete");
    deny(ADMIN_DELETE, "{}", "user:users:delete");
    deny("[]", "{}", "user:posts:read");
    deny(ADMIN_DELETE, "{}", "user:posts:read");
    allow(ADMIN_DELETE, "{}", "admin:users:delete");
}

#[test]
fn multiple_permissions_in_one_rule() {
    allow(OWNER, r#"{"user": {"id": 10}}"#, "user:posts:update");
    allow(OWNER, r#"{"user": {"id": 10}}"#, "user:posts:remove");
    deny(OWNER, r#"{"user": {"id": 10}}"#, "user:posts:create");
    // the same check applies to every listed permission
    deny(OWNER, r#"{"user": {"id": 20}}"#, "user:posts:update");
    deny(OWNER, r#"{"user": {"id": 20}}"#, "user:posts:remove");
    allow(
        r#"[{"id": "1", "permissions": ["user:posts:update", "user:posts:update"]}]"#,
        "{}",
        "user:posts:update",
    );
}

#[test]
fn multiple_rules_are_or_ed() {
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:delete"]},
            {"id": "2", "permissions": ["admin:users:update"]}]"#,
        "{}",
        "admin:users:update",
    );
    // keeps checking after a matching rule fails its check
    allow(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.role": "editor"}},
            {"id": "2", "permissions": ["user:posts:update"], "check": {"user.role": "owner"}}]"#,
        r#"{"user": {"role": "owner"}}"#,
        "user:posts:update",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.role": "editor"}},
            {"id": "2", "permissions": ["user:posts:update"], "check": {"user.role": "admin"}}]"#,
        r#"{"user": {"role": "user"}}"#,
        "user:posts:update",
    );
    // ignores rules for unrelated permissions (even with an impossible check)
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:delete"], "check": {"impossible": true}},
            {"id": "2", "permissions": ["user:posts:read"]}]"#,
        "{}",
        "user:posts:read",
    );
}

#[test]
fn wildcards() {
    allow(
        r#"[{"id": "1", "permissions": ["*:posts:read"]}]"#,
        "{}",
        "user:posts:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["*:posts:read"]}]"#,
        "{}",
        "admin:posts:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["admin:*:read"]}]"#,
        "{}",
        "admin:users:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["admin:*:read"]}]"#,
        "{}",
        "admin:posts:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:*"]}]"#,
        "{}",
        "admin:users:create",
    );
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:*"]}]"#,
        "{}",
        "admin:users:update",
    );
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:*"]}]"#,
        "{}",
        "admin:users:delete",
    );
    allow(
        r#"[{"id": "1", "permissions": ["*:*:*"]}]"#,
        "{}",
        "anything:anything:anything",
    );
    // no partial matching
    deny(
        r#"[{"id": "1", "permissions": ["admin:*:*"]}]"#,
        "{}",
        "user:users:delete",
    );
    deny(
        r#"[{"id": "1", "permissions": ["*:users:*"]}]"#,
        "{}",
        "admin:posts:delete",
    );
    deny(
        r#"[{"id": "1", "permissions": ["*:*:delete"]}]"#,
        "{}",
        "admin:users:update",
    );
}

#[test]
fn checks() {
    allow(
        r#"[{"id": "1", "permissions": ["admin:users:delete"], "check": {"user.role": "admin"}}]"#,
        r#"{"user": {"role": "admin"}}"#,
        "admin:users:delete",
    );
    deny(
        r#"[{"id": "1", "permissions": ["admin:users:delete"], "check": {"user.role": "admin"}}]"#,
        r#"{"user": {"role": "user"}}"#,
        "admin:users:delete",
    );
    allow(
        r#"[{"id": "1", "permissions": ["user:posts:read"], "check": {"request.user.role": "admin"}}]"#,
        r#"{"request": {"user": {"role": "admin"}}}"#,
        "user:posts:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.active": true}}]"#,
        r#"{"user": {"active": true}}"#,
        "user:posts:update",
    );
    allow(
        r#"[{"id": "1", "permissions": ["user:accounts:read"], "check": {"user.age": 25}}]"#,
        r#"{"user": {"age": 25}}"#,
        "user:accounts:read",
    );
    allow(
        r#"[{"id": "1", "permissions": ["user:users:update"], "check": {"user.deletedAt": null}}]"#,
        r#"{"user": {"deletedAt": null}}"#,
        "user:users:update",
    );
}

#[test]
fn expr_ownership() {
    let rule = r#"[{
        "id": "1", "permissions": ["user:posts:update"],
        "check": {"$expr": {"$eq": ["$user.id", "$post.authorId"]}}
    }]"#;
    allow(
        rule,
        r#"{"user": {"id": 10}, "post": {"authorId": 10}}"#,
        "user:posts:update",
    );
    deny(
        rule,
        r#"{"user": {"id": 10}, "post": {"authorId": 20}}"#,
        "user:posts:update",
    );
    // strict equality: 10 !== "10"
    deny(
        rule,
        r#"{"user": {"id": 10}, "post": {"authorId": "10"}}"#,
        "user:posts:update",
    );

    let combined = r#"[{
        "id": "1", "permissions": ["user:posts:update"],
        "check": {"$and": [
            {"$expr": {"$eq": ["$user.id", "$post.authorId"]}},
            {"user.active": true}
        ]}
    }]"#;
    allow(
        combined,
        r#"{"user": {"id": 10, "active": true}, "post": {"authorId": 10}}"#,
        "user:posts:update",
    );
    deny(
        combined,
        r#"{"user": {"id": 10, "active": false}, "post": {"authorId": 10}}"#,
        "user:posts:update",
    );
}

#[test]
fn real_world_authorization() {
    let rules = r#"[
        {"id": "admin", "name": "Admins", "permissions": ["admin:*:*"]},
        {"id": "owner", "name": "Post owners", "permissions": ["user:posts:update", "user:posts:remove"],
         "check": {"$expr": {"$eq": ["$user.id", "$post.authorId"]}}},
        {"id": "published", "name": "Published posts", "permissions": ["*:posts:read"],
         "check": {"post.published": true}}
    ]"#;
    allow(rules, "{}", "admin:users:create");
    allow(rules, "{}", "admin:users:update");
    allow(rules, "{}", "admin:users:delete");
    allow(rules, "{}", "admin:posts:update");
    allow(
        rules,
        r#"{"user": {"id": 10}, "post": {"authorId": 10}}"#,
        "user:posts:update",
    );
    allow(
        rules,
        r#"{"user": {"id": 10}, "post": {"authorId": 10}}"#,
        "user:posts:remove",
    );
    deny(
        rules,
        r#"{"user": {"id": 10}, "post": {"authorId": 20}}"#,
        "user:posts:update",
    );
    deny(
        rules,
        r#"{"user": {"id": 10}, "post": {"authorId": 20}}"#,
        "user:posts:remove",
    );
    allow(rules, r#"{"post": {"published": true}}"#, "user:posts:read");
    allow(
        rules,
        r#"{"post": {"published": true}}"#,
        "guest:posts:read",
    );
    deny(
        rules,
        r#"{"post": {"published": false}}"#,
        "user:posts:read",
    );
}

#[test]
fn request_data_validation() {
    let email = r#"[{"id": "1", "permissions": ["user:users:create"], "check": {"data.email": {"$exists": true}}}]"#;
    allow(
        email,
        r#"{"data": {"email": "a@example.com"}}"#,
        "user:users:create",
    );
    deny(email, r#"{"data": {}}"#, "user:users:create");

    let both = r#"[{"id": "1", "permissions": ["user:users:create"], "check": {"$and": [
        {"data.email": {"$exists": true}}, {"data.name": {"$exists": true}}]}}]"#;
    allow(
        both,
        r#"{"data": {"email": "a@example.com", "name": "Alice"}}"#,
        "user:users:create",
    );
    deny(
        both,
        r#"{"data": {"email": "a@example.com"}}"#,
        "user:users:create",
    );

    let no_password = r#"[{"id": "1", "permissions": ["user:users:update"], "check": {"data.password": {"$exists": false}}}]"#;
    allow(
        no_password,
        r#"{"data": {"name": "Alice"}}"#,
        "user:users:update",
    );
    deny(
        no_password,
        r#"{"data": {"name": "Alice", "password": "secret"}}"#,
        "user:users:update",
    );

    let separate = r#"[
        {"id": "normal", "permissions": ["user:users:update"], "check": {"data.password": {"$exists": false}}},
        {"id": "password", "permissions": ["user:users:password"], "check": {"data.password": {"$exists": true}}}
    ]"#;
    deny(
        separate,
        r#"{"data": {"password": "secret"}}"#,
        "user:users:update",
    );
    allow(
        separate,
        r#"{"data": {"password": "secret"}}"#,
        "user:users:password",
    );

    let protect = r#"[{"id": "1", "permissions": ["user:users:update"], "check": {"$and": [
        {"data.password": {"$exists": false}}, {"data.role": {"$exists": false}},
        {"data.isAdmin": {"$exists": false}}]}}]"#;
    allow(
        protect,
        r#"{"data": {"name": "Alice"}}"#,
        "user:users:update",
    );
    deny(
        protect,
        r#"{"data": {"role": "admin"}}"#,
        "user:users:update",
    );
    deny(
        protect,
        r#"{"data": {"isAdmin": true}}"#,
        "user:users:update",
    );
}

#[test]
fn context_edge_cases() {
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.role": "owner"}}]"#,
        "{}",
        "user:posts:update",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"post.authorId": 10}}]"#,
        r#"{"user": {"id": 10}}"#,
        "user:posts:update",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"post.authorId": 10}}]"#,
        r#"{"post": null}"#,
        "user:posts:update",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.active": true}}]"#,
        r#"{"user": {"active": false}}"#,
        "user:posts:update",
    );
    allow(
        r#"[{"id": "1", "permissions": ["user:posts:update"], "check": {"user.id": 0}}]"#,
        r#"{"user": {"id": 0}}"#,
        "user:posts:update",
    );
}

#[test]
fn enabled_flag() {
    let on = Acl::new(&json(r#"{"enabled": true, "rules": []}"#)).unwrap();
    assert!(!on.can(&Value::object(), "user:posts:read").unwrap());
    let off = Acl::new(&json(r#"{"enabled": false, "rules": []}"#)).unwrap();
    assert!(off.can(&Value::object(), "user:posts:read").unwrap());
    assert!(!off.enabled());
    let off_rules = Acl::new(&json(
        r#"{"enabled": false, "rules": [{"id": "1", "permissions": ["admin:users:delete"]}]}"#,
    ))
    .unwrap();
    assert!(off_rules.can(&Value::object(), "user:posts:read").unwrap());
    assert_eq!(off_rules.rules().len(), 1);
}

#[test]
fn permission_input() {
    let rules = r#"[{"id": "1", "permissions": ["user:posts:read"]}]"#;
    // undefined / null cannot be passed as a &str; the empty string and the
    // malformed shapes can.
    deny(rules, "{}", "");
    deny(rules, "{}", "user:posts");
    deny(rules, "{}", "user:posts:read:extra");
    deny(rules, "{}", ":posts:read");
    deny(rules, "{}", "user::read");
    deny(rules, "{}", "user:posts:");
    allow(rules, "{}", "user:posts:read");
}

#[test]
fn permission_helpers() {
    assert!(is_permission("a:b:c"));
    assert!(is_permission("*:*:*"));
    assert!(!is_permission("a:b"));
    assert!(!is_permission("a:b:c:d"));
    assert!(!is_permission("a::c"));
    assert!(!is_permission(""));
    assert!(matches_permission("*:*:*", "a:b:c"));
    assert!(matches_permission("a:*:c", "a:b:c"));
    assert!(!matches_permission("a:b:c", "a:b:d"));
    // A malformed *pattern* (validated away by `new`) would index undefined.
    assert!(!matches_permission("a", "a:b:c"));
}

#[test]
fn policy_validation() {
    invalid("null");
    invalid(r#"[]"#);
    invalid(r#""policy""#);
    invalid(r#"{"enabled": "true", "rules": []}"#);
    invalid(r#"{"rules": []}"#);
    invalid(r#"{"enabled": true, "rules": {}}"#);
    invalid(r#"{"enabled": true, "rules": [{"permissions": ["user:posts:read"]}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": "", "permissions": ["user:posts:read"]}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": 1, "permissions": ["user:posts:read"]}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": "1", "permissions": []}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": "1", "permissions": "user:posts:read"}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["user:posts"]}]}"#);
    invalid(r#"{"enabled": true, "rules": [{"id": "1", "permissions": [5]}]}"#);
    invalid(
        r#"{"enabled": true, "rules": [{"id": "1", "name": 123, "permissions": ["*:posts:read"]}]}"#,
    );
    invalid(
        r#"{"enabled": true, "rules": [{"id": "1", "name": null, "permissions": ["*:posts:read"]}]}"#,
    );
    invalid(
        r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["*:posts:read"], "check": "invalid"}]}"#,
    );
    invalid(
        r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["*:posts:read"], "check": null}]}"#,
    );
    invalid(
        r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["*:posts:read"], "check": []}]}"#,
    );
    invalid(r#"{"enabled": true, "rules": ["invalid"]}"#);
    invalid(r#"{"enabled": true, "rules": [null]}"#);

    // Optional name is fine.
    Acl::new(&json(
        r#"{"enabled": true, "rules": [{"id": "1", "name": "Read posts", "permissions": ["*:posts:read"]}]}"#,
    ))
    .unwrap();

    // Messages are the JavaScript TypeErrors.
    let e = expect_err(
        Acl::new(&json(
            r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["a:b"]}]}"#,
        )),
        "Invalid ACL permission",
    );
    assert_eq!(e.status(), 400);
    assert_eq!(e.name(), "TypeError");
    expect_err(Acl::new(&json("null")), "Invalid ACL policy");
    expect_err(
        Acl::new(&json(r#"{"enabled": 1, "rules": []}"#)),
        "ACL enabled must be a boolean",
    );
    expect_err(
        Acl::new(&json(r#"{"enabled": true, "rules": {}}"#)),
        "ACL rules must be an array",
    );
    expect_err(
        Acl::new(&json(r#"{"enabled": true, "rules": [1]}"#)),
        "Invalid ACL rule",
    );
    expect_err(
        Acl::new(&json(
            r#"{"enabled": true, "rules": [{"permissions": ["a:b:c"]}]}"#,
        )),
        "ACL rule id is required",
    );
    expect_err(
        Acl::new(&json(r#"{"enabled": true, "rules": [{"id": "1"}]}"#)),
        "ACL rule permissions are required",
    );
    expect_err(
        Acl::new(&json(
            r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["a:b:c"], "name": 1}]}"#,
        )),
        "ACL rule name must be a string",
    );
    expect_err(
        Acl::new(&json(
            r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["a:b:c"], "check": 1}]}"#,
        )),
        "ACL rule check must be an object",
    );
}

#[test]
fn security_fail_closed() {
    deny(
        r#"[{"id": "1", "permissions": ["admin:users:delete"], "check": {"user.role": "admin"}}]"#,
        r#"{"user": {"role": "user", "permission": "admin:users:delete"}}"#,
        "user:users:delete",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:update"],
            "check": {"$expr": {"$eq": ["$user.id", "$post.authorId"]}}}]"#,
        r#"{"user": {"id": 10}, "post": {"authorId": 20}}"#,
        "user:posts:update",
    );
    deny(
        r#"[{"id": "1", "permissions": ["user:posts:read"]}]"#,
        "{}",
        "user:posts:delete",
    );
}

#[test]
fn rule_metadata() {
    allow(
        r#"[{"id": "1", "name": "Anything", "permissions": ["user:posts:read"]}]"#,
        "{}",
        "user:posts:read",
    );
    let first = acl(r#"[{"id": "1", "name": "First", "permissions": ["user:posts:read"]}]"#);
    let second =
        acl(r#"[{"id": "1", "name": "Completely Different", "permissions": ["user:posts:read"]}]"#);
    assert!(first.can(&Value::object(), "user:posts:read").unwrap());
    assert!(second.can(&Value::object(), "user:posts:read").unwrap());
    assert_eq!(first.rules()[0].name.as_deref(), Some("First"));
    assert_eq!(
        second.rules()[0].name.as_deref(),
        Some("Completely Different")
    );
}

#[test]
fn typed_construction_matches_json_construction() {
    let typed = Acl::from_parts(
        true,
        vec![uflite::acl::Rule {
            id: "1".into(),
            name: None,
            permissions: vec!["admin:users:delete".into()],
            check: Some(json(r#"{"user.role": "admin"}"#)),
            effect: uflite::acl::Effect::Allow,
        }],
    )
    .unwrap();
    let from_json = Acl::new(&json(
        r#"{"enabled": true, "rules": [{"id": "1", "permissions": ["admin:users:delete"], "check": {"user.role": "admin"}}]}"#,
    ))
    .unwrap();
    assert_eq!(typed, from_json);
    assert!(typed
        .can(
            &json(r#"{"user": {"role": "admin"}}"#),
            "admin:users:delete"
        )
        .unwrap());
    assert!(!typed
        .can(&json(r#"{"user": {"role": "user"}}"#), "admin:users:delete")
        .unwrap());
    assert!(Acl::from_parts(
        true,
        vec![uflite::acl::Rule {
            id: String::new(),
            name: None,
            permissions: vec![],
            check: None,
            effect: uflite::acl::Effect::Allow,
        }]
    )
    .is_err());
}

#[test]
fn rule_without_effect_is_allow() {
    let acl = Acl::new(&json(
        r#"{"enabled":true,"rules":[{"id":"r1","permissions":["user:posts:*"]}]}"#,
    ))
    .expect("valid policy");
    assert_eq!(acl.rules()[0].effect, Effect::Allow);
}

#[test]
fn deny_overrides_allow() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"],"check":{"user.premium":true}},
        {"id":"b","permissions":["user:posts:*"],"effect":"deny","check":{"user.banned":true}}
    ]"#);
    let ctx = json(r#"{"user":{"premium":true,"banned":true}}"#);
    assert_eq!(acl.decide(&ctx, "user:posts:find").unwrap(), Decision::Deny);
    assert!(!acl.can(&ctx, "user:posts:find").unwrap());
}

#[test]
fn allow_wins_when_deny_does_not_match() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"],"check":{"user.premium":true}},
        {"id":"b","permissions":["user:posts:*"],"effect":"deny","check":{"user.banned":true}}
    ]"#);
    let ctx = json(r#"{"user":{"premium":true,"banned":false}}"#);
    assert_eq!(
        acl.decide(&ctx, "user:posts:find").unwrap(),
        Decision::Allow
    );
    assert!(acl.can(&ctx, "user:posts:find").unwrap());
}

#[test]
fn default_is_deny() {
    let acl = acl(r#"[
        {"id":"a","permissions":["admin:users:*"]}
    ]"#);
    assert_eq!(
        acl.decide(&json("{}"), "user:posts:find").unwrap(),
        Decision::DefaultDeny
    );
    assert!(!acl.can(&json("{}"), "user:posts:find").unwrap());
}

#[test]
fn deny_rule_with_check_not_matching_grants() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"],"check":{"user.premium":true}},
        {"id":"b","permissions":["user:posts:*"],"effect":"deny","check":{"user.banned":true}}
    ]"#);
    let ctx = json(r#"{"user":{"premium":true,"banned":false}}"#);
    assert!(acl.can(&ctx, "user:posts:find").unwrap());
}

#[test]
fn deny_without_check_blocks_every_matching_permission() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"]},
        {"id":"b","permissions":["user:posts:*"],"effect":"deny"}
    ]"#);
    assert!(!acl.can(&json("{}"), "user:posts:find").unwrap());
    assert!(!acl.can(&json("{}"), "user:posts:create").unwrap());
}

#[test]
fn invalid_effect_is_rejected() {
    assert!(Acl::new(&json(
        r#"{"enabled":true,"rules":[{"id":"a","permissions":["a:b:c"],"effect":"maybe"}]}"#
    ))
    .is_err());
}

#[test]
fn deny_round_trips_through_json() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"],"effect":"deny"}
    ]"#);
    assert_eq!(acl.rules()[0].effect, Effect::Deny);
    assert_eq!(
        acl.rules()[0].to_value().get("effect"),
        Some(&json("\"deny\""))
    );
}

#[test]
fn allow_rule_omits_effect_when_serialised() {
    let acl = acl(r#"[
        {"id":"a","permissions":["user:posts:*"]}
    ]"#);
    assert_eq!(acl.rules()[0].to_value().get("effect"), None);
}
