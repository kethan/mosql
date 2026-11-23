//! Compile-and-run checks for the code shown in `src/lib.rs` and `README.md`.
//!
//! `Cargo.toml` sets `doctest = false` because this crate must build in
//! environments without `rustdoc` (the wasm toolchain has none). That would
//! normally let a broken example ship unnoticed, so every documented snippet is
//! duplicated here as a real test.

use mosql::memory::Collection;
use mosql::{parse_json, Builder, CrudOptions, Db, Value};

/// The example in the crate-level doc comment.
#[test]
fn crate_doc_example() {
    let query = parse_json(r#"{"age": {"$gte": 18}}"#).unwrap();
    let sql = Builder::full()
        .collection("users", Db::Pg)
        .unwrap()
        .find(&query, None)
        .unwrap()
        .sort(&parse_json(r#"{"age": -1}"#).unwrap())
        .unwrap()
        .limit(&Value::Int(10))
        .unwrap()
        .to_sql()
        .unwrap();

    assert_eq!(
        sql,
        "SELECT * FROM users WHERE age >= 18 ORDER BY age DESC NULLS LAST LIMIT 10"
    );
}

/// The in-memory engine example from the README.
#[test]
fn memory_engine_example() {
    let query = parse_json(r#"{"age": {"$gte": 18}}"#).unwrap();
    let coll = Collection::new(
        "users",
        vec![
            parse_json(r#"{"name": "Ada", "age": 36}"#).unwrap(),
            parse_json(r#"{"name": "Bob", "age": 17}"#).unwrap(),
        ],
        Default::default(),
    );

    let hits = coll
        .find(&query)
        .sort(&parse_json(r#"{"age": -1}"#).unwrap())
        .limit(2)
        .to_vec()
        .unwrap();

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].get("name").unwrap().as_str(), Some("Ada"));
}

/// Every chain method returns `Result`, so `?` works at every step.
#[test]
fn chain_is_uniform() -> Result<(), mosql::Error> {
    let query = parse_json(r#"{"age": {"$gte": 18}}"#)?;
    let sort = parse_json(r#"{"age": -1}"#)?;

    let sql = Builder::full()
        .collection("users", Db::Mysql)?
        .find(&query, None)?
        .sort(&sort)?
        .skip(&Value::Int(5))?
        .limit(&Value::Int(10))?
        .to_sql()?;

    assert!(
        sql.starts_with("SELECT * FROM users WHERE age >= 18"),
        "{sql}"
    );

    let counted = Builder::full()
        .collection("users", Db::Sqlite)?
        .find(&query, None)?
        .count()?
        .to_sql()?;
    assert!(counted.contains("COUNT(*) AS count"), "{counted}");

    let unique = Builder::full()
        .collection("users", Db::Sqlite)?
        .find(&query, None)?
        .distinct()?
        .to_sql()?;
    assert!(
        unique.starts_with("SELECT DISTINCT * FROM users"),
        "{unique}"
    );

    Ok(())
}

/// The three allowlist variants all build and differ where expected.
#[test]
fn variants_build() -> Result<(), mosql::Error> {
    let query = parse_json(r#"{"age": {"$gte": 18}}"#)?;
    for b in [Builder::full(), Builder::lite(), Builder::tiny()] {
        let sql = b
            .collection("users", Db::Pg)?
            .find(&query, None)?
            .to_sql()?;
        assert!(!sql.is_empty());
    }
    Ok(())
}

/// A filter `tiny` does not allow is rejected, with the JS-identical message.
#[test]
fn tiny_rejects_unknown_operator() {
    let err = Builder::tiny()
        .collection("users", Db::Pg)
        .unwrap()
        .find(&parse_json(r#"{"name": {"$regex": "^a"}}"#).unwrap(), None)
        .and_then(|q| q.to_sql())
        .unwrap_err();
    assert_eq!(err.0, "Unknown operator: $regex");
}

/// Insert/update helpers accept the shared `CrudOptions`.
#[test]
fn crud_options_compile() -> Result<(), mosql::Error> {
    let users = Builder::full().collection("users", Db::Pg)?;
    let opts = CrudOptions::default();
    let doc = parse_json(r#"{"name": "Ada"}"#)?;

    assert!(users
        .insert_one(&doc, &opts)?
        .starts_with("INSERT INTO users"));
    assert!(users
        .update_many(
            &parse_json("{}")?,
            &parse_json(r#"{"$set": {"a": 1}}"#)?,
            &opts
        )?
        .starts_with("UPDATE users"));
    let filter = parse_json(r#"{"name": "Ada"}"#)?;
    assert!(users
        .delete_one(&filter, &opts)?
        .starts_with("DELETE FROM users"));

    // An empty filter is a delete-all, which is refused unless opted in.
    let err = users
        .delete_many(&parse_json("{}")?, &CrudOptions::default())
        .unwrap_err();
    assert_eq!(
        err.0,
        "deleteMany requires a filter or allowDeleteAll option"
    );

    let opts = CrudOptions {
        allow_delete_all: true,
        ..Default::default()
    };
    assert!(users
        .delete_many(&parse_json("{}")?, &opts)?
        .starts_with("DELETE FROM users"));
    Ok(())
}

/// The two snippets in the README's "As a Rust crate" section, verbatim.
#[test]
fn readme_as_a_crate_snippets() -> Result<(), mosql::Error> {
    use mosql::{parse_json, Builder, Db, Value};

    let query = parse_json(r#"{"age": {"$gte": 18}, "role": "admin"}"#)?;
    let sort = parse_json(r#"{"age": -1}"#)?; // -1 = descending, 1 = ascending

    let sql = Builder::full()
        .collection("users", Db::Pg)?
        .find(&query, None)?
        .sort(&sort)?
        .limit(&Value::Int(10))?
        .to_sql()?;
    assert_eq!(
        sql,
        "SELECT * FROM users WHERE age >= 18 AND role = 'admin' ORDER BY age DESC NULLS LAST LIMIT 10"
    );

    use mosql::memory::Collection;

    let query = parse_json(r#"{"age": {"$gte": 18}}"#)?;
    let sort = parse_json(r#"{"age": -1}"#)?;

    let coll = Collection::new(
        "users",
        vec![
            parse_json(r#"{"name": "Ada", "age": 36}"#)?,
            parse_json(r#"{"name": "Bob", "age": 17}"#)?,
        ],
        Default::default(),
    );
    let hits: Vec<Value> = coll.find(&query).sort(&sort).limit(10).to_vec()?;
    assert_eq!(hits.len(), 1);
    Ok(())
}
