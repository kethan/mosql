//! Copying an app.
//!
//! A clone is one app made out of another: the same schema, the same
//! rules, the same auth — and, when it is asked for, the same rows.
//!
//! The rows are the whole difficulty. A configuration is cheap to copy
//! and a row is not, so the question "does the clone bring the data?"
//! has to be answered on purpose. There are three answers, and each one
//! is a different product decision:
//!
//! * **omit** — the clone starts empty. This is the default, and the
//!   safe one: a clone that quietly shared a live database would be a
//!   way to write to production by accident.
//! * **copy** — the clone gets the rows in a database of its own.
//! * **share** — the clone and the source are one database under two
//!   names. Only when the operator says so.
//!
//! # Where the clone's rows go
//!
//! An embedded engine — a SQLite file, memory — gives every app its own
//! rows for free, because the file is named after the app. A serverless
//! engine does not: a Turso database is made at Turso, and a platform
//! cannot make one for itself.
//!
//! So a clone of an app in a serverless database gets **no database at
//! all** until somebody names one. It is a template — a schema, a set of
//! rules, an auth configuration — and it becomes an app with rows when it
//! is pointed at a database. Copying the URL instead would point the
//! clone at the original's rows, which is exactly the accident this
//! exists to prevent.
//!
//! Copying the rows needs a destination, so a copy of a serverless app
//! without one is refused rather than quietly pointed at the source.

use uflite::error::{Error, Result};

use crate::config::{AppDoc, DatabaseConfig, DbKind};
use crate::db::AppDb;
use crate::platform::AppRecord;

/// What a clone does with the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloneData {
    /// The rows are not copied. The clone starts empty.
    Omit,
    /// The rows are copied into the clone's own database.
    Copy,
    /// The clone uses the source's database, rows and all.
    ///
    /// One database, two names. A write through one is a write through
    /// the other, so this is only ever done on purpose.
    Share,
}

impl CloneData {
    /// The name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            CloneData::Omit => "omit",
            CloneData::Copy => "copy",
            CloneData::Share => "share",
        }
    }

    /// Read the name on the wire.
    pub fn parse(text: &str) -> Option<CloneData> {
        match text.trim().to_ascii_lowercase().as_str() {
            "omit" | "none" | "no" | "false" => Some(CloneData::Omit),
            "copy" | "yes" | "true" | "with" => Some(CloneData::Copy),
            "share" | "same" => Some(CloneData::Share),
            _ => None,
        }
    }
}

/// A request to clone an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRequest {
    /// The clone's name.
    pub name: String,
    /// What happens to the rows.
    pub data: CloneData,
    /// The database the clone's rows go in, when it is not the source's.
    ///
    /// A URL. For a serverless source this is how the operator says
    /// which database the clone should have.
    pub database: Option<String>,
    /// The token for that database.
    ///
    /// The source's own token is never carried over to a different
    /// database: it is the password of the source's database, and
    /// handing it to another service would leak it.
    pub token: Option<String>,
}

impl CloneRequest {
    /// A clone with a name and nothing else: the configuration, and no
    /// rows.
    pub fn named(name: impl Into<String>) -> CloneRequest {
        CloneRequest {
            name: name.into(),
            data: CloneData::Omit,
            database: None,
            token: None,
        }
    }

    /// The same clone, with the rows.
    pub fn with_data(mut self, data: CloneData) -> CloneRequest {
        self.data = data;
        self
    }

    /// The same clone, with a database of its own.
    pub fn with_database(mut self, url: impl Into<String>) -> CloneRequest {
        self.database = Some(url.into());
        self
    }
}

/// What a clone turned out to be.
///
/// The plan is worked out before the app is made, so a request that
/// cannot be honoured is refused before anything is written.
#[derive(Debug, Clone, PartialEq)]
pub struct ClonePlan {
    /// The clone's document.
    pub doc: AppDoc,
    /// The source's id.
    pub source_id: String,
    /// Whether the rows are copied from the source into the clone.
    pub copy_rows: bool,
    /// Whether the clone and the source are one database.
    pub shares_database: bool,
}

