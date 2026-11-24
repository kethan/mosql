//! Reading, validating and writing an app document.

use mosql::{parse_json, Value};
use nanobase::{AppDoc, AppType, ColumnType, ConfigError, DbKind, Secrets, StorageKind};

/// The document from the spec, verbatim.
fn spec_doc() -> &'static str {
    r#"{
  "meta": { "nanobaseVersion": "1.0.0", "includesData": true },
  "app": { "name": "Config Case", "type": "remote" },
  "config": {
    "auth": {
      "database": { "type": "neon", "url": "postgres://neon", "token": "neon-token" },
      "secret": "jwt-secret",
      "providers": {
        "google":   { "enabled": true, "clientId": "g-id", "clientSecret": "g-secret" },
        "github":   { "enabled": true, "clientId": "gh-id", "clientSecret": "gh-secret" },
        "facebook": { "enabled": true }
      },
      "features": { "emailPassword": true },
      "redis": { "enabled": false }
    },
    "database": { "type": "sqlite" },
    "storage": {
      "type": "local",
      "local": { "basePath": "./data/files", "publicUrl": "http://localhost:3000/files" }
    },
    "email": null,
    "acl": { "enabled": true, "rules": [] }
  },
  "schema": {
    "users": {
      "columns": {
        "id":    { "type": "integer", "primary": true, "autoIncrement": true },
        "name":  { "type": "text", "notNull": true },
        "email": { "type": "text" }
      }
    },
    "todos": {
      "columns": {
        "id":        { "type": "integer", "primary": true, "autoIncrement": true },
        "userId":    { "type": "integer" },
        "title":     { "type": "text" },
        "completed": { "type": "boolean" }
      }
    }
  },
  "data": {
    "contacts": [ { "name": "Alice" }, { "name": "Bob" } ],
    "events":   [ { "name": "Launch" } ]
  }
}"#
}

fn doc() -> AppDoc {
    let doc = AppDoc::parse(spec_doc()).expect("parses");
    doc.validate().expect("valid");
    doc
}

#[test]
fn the_spec_document_is_valid() {
    doc();
}

#[test]
fn meta_app_and_config_are_read() {
    let d = doc();
    assert_eq!(d.meta.nanobase_version, "1.0.0");
    assert!(d.meta.includes_data);
    assert!(d.meta.includes_secrets);
    assert_eq!(d.app.name, "Config Case");
    assert_eq!(d.app.app_type, AppType::Remote);
    assert_eq!(d.config.database.kind, DbKind::Sqlite);
    assert_eq!(d.config.storage.kind, StorageKind::Local);
    assert_eq!(
        d.config
            .storage
            .local
            .as_ref()
            .map(|l| l.base_path.as_str()),
        Some("./data/files")
    );
    assert!(d.config.email.is_none());
    assert!(d.config.acl.enabled);
    assert!(d.config.acl.rules.is_empty());
}

#[test]
fn auth_settings_are_read() {
    let d = doc();
    let auth = &d.config.auth;
    assert!(auth.is_enabled());
    assert_eq!(auth.secret.as_deref(), Some("jwt-secret"));
    assert_eq!(
        auth.database.as_ref().map(|db| db.kind),
        Some(DbKind::Postgres)
    );
    assert!(auth.features.email_password);
    assert!(!auth.redis.enabled);
    assert_eq!(
        auth.providers
            .get("google")
            .map(|p| p.client_secret.as_deref()),
        Some(Some("g-secret"))
    );
    // `facebook` is enabled with no credentials yet; that is a validation
    // problem, not a parse failure.
    assert!(auth.providers.get("facebook").map(|p| p.enabled) == Some(true));
    let mut names = auth.enabled_providers();
    names.sort_unstable();
    assert_eq!(names, vec!["facebook", "github", "google"]);
}

#[test]
fn schema_columns_keep_their_flags() {
    let d = doc();
    let users = d.schema.get("users").expect("users collection");
    let id = users.columns.get("id").expect("id column");
    assert_eq!(id.kind, ColumnType::Integer);
    assert!(id.primary && id.auto_increment);
    let name = users.columns.get("name").expect("name column");
    assert!(name.not_null);
    assert_eq!(name.kind, ColumnType::Text);
    assert_eq!(users.primary().map(|(n, _)| n), Some("id"));
}

#[test]
fn the_document_round_trips() {
    let d = doc();
    let text = d.to_json();
    let again = AppDoc::parse(&text).expect("re-parses");
    assert_eq!(again, d);
    // And the JSON is the same shape, not merely the same data.
    assert_eq!(parse_json(&text).unwrap(), d.to_value());
}

