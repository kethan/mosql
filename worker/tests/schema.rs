//! The schema file, against the engine's own.
//!
//! The Worker creates its tables from `schema.sql`, and the engine knows
//! what the tables should look like. If the two drift apart, a deployment
//! would fail at run time with a column that is not there — so they are
//! compared here, and the file is regenerated with
//!
//! ```text
//! cargo run --features schema --bin schema > schema.sql
//! ```

#[test]
fn the_schema_file_is_the_schema_the_engine_expects() {
    let expected = better_auth_core::models::schema_sql();
    let found = include_str!("../schema.sql");
    assert_eq!(
        found.trim_end(),
        expected.trim_end(),
        "schema.sql is out of date; regenerate it with `cargo run --features schema --bin schema > schema.sql`"
    );
}

#[test]
fn the_schema_has_a_table_for_every_model() {
    let schema = better_auth_core::models::schema_sql();
    for model in ["user", "session", "account", "verification", "twoFactor"] {
        assert!(
            schema.contains(&format!("CREATE TABLE IF NOT EXISTS \"{model}\"")),
            "the schema has no table for {model}"
        );
    }
}
