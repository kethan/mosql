//! Naming the cloud, instead of reading it out of a URL.
//!
//! A URL says a protocol more often than it says a cloud. A
//! `postgres://` URL names PostgreSQL, and Neon, Vercel Postgres and
//! Supabase all speak it — but their endpoints and their sessions are
//! different, and their URLs do not always say which one they are. A
//! `mysql://` URL is the same story: TiDB Cloud and PlanetScale both
//! speak MySQL over HTTP, and they are not interchangeable.
//!
//! So the user may name the cloud. These tests are about what that name
//! does: it is read from a document, it is checked against the engine,
//! it survives an export, and — the point of it all — it is what the
//! request is built from.

use std::path::Path;

use nanobase::config::DbProvider;
use nanobase::{AppDoc, DatabaseConfig, DbKind};

/// A document with a database block, as a person would write it.
fn document(database: &str) -> String {
    format!(r#"{{"app": {{"name": "todos"}}, "config": {{"database": {database}}}}}"#)
}

/// The database of a document, read from its JSON.
fn database_of(text: &str) -> Result<DatabaseConfig, String> {
    let doc = AppDoc::parse(text).map_err(|e| e.to_string())?;
    Ok(doc.config.database)
}

#[test]
fn a_document_may_name_the_cloud() {
    let config = database_of(&document(
        r#"{"type": "postgres", "provider": "neon", "url": "postgres://u:p@host/db"}"#,
    ))
    .unwrap();
    assert_eq!(config.kind, DbKind::Postgres);
    assert_eq!(config.provider, Some(DbProvider::Neon));
    assert_eq!(config.provider_name(), Some("neon"));
}

#[test]
fn the_cloud_says_which_engine_it_is() {
    // A document that names the cloud and no engine is not missing
    // anything: a Neon database is PostgreSQL, and the platform knows
    // that.
    let config = database_of(&document(
        r#"{"provider": "neon", "url": "postgres://u:p@host/db"}"#,
    ))
    .unwrap();
    assert_eq!(config.kind, DbKind::Postgres);
    assert_eq!(config.raw_type, "postgres");

    let config = database_of(&document(
        r#"{"provider": "turso", "url": "libsql://db.turso.io"}"#,
    ))
    .unwrap();
    assert_eq!(config.kind, DbKind::Libsql);

    let config = database_of(&document(
        r#"{"provider": "planetscale", "url": "mysql://u:p@host/db"}"#,
    ))
    .unwrap();
    assert_eq!(config.kind, DbKind::Mysql);
}

#[test]
fn a_cloud_that_disagrees_with_the_engine_is_refused() {
    // A Turso database is libSQL, and a document that says `postgres`
    // and `turso` is asking for two databases at once.
    let err = database_of(&document(
        r#"{"type": "postgres", "provider": "turso", "url": "libsql://db.turso.io"}"#,
    ))
    .unwrap_err();
    assert!(err.contains("turso"), "the cloud is named: {err}");
    assert!(err.contains("speaks"), "and what it speaks: {err}");
}

#[test]
fn a_cloud_that_is_not_a_cloud_is_refused() {
    let err = database_of(&document(
        r#"{"type": "postgres", "provider": "sqlite", "url": "postgres://host/db"}"#,
    ))
    .unwrap_err();
    assert!(err.contains("unknown provider"), "{err}");
    // The message says which clouds there are.
    assert!(err.contains("neon"), "{err}");
    assert!(err.contains("planetscale"), "{err}");
}

#[test]
fn the_cloud_survives_an_export() {
    let doc = AppDoc::parse(&document(
        r#"{"type": "postgres", "provider": "neon", "url": "postgres://u:p@host/db", "token": "s3cret"}"#,
    ))
    .unwrap();
    let json = doc.to_json();
    assert!(json.contains("\"provider\":\"neon\""), "{json}");
    // And the document reads back as the same thing.
    let again = AppDoc::parse(&json).unwrap();
    assert_eq!(again.config.database.provider, Some(DbProvider::Neon));
    // The cloud is not a secret, and a redacted export keeps it.
    let redacted = doc.to_json_redacted();
    assert!(redacted.contains("\"provider\":\"neon\""), "{redacted}");
    assert!(!redacted.contains("s3cret"), "{redacted}");
}

#[test]
fn the_names_people_use_are_accepted() {
    for (name, cloud) in [
        ("turso", DbProvider::Turso),
        ("libsql", DbProvider::Turso),
        ("d1", DbProvider::D1),
        ("cloudflare", DbProvider::D1),
        ("neon", DbProvider::Neon),
        ("postgres", DbProvider::Neon),
        ("tidb", DbProvider::Tidb),
        ("planetscale", DbProvider::PlanetScale),
        ("pscale", DbProvider::PlanetScale),
    ] {
        assert_eq!(DbProvider::parse(name), Some(cloud), "{name}");
    }
    assert_eq!(DbProvider::parse("sqlite"), None);
    assert_eq!(DbProvider::parse("  Neon  "), Some(DbProvider::Neon));
}

#[test]
fn a_clone_may_be_given_its_own_cloud() {
    use nanobase::clone::{plan, CloneData, CloneRequest};
    use nanobase::platform::AppRecord;

    let mut doc = AppDoc::empty("todos");
    doc.config.database = DatabaseConfig {
        kind: DbKind::Postgres,
        raw_type: "postgres".to_string(),
        url: Some("postgres://u:p@neon.example/db".to_string()),
        token: Some("secret".to_string()),
        provider: Some(DbProvider::Neon),
    };
    let source = AppRecord {
        id: "u_source".to_string(),
        created_at: "2026-01-01T00:00:00Z".to_string(),
        doc,
    };

    // The clone is given a database in another cloud: the cloud is
    // named, and the engine follows it.
    let request = CloneRequest::named("staging")
        .with_data(CloneData::Copy)
        .with_database("mysql://u:p@other.example/db")
        .with_provider(DbProvider::Tidb);
    let clone = plan(&source, &request).unwrap();
    assert_eq!(clone.doc.config.database.provider, Some(DbProvider::Tidb));
    assert_eq!(clone.doc.config.database.kind, DbKind::Mysql);
    assert_eq!(clone.doc.config.database.raw_type, "mysql");
    // The source's token is still not carried over.
    assert!(clone.doc.config.database.token.is_none());

    // A clone that shares the source's database keeps the cloud too.
    let request = CloneRequest::named("staging").with_data(CloneData::Share);
    let clone = plan(&source, &request).unwrap();
    assert_eq!(clone.doc.config.database.provider, Some(DbProvider::Neon));
}

#[cfg(feature = "curl")]
#[test]
fn the_named_cloud_is_the_one_that_is_used() {
    use nanobase::db::factory;

    // A MySQL URL whose host is neither a TiDB Cloud host nor a
    // PlanetScale host is refused, because guessing would send the
    // request to the wrong place with no warning.
    let config = DatabaseConfig {
        kind: DbKind::Mysql,
        raw_type: "mysql".to_string(),
        url: Some("mysql://user:pass@db.example.com/mydb".to_string()),
        token: None,
        provider: None,
    };
    assert!(
        factory::open(&config, Path::new("/tmp/nanobase-does-not-use-this")).is_err(),
        "the URL alone does not say which cloud it is"
    );

    // The user says, and the platform believes them.
    let config = DatabaseConfig {
        provider: Some(DbProvider::PlanetScale),
        ..config
    };
    let db = factory::open(&config, Path::new("/tmp/nanobase-does-not-use-this")).unwrap();
    assert_eq!(db.kind(), "planetscale");
}

#[cfg(feature = "curl")]
#[test]
fn a_console_url_may_name_the_cloud() {
    use nanobase::db::factory;

    // The URL a console shows is not the URL a driver wants: it says
    // `https://`, and the host is all the driver needs. With the cloud
    // named, the request goes to the right place.
    let config = DatabaseConfig {
        kind: DbKind::Postgres,
        raw_type: "postgres".to_string(),
        url: Some("https://ep-cool.us-east-2.aws.neon.tech/mydb".to_string()),
        token: Some("s3cret".to_string()),
        provider: Some(DbProvider::Neon),
    };
    let db = factory::open(&config, Path::new("/tmp/nanobase-does-not-use-this")).unwrap();
    assert_eq!(db.kind(), "neon");
}