/// What a clone turned out to be.
///
/// The summary is what the operator is told: which app was made, how
/// many rows went with it, and whether it is the source's database or
/// one of its own.
#[derive(Debug, Clone, PartialEq)]
pub struct CloneSummary {
    /// The clone's id.
    pub id: String,
    /// The clone's name.
    pub name: String,
    /// How many rows were copied.
    pub rows_copied: usize,
    /// Whether the clone and the source are one database.
    pub shares_database: bool,
    /// Whether the clone has a database of its own.
    ///
    /// A clone of an app in a serverless database does not, until
    /// somebody names one.
    pub has_database: bool,
}

impl CloneSummary {
    /// The summary, as a document.
    pub fn to_value(&self) -> mosql::Value {
        let mut o = mosql::Object::new();
        o.set("id", mosql::Value::str(self.id.clone()));
        o.set("name", mosql::Value::str(self.name.clone()));
        o.set("rowsCopied", mosql::Value::Int(self.rows_copied as i64));
        o.set("sharesDatabase", mosql::Value::Bool(self.shares_database));
        o.set("hasDatabase", mosql::Value::Bool(self.has_database));
        mosql::Value::Object(o)
    }
}

/// Work out what a clone of `source` should be.
///
/// This is where the rules live, and it is a pure function of the source
/// and the request: no database is opened, nothing is written.
pub fn plan(source: &AppRecord, request: &CloneRequest) -> Result<ClonePlan> {
    let mut doc = source.doc.clone();
    doc.app.name = request.name.trim().to_string();
    // The document's own rows are not copied: they are a declaration of
    // what an app starts with, and the clone's rows come from the
    // database — copied, shared, or empty.
    doc.data.clear();
    doc.meta.includes_data = false;

    let database = &source.doc.config.database;
    let mut copy_rows = false;
    let mut shares_database = false;

    match request.data {
        CloneData::Share => {
            if request.database.is_some() {
                return Err(Error::bad_request(
                    "a clone that shares the source's database cannot also be given one of its own",
                ));
            }
            // The clone keeps the source's database, unchanged: the same
            // URL and the same token. The rows are the source's rows.
            shares_database = true;
        }
        CloneData::Omit => {
            doc.config.database = destination(database, request, None)?;
        }
        CloneData::Copy => {
            let destination = destination(database, request, Some("copying the rows"))?;
            if let (Some(url), Some(source_url)) = (&destination.url, &database.url) {
                if url == source_url {
                    return Err(Error::bad_request(
                        "the clone's database is the source's database; copying the rows would double them",
                    ));
                }
            }
            doc.config.database = destination;
            copy_rows = true;
        }
    }

    Ok(ClonePlan {
        doc,
        source_id: source.id.clone(),
        copy_rows,
        shares_database,
    })
}

/// The database a clone's rows go in.
///
/// `why` says what the database is for, in the error message: a clone
/// that copies the rows needs a destination, and one that does not may
/// have none at all.
fn destination(
    source: &DatabaseConfig,
    request: &CloneRequest,
    why: Option<&str>,
) -> Result<DatabaseConfig> {
    match &request.database {
        Some(url) => Ok(DatabaseConfig {
            kind: source.kind,
            raw_type: source.raw_type.clone(),
            url: Some(url.clone()),
            token: request.token.clone(),
        }),
        None if source.is_embedded() => {
            // An embedded engine gives every app its own rows: the file
            // is named after the app, so a new app is a new file.
            Ok(source.clone())
        }
        None => match why {
            Some(why) => Err(Error::bad_request(format!(
                "{why} needs a database to go in; name one with --database URL"
            ))),
            None => Ok(DatabaseConfig {
                kind: DbKind::None,
                raw_type: DbKind::None.as_str().to_string(),
                url: None,
                token: None,
            }),
        },
    }
}