#[test]
fn round_trip_survives_the_pretty_printer() {
    let d = doc();
    let pretty = d.to_json_pretty();
    assert!(pretty.contains("\n  \"app\": {\n"));
    let again = AppDoc::parse(&pretty).expect("re-parses");
    assert_eq!(again, d);
}

#[test]
fn pretty_output_escapes_strings_like_the_compact_one() {
    let mut d = doc();
    d.app.name = "quote \" and \\ and\nnewline".to_string();
    let pretty = d.to_json_pretty();
    let compact = d.to_json();
    assert_eq!(parse_json(&pretty).unwrap(), parse_json(&compact).unwrap());
}

#[test]
fn redaction_removes_every_secret() {
    let d = doc();
    let redacted = d.to_json_pretty_redacted();
    for secret in ["jwt-secret", "g-secret", "gh-secret", "neon-token"] {
        assert!(
            !redacted.contains(secret),
            "{secret} leaked into the shareable export"
        );
    }
    let parsed = parse_json(&redacted).unwrap();
    assert!(!parsed
        .get("meta")
        .unwrap()
        .get("includesSecrets")
        .unwrap()
        .as_bool()
        .unwrap());
    // Non-secret configuration survives.
    assert!(redacted.contains("postgres://neon"));
    assert!(redacted.contains("./data/files"));
}

#[test]
fn redacting_leaves_the_original_alone() {
    let d = doc();
    let copy = d.to_json_pretty_redacted();
    assert!(d.config.auth.secret.is_some());
    let _ = copy;
}

// ------------------------------------------------------------- validation ---

/// Every problem with a document, from either stage.
fn issues(doc: &str) -> Vec<String> {
    match AppDoc::parse(doc) {
        Err(e) => e.issues.iter().map(|i| i.path.clone()).collect(),
        Ok(_) => vec![],
    }
}

fn with<F: FnOnce(&mut String) -> String>(edit: F) -> String {
    let mut s = spec_doc().to_string();
    edit(&mut s)
}

