//! The platform's own data, in SQLite.
//!
//! Three tables: `meta` for the signing key, `users` for accounts, `apps` for
//! the app list. An app's *configuration* is stored as one JSON document
//! rather than exploded into columns, because the platform never queries inside
//! it — it hands the whole document to whoever serves the app. The things that
//! *are* queried (`id`, `name`, `created_at`) are columns.
//!
//! `name` is `UNIQUE COLLATE NOCASE`, so the database itself refuses two apps
//! that differ only in case. That invariant is too important to leave to a
//! check in one code path.

use std::path::{Path, PathBuf};

use mosql::Value;
use uflite::error::{Error, Result};

use crate::config::AppDoc;
use crate::platform::{AppRecord, BoxFuture, PasswordHash, Platform, PlatformStore, User};
use crate::sqlite::Connection;

/// The schema. Written with `IF NOT EXISTS` so opening an existing file is
/// harmless, and so a new column can be added here without a migration step
/// for as long as the old rows are still readable.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id          TEXT PRIMARY KEY,
    email       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    iterations  INTEGER NOT NULL,
    salt        TEXT NOT NULL,
    hash        TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS apps (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at  TEXT NOT NULL,
    document    TEXT NOT NULL
);
";

/// A platform catalog in one SQLite file.
pub struct SqliteStore {
    path: PathBuf,
}

impl SqliteStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        SqliteStore { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The directory the database sits in — the root that per-app data
    /// directories hang off.
    pub fn root(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    /// Open the database, creating it and its schema if needed.
    fn connect(&self) -> Result<Connection> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| Error::internal(format!("cannot create {}: {e}", dir.display())))?;
        }
        let conn = Connection::open(&self.path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(conn)
    }

    /// Import a platform document written by an earlier version.
    ///
    /// The first release kept its state in a JSON file. Rather than make
    /// everyone start over, a JSON file beside the database is read once and
    /// then renamed out of the way. Returns whether an import happened.
    pub fn migrate_from_json(&self) -> Result<bool> {
        let legacy = self.path.with_extension("json");
        if !legacy.exists() {
            return Ok(false);
        }
        let conn = self.connect()?;
        let already = conn.query_one("SELECT COUNT(*) AS n FROM users", &[])?;
        let users: i64 = already
            .and_then(|r| r.at(0).cloned())
            .and_then(|v| match v {
                Value::Int(i) => Some(i),
                _ => None,
            })
            .unwrap_or(0);
        if users > 0 {
            // The database is already in use; leave the old file alone rather
            // than overwrite live data with stale data.
            return Ok(false);
        }

        let platform = crate::platform::FileStore::new(&legacy).load_now()?;
        self.save_now(&conn, &platform)?;

        let done = self.path.with_extension("json.migrated");
        match std::fs::rename(&legacy, &done) {
            Ok(()) => Ok(true),
            // Another process migrated it first. Its save and this one wrote
            // the same rows, so there is nothing left to do.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(Error::internal(format!(
                "cannot retire {}: {e}",
                legacy.display()
            ))),
        }
    }
}

impl PlatformStore for SqliteStore {
    fn load(&self) -> BoxFuture<'_, Result<Platform>> {
        Box::pin(async move {
            let conn = self.connect()?;
            self.load_now(&conn)
        })
    }

    fn save(&self, platform: &Platform) -> BoxFuture<'_, Result<()>> {
        // The catalog is small, so the future owns a copy: it then borrows only
        // `self`, which is what `'_` in the signature promises.
        let platform = platform.clone();
        Box::pin(async move {
            let conn = self.connect()?;
            self.save_now(&conn, &platform)
        })
    }
}

