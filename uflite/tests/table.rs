//! Port of uflite's `tests/table.spec.js`. The SQLite-backed cases use a
//! recording adapter instead of `bun:sqlite`: the service is a validator and
//! delegator, so what matters is that every DDL call reaches the adapter with
//! its arguments, that adapter results and errors pass through untouched, and
//! that nothing invalid gets that far.

mod common;

use std::sync::Mutex;

use common::{expect_err, expect_json, json};
use mosql::Value;
use uflite::table::{is_column_type, is_identifier, TableDb, TableService};
use uflite::{Error, Result};

/// Every capability, recording `(method, args)` and answering `response`.
struct Recorder {
    calls: Mutex<Vec<(&'static str, Value)>>,
    response: Value,
    /// `(method, message)`: this method fails with the adapter's own error.
    failing: Option<(&'static str, &'static str)>,
}

impl Recorder {
    fn new() -> Self {
        Recorder {
            calls: Mutex::new(Vec::new()),
            response: json(r#"{"acknowledged": true}"#),
            failing: None,
        }
    }

    fn failing(method: &'static str, message: &'static str) -> Self {
        Recorder {
            failing: Some((method, message)),
            ..Recorder::new()
        }
    }

    fn record(&self, method: &'static str, args: Value) -> Result<Value> {
        self.calls.lock().unwrap().push((method, args));
        match self.failing {
            Some((m, msg)) if m == method => Err(Error::internal(msg)),
            _ => Ok(self.response.clone()),
        }
    }

    fn calls(&self) -> Vec<(&'static str, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

fn arr(items: Vec<Value>) -> Value {
    Value::Array(items)
}

impl TableDb for Recorder {
    async fn list_collections(&self) -> Result<Value> {
        self.record("listCollections", arr(vec![]))
    }
    async fn get_table_schema(&self, table: &str) -> Result<Value> {
        self.record("getTableSchema", arr(vec![Value::str(table)]))
    }
    async fn create_table_with_schema(&self, table: &str, schema: &Value) -> Result<Value> {
        self.record(
            "createTableWithSchema",
            arr(vec![Value::str(table), schema.clone()]),
        )
    }
    async fn add_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        self.record(
            "addColumn",
            arr(vec![
                Value::str(table),
                Value::str(column),
                Value::str(column_type),
                options.cloned().unwrap_or(Value::Null),
            ]),
        )
    }
    async fn rename_column(&self, table: &str, old: &str, new: &str) -> Result<Value> {
        self.record(
            "renameColumn",
            arr(vec![Value::str(table), Value::str(old), Value::str(new)]),
        )
    }
    async fn modify_column(
        &self,
        table: &str,
        column: &str,
        column_type: &str,
        options: Option<&Value>,
    ) -> Result<Value> {
        self.record(
            "modifyColumn",
            arr(vec![
                Value::str(table),
                Value::str(column),
                Value::str(column_type),
                options.cloned().unwrap_or(Value::Null),
            ]),
        )
    }
    async fn drop_column(&self, table: &str, column: &str) -> Result<Value> {
        self.record(
            "dropColumn",
            arr(vec![Value::str(table), Value::str(column)]),
        )
    }
    async fn drop_collection(&self, table: &str) -> Result<Value> {
        self.record("dropCollection", arr(vec![Value::str(table)]))
    }
}

/// The spec's "partial" adapter: no column operations at all.
struct NoColumnOps;

impl TableDb for NoColumnOps {
    async fn list_collections(&self) -> Result<Value> {
        Ok(Value::Array(vec![]))
    }
    async fn get_table_schema(&self, _table: &str) -> Result<Value> {
        Ok(json(r#"{"columns": {}}"#))
    }
    async fn create_table_with_schema(&self, _table: &str, _schema: &Value) -> Result<Value> {
        Ok(json(r#"{"acknowledged": true}"#))
    }
    async fn add_column(
        &self,
        _table: &str,
        _column: &str,
        _column_type: &str,
        _options: Option<&Value>,
    ) -> Result<Value> {
        Ok(json(r#"{"acknowledged": true}"#))
    }
    async fn drop_collection(&self, _table: &str) -> Result<Value> {
        Ok(json(r#"{"acknowledged": true}"#))
    }
}

/// Only `dropCollection` exists — like the "propagates adapter failures" case.
struct DropOnly;

impl TableDb for DropOnly {
    async fn drop_collection(&self, _table: &str) -> Result<Value> {
        Err(Error::internal("underlying DDL failure"))
    }
}

#[test]
fn find_get_create_remove_delegate_to_the_adapter() {
    common::run(async {
        let db = Recorder::new();
        let tables = TableService::new(&db);
        expect_json(&tables.find().await.unwrap(), r#"{"acknowledged": true}"#);
        expect_json(
            &tables.get("users").await.unwrap(),
            r#"{"acknowledged": true}"#,
        );
        expect_json(
            &tables.create(&json(r#"{"name": "profiles", "schema": {"properties": {"name": {"type": "string"}}, "required": ["name"]}}"#)).await.unwrap(),
            r#"{"acknowledged": true}"#,
        );
        expect_json(
            &tables.remove("temporary").await.unwrap(),
            r#"{"acknowledged": true}"#,
        );
        let calls = db.calls();
        assert_eq!(
            calls.iter().map(|c| c.0).collect::<Vec<_>>(),
            [
                "listCollections",
                "getTableSchema",
                "createTableWithSchema",
                "dropCollection"
            ]
        );
        // the schema is passed through unchanged, including `required`
        expect_json(
            &calls[2].1,
            r#"["profiles", {"properties": {"name": {"type": "string"}}, "required": ["name"]}]"#,
        );
        // the service exposes the adapter it was built from
        assert_eq!(tables.db().calls().len(), 4);

        // `Arc<Adapter>` is an adapter too.
        let shared = std::sync::Arc::new(Recorder::new());
        let via_arc = TableService::new(shared.clone());
        expect_json(
            &via_arc.get("t").await.unwrap(),
            r#"{"acknowledged": true}"#,
        );
        assert_eq!(shared.calls()[0].0, "getTableSchema");
    });
}

#[test]
fn patch_delegates_every_ddl_operation_with_its_arguments() {
    common::run(async {
        let db = Recorder::new();
        let tables = TableService::new(&db);
        expect_json(
            &tables
                .patch(
                    "items",
                    &json(r#"{"operation": "addColumn", "columnName": "slug", "type": "TEXT",
                                     "options": {"required": true, "unique": true, "default": "new"}}"#),
                )
                .await
                .unwrap(),
            r#"{"acknowledged": true}"#,
        );
        expect_json(
            &tables
                .patch(
                    "items",
                    &json(r#"{"operation": "modifyColumn", "columnName": "title", "type": "VARCHAR(120)",
                                     "options": {"required": true}}"#),
                )
                .await
                .unwrap(),
            r#"{"acknowledged": true}"#,
        );
        expect_json(
            &tables
                .patch(
                    "items",
                    &json(r#"{"operation": "dropColumn", "columnName": "legacy"}"#),
                )
                .await
                .unwrap(),
            r#"{"acknowledged": true}"#,
        );
        expect_json(
            &tables
                .patch(
                    "accounts",
                    &json(r#"{"operation": "renameColumn", "oldName": "email", "newName": "contact_email"}"#),
                )
                .await
                .unwrap(),
            r#"{"acknowledged": true}"#,
        );
        // Without `options`, the adapter receives nothing — like `undefined`.
        tables
            .patch(
                "items",
                &json(r#"{"operation": "addColumn", "columnName": "x", "type": "TEXT"}"#),
            )
            .await
            .unwrap();
        let calls = db.calls();
        let methods: Vec<&str> = calls.iter().map(|c| c.0).collect();
        assert_eq!(
            methods,
            [
                "addColumn",
                "modifyColumn",
                "dropColumn",
                "renameColumn",
                "addColumn"
            ]
        );
        expect_json(
            &calls[0].1,
            r#"["items", "slug", "TEXT", {"required": true, "unique": true, "default": "new"}]"#,
        );
        expect_json(
            &calls[1].1,
            r#"["items", "title", "VARCHAR(120)", {"required": true}]"#,
        );
        expect_json(&calls[2].1, r#"["items", "legacy"]"#);
        expect_json(&calls[3].1, r#"["accounts", "email", "contact_email"]"#);
        expect_json(&calls[4].1, r#"["items", "x", "TEXT", null]"#);
    });
}

#[test]
fn preserves_adapter_errors_and_reports_missing_capabilities() {
    common::run(async {
        // An adapter that *has* the method but fails keeps its own message.
        let sqlite = Recorder::failing("modifyColumn", "SQLite does not support MODIFY COLUMN");
        let tables = TableService::new(&sqlite);
        let e = expect_err(
            tables
                .patch("items", &json(r#"{"operation": "modifyColumn", "columnName": "title", "type": "VARCHAR(80)"}"#))
                .await,
            "SQLite does not support MODIFY COLUMN",
        );
        assert_eq!(e.status(), 500);

        let sqlite = Recorder::failing("dropColumn", "SQLite does not support DROP COLUMN");
        expect_err(
            TableService::new(&sqlite)
                .patch(
                    "items",
                    &json(r#"{"operation": "dropColumn", "columnName": "title"}"#),
                )
                .await,
            "SQLite does not support DROP COLUMN",
        );

        // An adapter without the method fails with the capability message,
        // and validation still runs first.
        let tables = TableService::new(&NoColumnOps);
        expect_err(
            tables
                .patch(
                    "items",
                    &json(
                        r#"{"operation": "modifyColumn", "columnName": "title", "type": "TEXT"}"#,
                    ),
                )
                .await,
            "The database adapter does not support modifyColumn",
        );
        expect_err(
            tables
                .patch(
                    "items",
                    &json(r#"{"operation": "dropColumn", "columnName": "title"}"#),
                )
                .await,
            "The database adapter does not support dropColumn",
        );
        expect_err(
            tables
                .patch(
                    "items",
                    &json(r#"{"operation": "renameColumn", "oldName": "a", "newName": "b"}"#),
                )
                .await,
            "The database adapter does not support renameColumn",
        );

        // Adapter failures propagate unchanged.
        expect_err(
            TableService::new(&DropOnly).remove("users").await,
            "underlying DDL failure",
        );
    });
}

#[test]
fn rejects_invalid_input_before_calling_the_adapter() {
    common::run(async {
        let db = Recorder::new();
        let tables = TableService::new(&db);
        expect_err(
            tables.create(&Value::Null).await,
            "table data must be an object",
        );
        expect_err(
            tables.create(&json(r#"[{"name": "users"}]"#)).await,
            "table data must be an object",
        );
        expect_err(
            tables.create(&json(r#"{"name": "users"}"#)).await,
            "schema must be an object",
        );
        expect_err(
            tables
                .create(&json(r#"{"name": "users", "schema": []}"#))
                .await,
            "schema must be an object",
        );
        expect_err(
            tables
                .create(&json(
                    r#"{"name": "users; DROP TABLE users", "schema": {}}"#,
                ))
                .await,
            "table name must be a non-empty SQL identifier",
        );
        expect_err(
            tables.create(&json(r#"{"name": "users", "schema": {"properties": {"email;DROP": {"type": "string"}}}}"#)).await,
            "column name",
        );
        expect_err(
            tables
                .create(&json(
                    r#"{"name": "users", "schema": {"email": {"type": "TEXT; DROP TABLE users"}}}"#,
                ))
                .await,
            "valid SQL column type",
        );
        expect_err(tables.get("schema.users").await, "table name");
        expect_err(tables.get("").await, "table name");
        expect_err(tables.remove("").await, "table name");
        expect_err(
            tables.patch("users", &json("{}")).await,
            "must include an operation",
        );
        expect_err(
            tables
                .patch("users", &json(r#"{"operation": "unknown"}"#))
                .await,
            "Unsupported table patch operation 'unknown'",
        );
        expect_err(
            tables.patch("users", &Value::Null).await,
            "must include an operation",
        );
        expect_err(
            tables
                .patch("users", &json(r#"{"operation": "addColumn", "columnName": "email; DROP TABLE users", "type": "TEXT"}"#))
                .await,
            "column name",
        );
        expect_err(
            tables
                .patch("users", &json(r#"{"operation": "addColumn", "columnName": "email", "type": "TEXT; DROP TABLE users"}"#))
                .await,
            "valid SQL column type",
        );
        expect_err(
            tables
                .patch(
                    "users",
                    &json(
                        r#"{"operation": "renameColumn", "oldName": "ok", "newName": "bad name"}"#,
                    ),
                )
                .await,
            "new column name",
        );
        assert!(db.calls().is_empty(), "nothing reaches the adapter");
        // Validation errors are 400 TypeErrors, like the REST layer maps them.
        let e = tables.patch("users", &json("{}")).await.expect_err("");
        assert_eq!(e.status(), 400);
        assert_eq!(e.name(), "TypeError");
    });
}

#[test]
fn identifier_and_column_type_grammar() {
    assert!(is_identifier("users"));
    assert!(is_identifier("_tmp1"));
    assert!(!is_identifier("Ünicode"), "\\w is ASCII only");
    assert!(!is_identifier(""));
    assert!(!is_identifier("a b"));
    assert!(!is_identifier("a.b"));
    assert!(!is_identifier("a; DROP TABLE t"));
    assert!(!is_identifier("a\n"));

    for ty in [
        "INT",
        "VARCHAR(255)",
        "DOUBLE PRECISION",
        "DECIMAL(10, 2)",
        "VARCHAR (255)",
        "TIMESTAMP(6)",
        "NUMERIC( 10 , 2 )",
        "UNSIGNED BIG INT",
    ] {
        assert!(is_column_type(ty), "{ty:?} should be valid");
    }
    for ty in [
        "",
        "VARCHAR(255",
        "(255)",
        "1ABC",
        "VARCHAR(a)",
        "VARCHAR(-1)",
        "VARCHAR(1,)",
        "TEXT; DROP TABLE users",
        "VARCHAR ",
        " VARCHAR",
        "VARCHAR\n",
        "VARCHAR(255) TEXT",
    ] {
        assert!(!is_column_type(ty), "{ty:?} should be invalid");
    }
    // Tabs and newlines are `\s` inside the type grammar.
    assert!(is_column_type("DOUBLE\tPRECISION"));
    assert!(is_column_type("VARCHAR\n(255)"));
}

#[test]
fn schema_validation_covers_both_shapes() {
    use uflite::table::validate_schema;
    assert!(validate_schema(&json(r#"{"name": "TEXT"}"#)).is_ok());
    assert!(validate_schema(&json(
        r#"{"properties": {"name": {"type": "string"}, "age": {"type": "integer"}}}"#
    ))
    .is_ok());
    // JSON Schema types are left to umosql; only SQL-looking types are checked.
    assert!(validate_schema(&json(
        r#"{"label": {"type": "VARCHAR(80)", "required": true}}"#
    ))
    .is_ok());
    assert!(validate_schema(&json(r#"{"x": {"type": null}}"#)).is_err());
    assert!(validate_schema(&json(r#"{"x": {"type": "TEXT; DROP"}}"#)).is_err());
    assert!(validate_schema(&json(r#"{"x y": "TEXT"}"#)).is_err());
    assert!(
        validate_schema(&json(r#"{"x": 5}"#)).is_ok(),
        "a non-object definition has no type"
    );
    assert!(validate_schema(&json(r#"{"properties": null}"#)).is_err());
    assert!(validate_schema(&json(r#"{"properties": []}"#)).is_err());
    assert!(validate_schema(&json("{}")).is_ok());
}
