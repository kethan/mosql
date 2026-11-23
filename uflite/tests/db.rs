//! Port of uflite's `tests/db.spec.js` (minus the two `flite()` REST tests,
//! which become assertions on the error the REST layer serialises).
//!
//! The oracle used `umosql/memory` collections behind an explicit
//! `getCollection`; here a `MemoryCollection` is the provider directly.

mod common;

use common::*;
use mosql::Value;
use uflite::db::{Collection, DbService, MemoryCollection, Multi, Paginate};
use uflite::{DbOptions, Error, FindResult, Params};

fn service(name: &str, docs: &str, options: DbOptions) -> DbService<MemoryCollection> {
    DbService::new(MemoryCollection::new(name, common::docs(docs)), options)
}

const DEFAULT: fn() -> DbOptions = DbOptions::default;

// -- memory default ----------------------------------------------------------

#[test]
fn creates_an_isolated_in_memory_collection_when_no_adapter_is_provided() {
    run(async {
        let service = DbService::memory(vec![], DEFAULT());
        let other = DbService::memory(vec![], DEFAULT());
        let created = service
            .create(json(r#"{"name": "Ada", "active": true}"#), &Params::new())
            .await
            .unwrap();

        assert_eq!(created.get("name").unwrap().as_str(), Some("Ada"));
        let again = service
            .get(created.get("_id").unwrap(), &Params::new())
            .await
            .unwrap();
        expect_json(&again, r#"{"name": "Ada", "active": true, "_id": 1}"#);
        assert_eq!(page(other.find(&Params::new()).await.unwrap()).total, 0);
        assert_eq!(
            page(service.find(&params(r#"{"active": true}"#)).await.unwrap())
                .data
                .len(),
            1
        );
        let removed = service
            .remove(created.get("_id").unwrap(), &Params::new())
            .await
            .unwrap();
        assert_eq!(removed.get("name").unwrap().as_str(), Some("Ada"));
        assert_eq!(page(service.find(&Params::new()).await.unwrap()).total, 0);
    });
}

#[test]
fn accepts_seed_records_and_still_supports_explicit_providers() {
    run(async {
        let seeded = DbService::memory(docs(r#"[{"_id": "seed-1", "name": "Lin"}]"#), DEFAULT());
        expect_json(
            &seeded
                .get(&Value::str("seed-1"), &Params::new())
                .await
                .unwrap(),
            r#"{"_id": "seed-1", "name": "Lin"}"#,
        );

        // `initialData` must be an array: the Rust type system enforces it.
        let custom = service("custom", "[]", DEFAULT());
        assert_eq!(page(custom.find(&Params::new()).await.unwrap()).total, 0);
        let _ = seeded.options();
    });
}

#[test]
fn returns_http_404_for_a_missing_rest_record() {
    run(async {
        let users = DbService::memory(vec![], DEFAULT());
        let err = users
            .get(&Value::str("missing"), &Params::new())
            .await
            .expect_err("missing id");
        // The REST layer maps `error.status` and serialises the error.
        assert_eq!(err.status(), 404);
        expect_match(
            &Value::from(err),
            r#"{"status": 404, "code": 404, "name": "NotFound"}"#,
        );
    });
}

// -- find ----------------------------------------------------------------------

#[test]
fn find_returns_feathers_pagination() {
    run(async {
        let users = service(
            "db_find_pagination",
            r#"[
                {"_id": 1, "name": "Alice", "age": 30, "active": true},
                {"_id": 2, "name": "Bob", "age": 25, "active": true},
                {"_id": 3, "name": "Charlie", "age": 35, "active": false}
            ]"#,
            DEFAULT(),
        );
        let result = page(users.find(&Params::new()).await.unwrap());
        assert_eq!(result.total, 3);
        assert_eq!(result.limit, 10);
        assert_eq!(result.skip, 0);
        expect_docs(
            &result.data,
            r#"[
                {"_id": 1, "name": "Alice", "age": 30, "active": true},
                {"_id": 2, "name": "Bob", "age": 25, "active": true},
                {"_id": 3, "name": "Charlie", "age": 35, "active": false}
            ]"#,
        );
    });
}

#[test]
fn find_limit() {
    run(async {
        let users = service(
            "db_find_limit",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
            DEFAULT(),
        );
        let result = page(users.find(&params(r#"{"$limit": 2}"#)).await.unwrap());
        assert_eq!(result.total, 3);
        assert_eq!(result.limit, 2);
        assert_eq!(result.skip, 0);
        assert_eq!(result.data.len(), 2);
    });
}

#[test]
fn find_limit_is_capped_by_max() {
    run(async {
        let users = service(
            "db_find_limit_max",
            r#"[
                {"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"},
                {"_id": 3, "name": "Charlie"}, {"_id": 4, "name": "David"}
            ]"#,
            DbOptions::default().paginate(Paginate { default: 2, max: 2 }),
        );
        let result = page(users.find(&params(r#"{"$limit": 100}"#)).await.unwrap());
        assert_eq!(result.limit, 2);
        assert_eq!(result.data.len(), 2);
        assert_eq!(result.total, 4);
    });
}

#[test]
fn find_skip() {
    run(async {
        let users = service(
            "db_find_skip",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
            DEFAULT(),
        );
        let result = page(users.find(&params(r#"{"$skip": 1}"#)).await.unwrap());
        assert_eq!(result.total, 3);
        assert_eq!(result.skip, 1);
        expect_docs(
            &result.data,
            r#"[{"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
        );
    });
}

#[test]
fn find_skip_with_limit() {
    run(async {
        let users = service(
            "db_find_skip_limit",
            r#"[
                {"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"},
                {"_id": 3, "name": "Charlie"}, {"_id": 4, "name": "David"}
            ]"#,
            DEFAULT(),
        );
        let result = page(
            users
                .find(&params(r#"{"$skip": 1, "$limit": 2}"#))
                .await
                .unwrap(),
        );
        assert_eq!(result.total, 4);
        assert_eq!(result.skip, 1);
        assert_eq!(result.limit, 2);
        expect_docs(
            &result.data,
            r#"[{"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
        );
    });
}

#[test]
fn find_filters_records() {
    run(async {
        let users = service(
            "db_find_filter",
            r#"[
                {"_id": 1, "name": "Alice", "age": 30, "active": true},
                {"_id": 2, "name": "Bob", "age": 20, "active": false},
                {"_id": 3, "name": "Charlie", "age": 40, "active": true}
            ]"#,
            DEFAULT(),
        );
        let result = page(users.find(&params(r#"{"active": true}"#)).await.unwrap());
        assert_eq!(result.total, 2);
        expect_docs(
            &result.data,
            r#"[
                {"_id": 1, "name": "Alice", "age": 30, "active": true},
                {"_id": 3, "name": "Charlie", "age": 40, "active": true}
            ]"#,
        );
    });
}

#[test]
fn find_sort() {
    run(async {
        let users = service(
            "db_find_sort",
            r#"[
                {"_id": 1, "name": "Alice", "age": 30},
                {"_id": 2, "name": "Bob", "age": 20},
                {"_id": 3, "name": "Charlie", "age": 40}
            ]"#,
            DEFAULT(),
        );
        let result = page(
            users
                .find(&params(r#"{"$sort": {"age": 1}}"#))
                .await
                .unwrap(),
        );
        expect_docs(
            &result.data,
            r#"[
                {"_id": 2, "name": "Bob", "age": 20},
                {"_id": 1, "name": "Alice", "age": 30},
                {"_id": 3, "name": "Charlie", "age": 40}
            ]"#,
        );
    });
}

#[test]
fn find_select_string() {
    run(async {
        let users = service(
            "db_find_select_string",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "city": "Paris", "active": true}]"#,
            DEFAULT(),
        );
        let result = page(
            users
                .find(&params(r#"{"$select": "name age", "$limit": 10}"#))
                .await
                .unwrap(),
        );
        expect_json(&result.data[0], r#"{"_id": 1, "name": "Alice", "age": 30}"#);
    });
}

#[test]
fn find_select_array() {
    run(async {
        let users = service(
            "db_find_select_array",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "city": "Paris", "active": true}]"#,
            DEFAULT(),
        );
        let result = page(
            users
                .find(&params(r#"{"$select": ["name"], "$limit": 10}"#))
                .await
                .unwrap(),
        );
        expect_json(&result.data[0], r#"{"_id": 1, "name": "Alice"}"#);
    });
}

#[test]
fn find_select_excludes_id() {
    run(async {
        let users = service(
            "db_find_select_exclude_id",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "city": "Paris"}]"#,
            DEFAULT(),
        );
        let result = page(
            users
                .find(&params(r#"{"$select": {"name": 1, "age": 1, "_id": 0}}"#))
                .await
                .unwrap(),
        );
        expect_json(&result.data[0], r#"{"name": "Alice", "age": 30}"#);
    });
}

#[test]
fn find_paginate_false_returns_array() {
    run(async {
        let users = service(
            "db_find_no_pagination",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
            DEFAULT(),
        );
        let result = all(users.find(&Params::new().paginate(false)).await.unwrap());
        expect_docs(
            &result,
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
        );
    });
}

#[test]
fn find_limit_zero_returns_total_with_empty_data() {
    run(async {
        let users = service(
            "db_find_limit_zero",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}, {"_id": 3, "name": "Charlie"}]"#,
            DEFAULT(),
        );
        let result = page(users.find(&params(r#"{"$limit": 0}"#)).await.unwrap());
        assert_eq!(result.total, 3);
        assert_eq!(result.limit, 0);
        assert_eq!(result.skip, 0);
        assert!(result.data.is_empty());
    });
}

#[test]
fn find_skip_beyond_total_returns_empty_data() {
    run(async {
        let users = service(
            "db_find_skip_beyond",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}]"#,
            DEFAULT(),
        );
        let result = page(users.find(&params(r#"{"$skip": 10}"#)).await.unwrap());
        assert_eq!(result.total, 2);
        assert!(result.data.is_empty());
    });
}

// -- get ------------------------------------------------------------------------

#[test]
fn get_returns_record() {
    run(async {
        let users = service(
            "db_get",
            r#"[{"_id": 1, "name": "Alice", "age": 30}, {"_id": 2, "name": "Bob", "age": 25}]"#,
            DEFAULT(),
        );
        expect_json(
            &users.get(&Value::Int(2), &Params::new()).await.unwrap(),
            r#"{"_id": 2, "name": "Bob", "age": 25}"#,
        );
    });
}

#[test]
fn get_select() {
    run(async {
        let users = service(
            "db_get_select",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "city": "Paris"}]"#,
            DEFAULT(),
        );
        expect_json(
            &users
                .get(&Value::Int(1), &params(r#"{"$select": "name age"}"#))
                .await
                .unwrap(),
            r#"{"_id": 1, "name": "Alice", "age": 30}"#,
        );
    });
}

#[test]
fn get_missing_record_throws_404() {
    run(async {
        let users = service(
            "db_get_missing",
            r#"[{"_id": 1, "name": "Alice"}]"#,
            DEFAULT(),
        );
        let e = expect_404(users.get(&Value::Int(999), &Params::new()).await);
        assert_eq!(e.to_string(), "No record found for id '999'");
    });
}

// -- create ---------------------------------------------------------------------

#[test]
fn create_creates_record() {
    run(async {
        let users = service("db_create", "[]", DEFAULT());
        let result = users
            .create(json(r#"{"name": "Alice", "age": 30}"#), &Params::new())
            .await
            .unwrap();
        assert_eq!(result.get("name").unwrap().as_str(), Some("Alice"));
        assert_eq!(int(result.get("age").unwrap()), 30);
        assert!(result.get("_id").is_some());
        assert_eq!(users.provider().len(), 1);
    });
}

#[test]
fn create_rejects_multi_create() {
    run(async {
        let users = service(
            "db_create_multi_reject",
            "[]",
            DbOptions::default().multi(false),
        );
        let e = expect_err(
            users
                .create(
                    json(r#"[{"name": "Alice"}, {"name": "Bob"}]"#),
                    &Params::new(),
                )
                .await,
            "Multi create is not allowed",
        );
        assert_eq!(e.status(), 500);
        assert_eq!(users.provider().len(), 0);
    });
}

#[test]
fn create_allows_multi_create() {
    run(async {
        let users = service("db_create_multi", "[]", DbOptions::default().multi(true));
        let result = users
            .create(
                json(r#"[{"name": "Alice"}, {"name": "Bob"}]"#),
                &Params::new(),
            )
            .await
            .unwrap();
        let Value::Array(items) = &result else {
            panic!("multi create returns an array")
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].get("name").unwrap().as_str(), Some("Alice"));
        assert_eq!(items[1].get("name").unwrap().as_str(), Some("Bob"));
        assert_eq!(users.provider().len(), 2);
    });
}

// -- update ---------------------------------------------------------------------

#[test]
fn update_replaces_record() {
    run(async {
        let users = service(
            "db_update",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "city": "Paris", "active": true}]"#,
            DEFAULT(),
        );
        let result = users
            .update(
                &Value::Int(1),
                json(r#"{"name": "Alice Updated", "age": 31}"#),
                &Params::new(),
            )
            .await
            .unwrap();
        expect_json(&result, r#"{"_id": 1, "name": "Alice Updated", "age": 31}"#);
    });
}

#[test]
fn update_missing_record_throws_404() {
    run(async {
        let users = service(
            "db_update_missing",
            r#"[{"_id": 1, "name": "Alice"}]"#,
            DEFAULT(),
        );
        expect_404(
            users
                .update(
                    &Value::Int(999),
                    json(r#"{"name": "Missing"}"#),
                    &Params::new(),
                )
                .await,
        );
    });
}

// -- patch ----------------------------------------------------------------------

#[test]
fn patch_patches_record() {
    run(async {
        let users = service(
            "db_patch",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "active": true}]"#,
            DEFAULT(),
        );
        let result = users
            .patch(&Value::Int(1), &json(r#"{"age": 31}"#), &Params::new())
            .await
            .unwrap();
        expect_json(
            &result,
            r#"{"_id": 1, "name": "Alice", "age": 31, "active": true}"#,
        );
    });
}

#[test]
fn patch_supports_set() {
    run(async {
        let users = service(
            "db_patch_set",
            r#"[{"_id": 1, "name": "Alice", "age": 30, "active": true}]"#,
            DEFAULT(),
        );
        let result = users
            .patch(
                &Value::Int(1),
                &json(r#"{"$set": {"age": 31}}"#),
                &Params::new(),
            )
            .await
            .unwrap();
        expect_json(
            &result,
            r#"{"_id": 1, "name": "Alice", "age": 31, "active": true}"#,
        );
    });
}

#[test]
fn patch_missing_record_throws_404() {
    run(async {
        let users = service(
            "db_patch_missing",
            r#"[{"_id": 1, "name": "Alice"}]"#,
            DEFAULT(),
        );
        expect_404(
            users
                .patch(
                    &Value::Int(999),
                    &json(r#"{"name": "Missing"}"#),
                    &Params::new(),
                )
                .await,
        );
    });
}

#[test]
fn patch_rejects_multi_patch() {
    run(async {
        let users = service(
            "db_patch_multi_reject",
            r#"[{"_id": 1, "name": "Alice", "active": true}, {"_id": 2, "name": "Bob", "active": true}]"#,
            DbOptions::default().multi(Multi::none()),
        );
        expect_err(
            users
                .patch(
                    &Value::Null,
                    &json(r#"{"active": false}"#),
                    &params(r#"{"active": true}"#),
                )
                .await,
            "Multi patch is not allowed",
        );
        assert_eq!(
            users.provider().snapshot()[0]
                .get("active")
                .unwrap()
                .as_bool(),
            Some(true)
        );
    });
}

#[test]
fn multi_patch_updates_matching_records() {
    run(async {
        let users = service(
            "db_patch_multi",
            r#"[
                {"_id": 1, "name": "Alice", "active": true}, {"_id": 2, "name": "Bob", "active": true},
                {"_id": 3, "name": "Charlie", "active": false}, {"_id": 4, "name": "David", "active": true}
            ]"#,
            DbOptions::default().multi(true),
        );
        let result = users
            .patch(
                &Value::Null,
                &json(r#"{"$set": {"active": false}}"#),
                &params(r#"{"active": true}"#),
            )
            .await
            .unwrap();
        let Value::Array(items) = &result else {
            panic!("multi patch returns an array")
        };
        let mut ids: Vec<i64> = items.iter().map(|u| int(u.get("_id").unwrap())).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 2, 4]);
        let inactive = users
            .provider()
            .find(&json("{}"), None, Default::default())
            .await
            .unwrap()
            .into_iter()
            .filter(|u| u.get("active") == Some(&Value::Bool(false)))
            .count();
        assert_eq!(inactive, 4);
    });
}

// -- remove ---------------------------------------------------------------------

#[test]
fn remove_removes_and_returns_record() {
    run(async {
        let users = service(
            "db_remove",
            r#"[{"_id": 1, "name": "Alice"}, {"_id": 2, "name": "Bob"}]"#,
            DEFAULT(),
        );
        let result = users.remove(&Value::Int(1), &Params::new()).await.unwrap();
        expect_json(&result, r#"{"_id": 1, "name": "Alice"}"#);
        let remaining = users.provider().snapshot();
        assert_eq!(remaining.len(), 1);
        assert_eq!(int(remaining[0].get("_id").unwrap()), 2);
    });
}

#[test]
fn remove_missing_record_throws_404() {
    run(async {
        let users = service(
            "db_remove_missing",
            r#"[{"_id": 1, "name": "Alice"}]"#,
            DEFAULT(),
        );
        expect_404(users.remove(&Value::Int(999), &Params::new()).await);
    });
}

#[test]
fn remove_rejects_multi_remove() {
    run(async {
        let users = service(
            "db_remove_multi_reject",
            r#"[{"_id": 1, "name": "Alice", "active": true}, {"_id": 2, "name": "Bob", "active": true}]"#,
            DbOptions::default().multi(false),
        );
        expect_err(
            users
                .remove(&Value::Null, &params(r#"{"active": true}"#))
                .await,
            "Multi remove is not allowed",
        );
        assert_eq!(users.provider().len(), 2);
    });
}

#[test]
fn multi_remove_returns_removed_records() {
    run(async {
        let users = service(
            "db_remove_multi",
            r#"[
                {"_id": 1, "name": "Alice", "active": true}, {"_id": 2, "name": "Bob", "active": false},
                {"_id": 3, "name": "Charlie", "active": true}, {"_id": 4, "name": "David", "active": false}
            ]"#,
            DbOptions::default().multi(true),
        );
        let result = users
            .remove(&Value::Null, &params(r#"{"active": true}"#))
            .await
            .unwrap();
        let Value::Array(items) = &result else {
            panic!("multi remove returns an array")
        };
        assert_eq!(items.len(), 2);
        let mut ids: Vec<i64> = items.iter().map(|u| int(u.get("_id").unwrap())).collect();
        ids.sort();
        assert_eq!(ids, vec![1, 3]);
        let mut remaining: Vec<i64> = users
            .provider()
            .snapshot()
            .iter()
            .map(|u| int(u.get("_id").unwrap()))
            .collect();
        remaining.sort();
        assert_eq!(remaining, vec![2, 4]);
    });
}

// -- beyond the spec: error shape and options parsing ---------------------------

#[test]
fn not_found_error_serialises_to_the_rest_body() {
    let e = Error::record_not_found(&Value::str("x"));
    assert_eq!(e.status(), 404);
    expect_json(
        &Value::from(e),
        r#"{"name": "NotFound", "message": "No record found for id 'x'", "code": 404, "status": 404}"#,
    );
}

#[test]
fn options_parse_from_json_like_the_javascript_signature() {
    let o = DbOptions::from_value(&json(
        r#"{"idField": "id", "paginate": {"default": 5, "max": 50}, "multi": ["create", "remove"]}"#,
    ))
    .unwrap();
    assert_eq!(o.id_field, "id");
    assert_eq!(
        o.paginate,
        Paginate {
            default: 5,
            max: 50
        }
    );
    assert_eq!(
        o.multi,
        Multi {
            create: true,
            patch: false,
            remove: true
        }
    );
    assert_eq!(
        DbOptions::from_value(&json(r#"{"multi": {"patch": 1}}"#))
            .unwrap()
            .multi,
        Multi {
            create: false,
            patch: true,
            remove: false
        }
    );
    assert!(DbOptions::from_value(&json(r#"{"paginate": 5}"#)).is_err());
}

/// The crate-level doc example, verbatim, so it cannot drift.
#[test]
fn crate_doc_example() {
    async fn demo() -> Result<(), uflite::Error> {
        let docs = vec![
            mosql::parse_json(r#"{"_id": 1, "name": "Ada", "age": 36}"#).unwrap(),
            mosql::parse_json(r#"{"_id": 2, "name": "Bob", "age": 17}"#).unwrap(),
        ];
        let users = uflite::DbService::memory(docs, DbOptions::default());

        // Feathers pagination: { total, limit, skip, data }
        let adults = users
            .find(&Params::new().query_with(mosql::parse_json(
                r#"{"age": {"$gte": 18}, "$sort": {"age": -1}}"#,
            )?)?)
            .await?;
        let FindResult::Paginated(page) = &adults else {
            unreachable!()
        };
        assert_eq!(page.total, 1);
        assert_eq!(page.data[0].get("name").unwrap().as_str(), Some("Ada"));

        // create returns the stored document, with its generated _id
        let created = users
            .create(
                mosql::parse_json(r#"{"name": "Cyd", "age": 44}"#)?,
                &Params::new(),
            )
            .await?;
        assert!(created.get("_id").is_some());
        Ok(())
    }
    run(demo()).unwrap();
}