impl SqliteStore {
    fn load_now(&self, conn: &Connection) -> Result<Platform> {
        let mut platform = Platform::default();

        for row in conn.query("SELECT key, value FROM meta", &[])? {
            if row.get("key").and_then(Value::as_str) == Some("jwtSecret") {
                platform.jwt_secret = row
                    .get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
            }
        }

        for row in conn.query(
            "SELECT id, email, iterations, salt, hash, created_at FROM users
             ORDER BY created_at, id",
            &[],
        )? {
            let text = |name: &str| -> Result<String> {
                row.get(name)
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .ok_or_else(|| Error::internal(format!("users.{name} is not text")))
            };
            let iterations = match row.get("iterations") {
                Some(Value::Int(i)) => u32::try_from(*i)
                    .map_err(|_| Error::internal("users.iterations is out of range"))?,
                _ => return Err(Error::internal("users.iterations is not an integer")),
            };
            let salt = row
                .get("salt")
                .and_then(Value::as_str)
                .and_then(crate::crypto::b64url_decode)
                .ok_or_else(|| Error::internal("users.salt is not base64url"))?;
            let hash = row
                .get("hash")
                .and_then(Value::as_str)
                .and_then(crate::crypto::b64url_decode)
                .ok_or_else(|| Error::internal("users.hash is not base64url"))?;
            platform.users.push(User {
                id: text("id")?,
                email: text("email")?,
                password: PasswordHash {
                    iterations,
                    salt,
                    hash,
                },
                created_at: text("created_at")?,
            });
        }

        for row in conn.query(
            "SELECT id, created_at, document FROM apps ORDER BY name COLLATE NOCASE",
            &[],
        )? {
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::internal("apps.id is not text"))?
                .to_string();
            let created_at = row
                .get("created_at")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::internal("apps.created_at is not text"))?
                .to_string();
            let document = row
                .get("document")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::internal(format!("apps.document for {id} is not text")))?;
            // Read the shape only: the document was validated when it was
            // stored, and re-validating every app on every start would make
            // startup cost grow with the app count for no gain.
            let doc = AppDoc::read_str(document).map_err(|e| {
                Error::internal(format!("app {id} has an unreadable document: {e}"))
            })?;
            platform.apps.push(AppRecord {
                id,
                created_at,
                doc,
            });
        }

        Ok(platform)
    }

    fn save_now(&self, conn: &Connection, platform: &Platform) -> Result<()> {
        conn.transaction(|conn| {
            // The catalog is a few kilobytes and `save` is handed the whole of
            // it, so replacing each table outright is both simpler and faster
            // than diffing to decide what changed. The transaction is what
            // makes that safe: a crash leaves the previous state intact.
            conn.execute_batch("DELETE FROM meta; DELETE FROM users; DELETE FROM apps;")?;

            conn.execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                &[
                    Value::str("jwtSecret"),
                    Value::str(platform.jwt_secret.clone()),
                ],
            )?;

            for user in &platform.users {
                conn.execute(
                    "INSERT INTO users (id, email, iterations, salt, hash, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    &[
                        Value::str(user.id.clone()),
                        Value::str(user.email.clone()),
                        Value::Int(i64::from(user.password.iterations)),
                        Value::str(crate::crypto::b64url_encode(&user.password.salt)),
                        Value::str(crate::crypto::b64url_encode(&user.password.hash)),
                        Value::str(user.created_at.clone()),
                    ],
                )?;
            }

            for app in &platform.apps {
                conn.execute(
                    "INSERT INTO apps (id, name, created_at, document)
                     VALUES (?1, ?2, ?3, ?4)",
                    &[
                        Value::str(app.id.clone()),
                        Value::str(app.doc.app.name.clone()),
                        Value::str(app.created_at.clone()),
                        Value::str(app.doc.to_json()),
                    ],
                )?;
            }
            Ok(())
        })
    }
}