/// Copy every row of one database into another.
///
/// The rows go in with the identifiers they had, so a clone's rows refer
/// to each other the way the original's did.
pub fn copy_rows(source: &dyn AppDb, destination: &dyn AppDb) -> Result<usize> {
    let mut copied = 0usize;
    for collection in source.collections()? {
        for row in source.rows(&collection)? {
            destination.insert(&collection, row)?;
            copied += 1;
        }
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppDoc;

    fn source(database: DatabaseConfig) -> AppRecord {
        let mut doc = AppDoc::empty("todos");
        doc.config.database = database;
        AppRecord {
            id: "u_source".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            doc,
        }
    }

    fn sqlite() -> DatabaseConfig {
        DatabaseConfig {
            kind: DbKind::Sqlite,
            raw_type: "sqlite".to_string(),
            url: None,
            token: None,
        }
    }

    fn turso() -> DatabaseConfig {
        DatabaseConfig {
            kind: DbKind::Libsql,
            raw_type: "libsql".to_string(),
            url: Some("libsql://db.turso.io".to_string()),
            token: Some("secret".to_string()),
        }
    }

    #[test]
    fn a_clone_without_rows_gets_its_own_database() {
        let plan = plan(&source(sqlite()), &CloneRequest::named("staging")).unwrap();
        assert_eq!(plan.doc.app.name, "staging");
        assert_eq!(plan.doc.config.database.kind, DbKind::Sqlite);
        assert!(!plan.copy_rows);
        assert!(!plan.shares_database);
    }

    #[test]
    fn a_clone_of_a_serverless_app_has_no_database_yet() {
        let plan = plan(&source(turso()), &CloneRequest::named("staging")).unwrap();
        assert_eq!(plan.doc.config.database.kind, DbKind::None);
        assert!(plan.doc.config.database.url.is_none());
        assert!(plan.doc.config.database.token.is_none());
        assert!(!plan.copy_rows);
    }

    #[test]
    fn a_clone_may_be_given_a_database() {
        let request = CloneRequest::named("staging").with_database("libsql://other.turso.io");
        let plan = plan(&source(turso()), &request).unwrap();
        assert_eq!(plan.doc.config.database.kind, DbKind::Libsql);
        assert_eq!(
            plan.doc.config.database.url.as_deref(),
            Some("libsql://other.turso.io")
        );
        // The source's token is not carried over: it is the password of
        // the source's database.
        assert!(plan.doc.config.database.token.is_none());
    }

    #[test]
    fn a_clone_that_shares_keeps_the_database_exactly() {
        let request = CloneRequest::named("staging").with_data(CloneData::Share);
        let plan = plan(&source(turso()), &request).unwrap();
        assert!(plan.shares_database);
        assert_eq!(
            plan.doc.config.database.url.as_deref(),
            Some("libsql://db.turso.io")
        );
        assert_eq!(plan.doc.config.database.token.as_deref(), Some("secret"));
        assert!(!plan.copy_rows);
    }

    #[test]
    fn copying_the_rows_needs_a_destination() {
        let request = CloneRequest::named("staging").with_data(CloneData::Copy);
        let err = plan(&source(turso()), &request).unwrap_err();
        assert!(err.to_string().contains("needs a database"), "{err}");
    }

    #[test]
    fn copying_the_rows_into_the_source_is_refused() {
        let request = CloneRequest::named("staging")
            .with_data(CloneData::Copy)
            .with_database("libsql://db.turso.io");
        let err = plan(&source(turso()), &request).unwrap_err();
        assert!(err.to_string().contains("would double"), "{err}");
    }

    #[test]
    fn an_embedded_source_copies_its_rows_into_its_own_file() {
        let request = CloneRequest::named("staging").with_data(CloneData::Copy);
        let plan = plan(&source(sqlite()), &request).unwrap();
        assert!(plan.copy_rows);
        assert_eq!(plan.doc.config.database.kind, DbKind::Sqlite);
    }

    #[test]
    fn a_shared_clone_cannot_also_have_its_own() {
        let request = CloneRequest::named("staging")
            .with_data(CloneData::Share)
            .with_database("libsql://other.turso.io");
        let err = plan(&source(turso()), &request).unwrap_err();
        assert!(err.to_string().contains("cannot also"), "{err}");
    }

    #[test]
    fn the_names_on_the_wire_round_trip() {
        for data in [CloneData::Omit, CloneData::Copy, CloneData::Share] {
            assert_eq!(CloneData::parse(data.as_str()), Some(data));
        }
        assert_eq!(CloneData::parse("nonsense"), None);
    }
}