#[test]
fn every_problem_is_reported_at_once() {
    let text = with(|s| {
        s.replace(r#""name": "Config Case""#, r#""name": "Bad Name!""#)
            .replace(r#""type": "sqlite""#, r#""type": "weaviate""#)
            .replace(r#""type": "local""#, r#""type": "ftp""#)
    });
    let paths = issues(&text);
    assert!(paths.contains(&"app.name".to_string()), "{paths:?}");
    assert!(
        paths.contains(&"config.database.type".to_string()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"config.storage.type".to_string()),
        "{paths:?}"
    );
}

#[test]
fn a_provider_without_credentials_is_reported_not_refused() {
    // The spec document has `facebook` enabled with none. The app still
    // imports; the user is told the OAuth flow cannot start.
    let d = doc();
    assert_eq!(d.incomplete_providers(), vec!["facebook"]);
    assert!(d.validate().is_ok());
}

#[test]
fn credentials_from_the_environment_complete_a_provider() {
    let secrets = Secrets::from_pairs([
        ("SECRET_FACEBOOK_CLIENT_ID", "fb-id"),
        ("SECRET_FACEBOOK_CLIENT_SECRET", "fb-secret"),
    ]);
    let d = doc().overlay(&secrets);
    assert!(d.incomplete_providers().is_empty());
    assert_eq!(
        d.config.auth.providers["facebook"].client_secret.as_deref(),
        Some("fb-secret")
    );
}

#[test]
fn the_environment_does_not_override_the_file() {
    // A file value is a deliberate choice for this app.
    let secrets = Secrets::from_pairs([("SECRET_JWT_SECRET", "from-env")]);
    let d = doc().overlay(&secrets);
    assert_eq!(d.config.auth.secret.as_deref(), Some("jwt-secret"));
}

#[test]
fn the_environment_fills_an_absent_secret() {
    let mut text = spec_doc().to_string();
    text = text.replace(r#""secret": "jwt-secret","#, "");
    let secrets = Secrets::from_pairs([("SECRET_JWT_SECRET", "from-env-0123456789")]);
    let d = AppDoc::load(&text, &secrets).expect("complete once the environment supplies it");
    assert_eq!(d.config.auth.secret.as_deref(), Some("from-env-0123456789"));
}

#[test]
fn the_environment_can_supply_the_app_database() {
    let mut text = spec_doc().to_string();
    text = text.replace(
        r#""database": { "type": "sqlite" }"#,
        r#""database": { "type": "neon" }"#,
    );
    // No URL in the file, so it is a problem…
    assert!(AppDoc::parse(&text)
        .unwrap_err()
        .issues
        .iter()
        .any(|i| i.path == "config.database.url"));
    // …until the environment supplies one.
    let secrets = Secrets::from_pairs([("SECRET_DATA_DB_URL", "postgres://neon")]);
    let d = AppDoc::load(&text, &secrets).expect("complete once the environment supplies it");
    assert_eq!(d.config.database.url.as_deref(), Some("postgres://neon"));
}

#[test]
fn secrets_only_come_from_the_secret_prefix() {
    let mut text = spec_doc().to_string();
    text = text.replace(r#""secret": "jwt-secret","#, "");
    let secrets = Secrets::from_pairs([("JWT_SECRET", "nope")]);
    let d = AppDoc::read_str(&text).unwrap().overlay(&secrets);
    assert_eq!(d.config.auth.secret, None);
}

#[test]
fn provider_variable_names_are_uppercased() {
    let secrets = Secrets::from_pairs([
        ("SECRET_GOOGLE_CLIENT_ID", "g-id"),
        ("SECRET_GOOGLE_CLIENT_SECRET", "g-secret"),
    ]);
    assert_eq!(secrets.provider_client_id("google"), Some("g-id"));
    assert_eq!(secrets.provider_client_secret("google"), Some("g-secret"));
    assert_eq!(secrets.provider_client_id("github"), None);
    // Only `SECRET_*` is honoured.
    assert!(Secrets::from_pairs([("OTHER", "x")])
        .get("SECRET_OTHER")
        .is_none());
    assert_eq!(secrets.get("SECRET_JWT_SECRET"), None);
    assert_eq!(Secrets::from_pairs([("SECRET_A", "1")]).len(), 1);
}

#[test]
fn auth_secret_is_checked_only_when_auth_is_on() {
    let text = with(|s| {
        s.replace(r#""secret": "jwt-secret","#, "")
            .replace(r#""enabled": true"#, r#""enabled": false"#)
            .replace(r#""emailPassword": true"#, r#""emailPassword": false"#)
    });
    assert!(issues(&text).is_empty(), "no auth, no secret needed");
}

#[test]
fn a_short_secret_is_rejected() {
    let text = with(|s| s.replace(r#""secret": "jwt-secret""#, r#""secret": "short""#));
    assert!(issues(&text).contains(&"config.auth.secret".to_string()));
}

#[test]
fn a_remote_database_needs_a_url() {
    let text = with(|s| {
        s.replace(
            r#""database": { "type": "sqlite" }"#,
            r#""database": { "type": "neon" }"#,
        )
    });
    assert!(issues(&text).contains(&"config.database.url".to_string()));
}

#[test]
fn s3_storage_needs_a_bucket() {
    let text = with(|s| {
        s.replace(
            r#""type": "local",
      "local": { "basePath": "./data/files", "publicUrl": "http://localhost:3000/files" }"#,
            r#""type": "s3",
      "s3": { "endpoint": "https://r2.example.com" }"#,
        )
    });
    assert!(issues(&text).contains(&"config.storage.s3.bucket".to_string()));
}

#[test]
fn s3_keys_may_come_from_the_environment() {
    let text = with(|s| {
        s.replace(
            r#""type": "local",
      "local": { "basePath": "./data/files", "publicUrl": "http://localhost:3000/files" }"#,
            r#""type": "s3",
      "s3": { "endpoint": "https://r2.example.com", "bucket": "files" }"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(
        issues(&text).is_empty(),
        "keys come from SECRET_S3_ACCESS_KEY"
    );
}

#[test]
fn autoincrement_outside_a_primary_key_is_rejected() {
    let text = with(|s| {
        s.replace(
            r#""userId":    { "type": "integer" }"#,
            r#""userId":    { "type": "integer", "autoIncrement": true }"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).contains(&"schema.todos.columns.userId".to_string()));
}

#[test]
fn two_primary_keys_are_rejected() {
    let text = with(|s| {
        s.replace(
            r#""name":  { "type": "text", "notNull": true }"#,
            r#""name":  { "type": "text", "notNull": true, "primary": true }"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).contains(&"schema.users".to_string()));
}

#[test]
fn column_types_have_aliases() {
    let text = with(|s| {
        s.replace(
            r#""email": { "type": "text" }"#,
            r#""email": { "type": "string" }"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    let d = AppDoc::parse(&text).unwrap();
    assert_eq!(d.schema["users"].columns["email"].kind, ColumnType::Text);
    // The written form is kept, so export is not lossy.
    assert_eq!(d.schema["users"].columns["email"].raw_type, "string");
}

#[test]
fn database_providers_map_to_engines() {
    for (raw, kind) in [
        ("neon", DbKind::Postgres),
        ("turso", DbKind::Libsql),
        ("planetscale", DbKind::Mysql),
        ("d1", DbKind::D1),
        ("memory", DbKind::Memory),
    ] {
        let text = with(|s| {
            s.replace(
                r#""database": { "type": "sqlite" }"#,
                &format!(r#""database": {{ "type": "{raw}", "url": "x" }}"#),
            )
            .replace(
                r#""facebook": { "enabled": true }"#,
                r#""facebook": { "enabled": false }"#,
            )
        });
        let d = AppDoc::parse(&text).unwrap();
        assert_eq!(d.config.database.kind, kind, "{raw}");
    }
}

// ----------------------------------------------------------------- data ---

#[test]
fn data_must_fit_the_declared_schema() {
    let text = with(|s| {
        s.replace(
            r#""contacts": [ { "name": "Alice" }, { "name": "Bob" } ]"#,
            r#""users": [ { "name": "Alice", "nmae": "typo" } ]"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).contains(&"data.users[0].nmae".to_string()));
}

#[test]
fn data_types_are_checked_strictly() {
    let text = with(|s| {
        s.replace(
            r#""contacts": [ { "name": "Alice" }, { "name": "Bob" } ]"#,
            r#""todos": [ { "title": 1, "completed": "yes" } ]"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    let paths = issues(&text);
    assert!(
        paths.contains(&"data.todos[0].title".to_string()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"data.todos[0].completed".to_string()),
        "{paths:?}"
    );
}

#[test]
fn a_not_null_column_must_be_present() {
    let text = with(|s| {
        s.replace(
            r#""contacts": [ { "name": "Alice" }, { "name": "Bob" } ]"#,
            r#""users": [ { "email": "a@b.c" } ]"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).contains(&"data.users[0].name".to_string()));
}

#[test]
fn the_autoincrement_key_may_be_omitted() {
    let text = with(|s| {
        s.replace(
            r#""contacts": [ { "name": "Alice" }, { "name": "Bob" } ]"#,
            r#""users": [ { "name": "Alice" } ]"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).is_empty());
}

#[test]
fn null_is_allowed_where_a_value_would_not_be() {
    let text = with(|s| {
        s.replace(
            r#""contacts": [ { "name": "Alice" }, { "name": "Bob" } ]"#,
            r#""todos": [ { "title": null, "completed": null } ]"#,
        )
        .replace(
            r#""facebook": { "enabled": true }"#,
            r#""facebook": { "enabled": false }"#,
        )
    });
    assert!(issues(&text).is_empty());
}

#[test]
fn data_for_an_undeclared_collection_is_allowed_but_reported() {
    let d = doc();
    let mut names = d.undeclared_data_collections();
    names.sort_unstable();
    assert_eq!(names, vec!["contacts", "events"]);
}

#[test]
fn column_types_accept_what_their_engine_would() {
    assert!(ColumnType::Integer.accepts(&Value::Int(1)));
    assert!(ColumnType::Integer.accepts(&Value::Float(2.0)));
    assert!(!ColumnType::Integer.accepts(&Value::Float(2.5)));
    assert!(!ColumnType::Integer.accepts(&Value::str("1")));
    assert!(ColumnType::Real.accepts(&Value::Int(1)));
    assert!(ColumnType::Text.accepts(&Value::str("x")));
    assert!(!ColumnType::Text.accepts(&Value::Int(1)));
    assert!(ColumnType::Boolean.accepts(&Value::Bool(true)));
    assert!(!ColumnType::Boolean.accepts(&Value::str("true")));
    assert!(ColumnType::Datetime.accepts(&Value::str("2026-11-23")));
    assert!(ColumnType::Datetime.accepts(&Value::str("2026-11-23T10:00:00Z")));
    assert!(!ColumnType::Datetime.accepts(&Value::str("yesterday")));
    assert!(ColumnType::Json.accepts(&Value::object()));
    assert!(!ColumnType::Json.accepts(&Value::Null));
}

// ------------------------------------------------------------- malformed ---

#[test]
fn not_json_is_one_issue() {
    let err = AppDoc::parse("{oops").unwrap_err();
    assert_eq!(err.issues.len(), 1);
    assert_eq!(err.issues[0].path, "$");
    assert!(err.issues[0].message.starts_with("not valid JSON"));
}

#[test]
fn a_document_that_is_not_an_object_is_rejected() {
    let err = AppDoc::parse("[1,2,3]").unwrap_err();
    assert_eq!(err.issues[0].path, "$");
}

#[test]
fn parse_collects_structural_problems() {
    let err = AppDoc::parse("{}").unwrap_err();
    let paths: Vec<String> = err.issues.iter().map(|i| i.path.clone()).collect();
    assert!(paths.contains(&"app".to_string()), "{paths:?}");
    assert!(paths.contains(&"config".to_string()), "{paths:?}");
}

#[test]
fn the_error_envelope_is_the_contracts() {
    let err = AppDoc::parse("{}").unwrap_err();
    let wire = err.to_wire();
    assert_eq!(wire.get("name"), Some(&Value::str("Unprocessable")));
    assert_eq!(wire.get("code"), Some(&Value::Int(422)));
    assert_eq!(wire.get("className"), Some(&Value::str("unprocessable")));
    let errors = wire
        .get("errors")
        .and_then(Value::as_object)
        .expect("errors");
    assert!(!errors.is_empty());
    // Each field path appears exactly once.
    for (path, message) in errors.iter() {
        assert!(!message.as_str().unwrap_or("").is_empty(), "{path}");
    }
}

#[test]
fn the_error_is_displayable() {
    let err = AppDoc::parse("{}").unwrap_err();
    let text = err.to_string();
    assert!(text.contains("config"), "{text}");
    assert!(text.contains(": "), "{text}");
}

#[test]
fn an_empty_app_is_valid() {
    let doc = AppDoc::empty("todos");
    assert!(doc.validate().is_ok());
    assert_eq!(doc.config.database.kind, DbKind::Sqlite);
    assert_eq!(doc.config.storage.kind, StorageKind::Local);
    assert!(!doc.config.auth.is_enabled());
    // And it writes a document that reads back as the same thing.
    let again = AppDoc::parse(&doc.to_json()).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn an_empty_app_can_be_shaped_by_hand() {
    let mut doc = AppDoc::empty("notes");
    doc.config.database = nanobase::DatabaseConfig {
        kind: DbKind::Postgres,
        raw_type: "neon".to_string(),
        url: Some("postgres://neon".to_string()),
        token: None,
    };
    doc.config.acl.enabled = true;
    doc.config
        .acl
        .rules
        .push(parse_json(r#"{"role":"user"}"#).unwrap());
    let mut todos = nanobase::CollectionDef::default();
    todos.columns.insert(
        "id".to_string(),
        nanobase::ColumnDef {
            kind: ColumnType::Integer,
            raw_type: "integer".to_string(),
            primary: true,
            auto_increment: true,
            not_null: false,
            unique: false,
            default: None,
        },
    );
    doc.schema.insert("todos".to_string(), todos);
    doc.validate().expect("valid");
    let again = AppDoc::parse(&doc.to_json()).unwrap();
    assert_eq!(again, doc);
}

#[test]
fn config_error_is_a_std_error() {
    let err: Box<dyn std::error::Error> = Box::new(ConfigError::one("x", "y"));
    assert_eq!(err.to_string(), "x: y");
}

#[test]
fn issue_paths_are_stable() {
    let err = AppDoc::parse("{}").unwrap_err();
    for issue in &err.issues {
        assert!(!issue.path.is_empty());
        assert!(issue.path.is_ascii());
    }
}

#[test]
fn load_reports_problems_from_both_stages() {
    // Reading the shape and checking it are separate passes; a file that is
    // wrong in both ways must report both, or the user fixes them one run at
    // a time.
    let text = with(|s| {
        s.replace(r#""name": "Config Case""#, r#""name": "Bad Name!""#)
            .replace(r#""type": "sqlite""#, r#""type": "weaviate""#)
    });
    let secrets = Secrets::from_pairs([("SECRET_JWT_SECRET", "0123456789abcdef")]);
    let err = AppDoc::load(&text, &secrets).expect_err("both stages have problems");
    let paths: Vec<&str> = err.issues.iter().map(|i| i.path.as_str()).collect();
    assert!(
        paths.contains(&"config.database.type"),
        "read-stage problem missing: {paths:?}"
    );
    assert!(
        paths.contains(&"app.name"),
        "check-stage problem missing: {paths:?}"
    );
}

#[test]
fn load_accepts_a_document_whose_secrets_come_from_the_environment() {
    let text = spec_doc().to_string();
    let secrets = Secrets::from_pairs([
        ("SECRET_FACEBOOK_CLIENT_ID", "fb-id"),
        ("SECRET_FACEBOOK_CLIENT_SECRET", "fb-secret"),
    ]);
    let doc = AppDoc::load(&text, &secrets).expect("valid");
    assert!(doc.incomplete_providers().is_empty());
}