/// The plain SQL the store runs, for a test that the schema and the queries
/// agree with each other.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_creates_three_tables() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let tables: Vec<String> = conn
            .query(
                "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
                &[],
            )
            .unwrap()
            .iter()
            .filter_map(|r| r.at(0).and_then(Value::as_str).map(str::to_string))
            .collect();
        assert_eq!(tables, vec!["apps", "meta", "users"]);
    }

    #[test]
    fn app_names_are_unique_ignoring_case() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO apps (id, name, created_at, document) VALUES (?1, ?2, ?3, ?4)",
            &[
                Value::str("a1"),
                Value::str("Todos"),
                Value::str("2026-01-01T00:00:00Z"),
                Value::str("{}"),
            ],
        )
        .unwrap();
        let err = conn
            .execute(
                "INSERT INTO apps (id, name, created_at, document) VALUES (?1, ?2, ?3, ?4)",
                &[
                    Value::str("a2"),
                    Value::str("todos"),
                    Value::str("2026-01-01T00:00:00Z"),
                    Value::str("{}"),
                ],
            )
            .unwrap_err();
        assert_eq!(err.status(), 409, "a duplicate name must be a conflict");
    }

    #[test]
    fn a_row_reads_back_what_was_written() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            &[Value::str("k"), Value::str("hello")],
        )
        .unwrap();
        let rows = conn.query("SELECT key, value FROM meta", &[]).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("key"), Some(&Value::str("k")));
        assert_eq!(rows[0].get("value"), Some(&Value::str("hello")));
        assert_eq!(rows[0].get("missing"), None);
    }

    #[test]
    fn json_is_stored_as_text() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let doc = mosql::parse_json(r#"{"a":[1,2,{"b":null}]}"#).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            &[Value::str("k"), doc.clone()],
        )
        .unwrap();
        let back = conn
            .query_one("SELECT value FROM meta WHERE key = 'k'", &[])
            .unwrap()
            .unwrap();
        assert_eq!(
            back.get("value").and_then(Value::as_str),
            Some(mosql::to_json(&doc).as_str())
        );
    }

    #[test]
    fn binding_the_wrong_number_of_parameters_is_an_error() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let err = conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2)",
                &[Value::str("only-one")],
            )
            .unwrap_err();
        assert_eq!(err.status(), 500);
        assert!(err.to_string().contains("parameter"), "{err}");
    }

    #[test]
    fn a_transaction_rolls_back_on_failure() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute("INSERT INTO meta (key, value) VALUES ('keep', 'me')", &[])
            .unwrap();
        let result: Result<()> = conn.transaction(|conn| {
            conn.execute("DELETE FROM meta", &[])?;
            // Fails: the table has no such column.
            conn.execute("INSERT INTO meta (key, nope) VALUES ('x', 'y')", &[])?;
            Ok(())
        });
        assert!(result.is_err());
        let kept = conn
            .query_one("SELECT value FROM meta WHERE key = 'keep'", &[])
            .unwrap();
        assert_eq!(
            kept.map(|r| r.at(0).cloned()),
            Some(Some(Value::str("me"))),
            "the failed transaction must leave the row in place"
        );
    }

    #[test]
    fn a_bad_statement_is_an_internal_error() {
        let conn = Connection::open_memory().unwrap();
        let err = conn.query("SELECT * FROM no_such_table", &[]).unwrap_err();
        assert_eq!(err.status(), 500);
        assert!(err.to_string().contains("no_such_table"), "{err}");
    }

    #[test]
    fn each_column_type_reads_back_as_its_own_value() {
        let conn = Connection::open_memory().unwrap();
        let row = conn
            .query_one("SELECT NULL AS n, 1 AS i, 1.5 AS f, 'x' AS s", &[])
            .unwrap()
            .unwrap();
        assert_eq!(row.get("n"), Some(&Value::Null));
        assert_eq!(row.get("i"), Some(&Value::Int(1)));
        assert_eq!(row.get("f"), Some(&Value::Float(1.5)));
        assert_eq!(row.get("s"), Some(&Value::str("x")));
    }

    #[test]
    fn a_text_column_coerces_a_bound_integer() {
        // `meta.value` is declared TEXT, so SQLite applies TEXT affinity and
        // the integer 1 comes back as "1". This is why every column in the
        // schema has the affinity its values need: a boolean bound to the
        // INTEGER `users.iterations` stays an integer, and only a string is
        // ever bound to `meta.value`.
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)",
            &[Value::str("k"), Value::Int(1)],
        )
        .unwrap();
        let row = conn
            .query_one("SELECT value FROM meta", &[])
            .unwrap()
            .unwrap();
        assert_eq!(row.get("value"), Some(&Value::str("1")));
    }

    #[test]
    fn an_integer_column_keeps_an_integer() {
        let conn = Connection::open_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO users (id, email, iterations, salt, hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                Value::str("u1"),
                Value::str("a@b.co"),
                Value::Int(600_000),
                Value::str("salt"),
                Value::str("hash"),
                Value::str("2026-01-01T00:00:00Z"),
            ],
        )
        .unwrap();
        let row = conn
            .query_one("SELECT iterations, email FROM users", &[])
            .unwrap()
            .unwrap();
        assert_eq!(row.get("iterations"), Some(&Value::Int(600_000)));
        assert_eq!(row.get("email"), Some(&Value::str("a@b.co")));
    }
}
