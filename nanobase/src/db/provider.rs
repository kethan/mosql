//! An app's rows in a serverless database that speaks its own protocol.
//!
//! The other remote adapter — [`crate::db::http::HttpDb`] — talks to a
//! service that holds a database and speaks *this platform's* protocol.
//! This one talks to the database itself, in the protocol it uses:
//! libSQL's pipeline, Cloudflare's D1 query endpoint, Neon's `/sql`,
//! TiDB's proxy, Vitess' `Execute`. Those are five different wire
//! formats, and [`mosql_core`] speaks all five.
//!
//! The rows are stored the way the local file stores them — one table per
//! collection, an `id` column and a `data` column holding the row as JSON
//! — so a row means the same thing whichever engine holds it, and the
//! filter is `mosql`'s matcher over the rows the table holds, in the
//! process, exactly as it is in the local file.
//!
//! What differs is the SQL around it: MySQL will not index a `TEXT`
//! column, and it quotes identifiers with backticks rather than with
//! double quotes. So the statements are written per dialect, and the
//! placeholders are written once — `?` — because the driver knows which
//! dialect it is talking to and rewrites them as it needs to.
//!
//! The transport is the host's own HTTP client, so the TLS is the host's
//! business. The driver is asynchronous and the platform is not, so the
//! call is made through [`mosql_core::driver::block_on`] — which does not
//! spin, because the transport answers within the first poll.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use mosql::Value;
use mosql_core::driver::{
    AnyServerlessDriver, DbError, Dialect, DriverConfig, HttpTransport, QueryResult,
    ServerlessDriver, SqlValue,
};
use uflite::error::{Error, Result};

use crate::db::{apply_changes, sanitize, AppDb, Row};
use crate::httpclient::{HttpClient, HttpRequest, HttpResponse};

/// A transport that hands the request to the host's HTTP client.
///
/// The client blocks, so the answer is there when the future is first
/// polled: there is nothing to wait for, and nothing to spin on.
pub struct BlockingTransport {
    client: Arc<dyn HttpClient>,
}

impl BlockingTransport {
    /// A transport over the host's client.
    pub fn new(client: Arc<dyn HttpClient>) -> BlockingTransport {
        BlockingTransport { client }
    }

    /// Make the request.
    fn send(&self, url: &str, headers: &[(&str, &str)], body: String) -> Result<HttpResponse> {
        let request = HttpRequest {
            method: "POST".to_string(),
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            body: body.into_bytes(),
        };
        self.client
            .request(&request)
            .map_err(|e| Error::internal(format!("the database did not answer: {e}")))
    }
}

impl HttpTransport for BlockingTransport {
    async fn post_json(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> std::result::Result<(u16, String), DbError> {
        let response = self
            .send(url, headers, body)
            .map_err(|e| DbError::new(e.to_string()))?;
        Ok((
            response.status,
            String::from_utf8_lossy(&response.body).into_owned(),
        ))
    }

    async fn post_json_with_headers(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: String,
    ) -> std::result::Result<(u16, String, Vec<(String, String)>), DbError> {
        let response = self
            .send(url, headers, body)
            .map_err(|e| DbError::new(e.to_string()))?;
        Ok((
            response.status,
            String::from_utf8_lossy(&response.body).into_owned(),
            response.headers,
        ))
    }
}

/// An app's rows in a serverless database.
pub struct ProviderDb<T: HttpTransport = BlockingTransport> {
    driver: AnyServerlessDriver<T>,
    dialect: Dialect,
    /// The name of the provider, for `kind`.
    provider: &'static str,
    /// The collections this database is known to hold.
    ///
    /// A serverless database may not let a client list its tables, so
    /// this is the fallback: what the platform has created, and what it
    /// has seen.
    collections: Mutex<BTreeSet<String>>,
}

impl ProviderDb<BlockingTransport> {
    /// Open the database at `url`.
    ///
    /// The URL says which provider it is: `libsql://`, `d1://`,
    /// `postgres://`, `mysql://`. The credentials are in the URL too, or
    /// in the client's own hands.
    pub fn new(url: &str, client: Arc<dyn HttpClient>) -> Result<Self> {
        let config = DriverConfig::from_url(url)
            .map_err(|e| Error::internal(format!("the database URL is not usable: {e}")))?;
        let provider = config.provider.as_str();
        let dialect = config.dialect;
        let driver = AnyServerlessDriver::from_url(url, BlockingTransport::new(client))
            .map_err(|e| Error::internal(format!("the database URL is not usable: {e}")))?;
        Ok(ProviderDb {
            driver,
            dialect,
            provider,
            collections: Mutex::new(BTreeSet::new()),
        })
    }
}

impl<T: HttpTransport> ProviderDb<T> {
    /// A database over a driver the caller made — a mock, in a test.
    pub fn with_driver(driver: AnyServerlessDriver<T>, provider: &'static str) -> Self {
        let dialect = driver.dialect();
        ProviderDb {
            driver,
            dialect,
            provider,
            collections: Mutex::new(BTreeSet::new()),
        }
    }

    /// The dialect this database speaks.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Run one statement and read the answer.
    fn run(&self, sql: &str) -> Result<QueryResult> {
        self.run_with(sql, &[])
    }

    /// Run one statement with values in it.
    fn run_with(&self, sql: &str, params: &[SqlValue]) -> Result<QueryResult> {
        mosql_core::driver::block_on(self.driver.execute(sql, params))
            .map_err(|e| Error::internal(format!("the database refused the statement: {e}")))
    }

    /// The table a collection lives in.
    ///
    /// A collection is a table with two columns: `id`, the row's
    /// identifier, and `data`, the row itself as JSON — the same shape the
    /// local file uses, so a row means the same thing in both.
    fn table(&self, collection: &str) -> String {
        format!("c_{}", sanitize(collection))
    }

    /// An identifier, quoted the way this dialect quotes them.
    fn quote(&self, name: &str) -> String {
        match self.dialect {
            // MySQL quotes with backticks; PostgreSQL and SQLite quote
            // with double quotes.
            Dialect::Mysql => format!("`{name}`"),
            _ => format!("\"{name}\""),
        }
    }

    /// The statement that makes the table for a collection.
    ///
    /// MySQL will not index a `TEXT` column, so there the identifier is
    /// a `VARCHAR`; a `utf8mb4` one of 191 characters is the longest a
    /// MySQL index key may be.
    fn create_table_sql(&self, collection: &str) -> String {
        let table = self.quote(&self.table(collection));
        let id = self.quote("id");
        let data = self.quote("data");
        match self.dialect {
            Dialect::Mysql => format!(
                "CREATE TABLE IF NOT EXISTS {table} ({id} VARCHAR(191) PRIMARY KEY, {data} LONGTEXT NOT NULL)"
            ),
            _ => format!(
                "CREATE TABLE IF NOT EXISTS {table} ({id} TEXT PRIMARY KEY, {data} TEXT NOT NULL)"
            ),
        }
    }

    /// Make sure the table for a collection exists.
    fn ensure(&self, collection: &str) -> Result<()> {
        self.run(&self.create_table_sql(collection))?;
        self.collections
            .lock()
            .expect("not poisoned")
            .insert(collection.to_string());
        Ok(())
    }

    /// Every row of a collection, as documents.
    fn all(&self, collection: &str) -> Result<Vec<Row>> {
        let table = self.quote(&self.table(collection));
        let id = self.quote("id");
        let data = self.quote("data");
        // Ordered by the identifier, so that a page of rows is the same
        // page every time: a limit and an offset over an unordered table
        // is a page that moves.
        let result = self.run(&format!("SELECT {data} FROM {table} ORDER BY {id}"))?;
        let mut out = Vec::with_capacity(result.rows.len());
        for row in &result.rows {
            let Some(text) = row.first().and_then(|cell| cell.as_str()) else {
                continue;
            };
            match mosql::parse_json(text) {
                Ok(value) => out.push(value),
                Err(e) => {
                    return Err(Error::internal(format!(
                        "a row of {collection} is not readable: {e}"
                    )))
                }
            }
        }
        Ok(out)
    }

    /// The rows matching a filter.
    ///
    /// The filter is `mosql`'s matcher, run here over the rows the table
    /// holds — the same rule the local file and the in-process engine
    /// use, so a query means the same thing in every engine.
    fn find_in(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        let all = self.all(collection)?;
        let mut found: Vec<Row> = Vec::with_capacity(all.len());
        for row in all {
            if mosql::memory::matches(query, &row)? {
                found.push(row);
            }
        }
        let offset = offset.unwrap_or(0) as usize;
        found = found.into_iter().skip(offset).collect();
        if let Some(limit) = limit {
            found.truncate(limit as usize);
        }
        Ok(found)
    }
}

impl<T: HttpTransport> AppDb for ProviderDb<T> {
    fn kind(&self) -> &'static str {
        self.provider
    }

    fn collections(&self) -> Result<Vec<String>> {
        // Each dialect says what it has in its own words. A database that
        // will not say is not a failure: what the platform has created is
        // the answer then.
        let sql = match self.dialect {
            Dialect::Sqlite => "SELECT name FROM sqlite_master WHERE type = 'table'",
            Dialect::Postgres => {
                "SELECT tablename FROM pg_catalog.pg_tables WHERE schemaname = 'public'"
            }
            Dialect::Mysql => {
                "SELECT table_name FROM information_schema.tables WHERE table_schema = DATABASE()"
            }
        };
        let known = self
            .collections
            .lock()
            .expect("not poisoned")
            .iter()
            .cloned()
            .collect::<Vec<String>>();
        let Ok(result) = self.run(sql) else {
            return Ok(known);
        };
        let mut out: Vec<String> = Vec::new();
        for row in &result.rows {
            let Some(name) = row.first().and_then(|cell| cell.as_str()) else {
                continue;
            };
            if let Some(collection) = name.strip_prefix("c_") {
                out.push(collection.to_string());
            }
        }
        if out.is_empty() {
            return Ok(known);
        }
        out.sort();
        Ok(out)
    }

    fn rows(&self, collection: &str) -> Result<Vec<Row>> {
        self.ensure(collection)?;
        self.all(collection)
    }

    fn find(
        &self,
        collection: &str,
        query: &Value,
        limit: Option<u64>,
        offset: Option<u64>,
    ) -> Result<Vec<Row>> {
        self.ensure(collection)?;
        self.find_in(collection, query, limit, offset)
    }

    fn find_one(&self, collection: &str, query: &Value) -> Result<Option<Row>> {
        self.ensure(collection)?;
        Ok(self
            .find_in(collection, query, Some(1), None)?
            .into_iter()
            .next())
    }

    fn count(&self, collection: &str, query: &Value) -> Result<u64> {
        self.ensure(collection)?;
        Ok(self.find_in(collection, query, None, None)?.len() as u64)
    }

    fn insert(&self, collection: &str, row: Row) -> Result<Row> {
        self.ensure(collection)?;
        let mut row = row;
        if row.get("id").is_none_or(|v| v.is_null()) {
            let id = crate::crypto::random_id();
            mosql::memory::set_path(&mut row, "id", Value::str(id));
        }
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("a row needs an id"))?
            .to_string();
        let table = self.quote(&self.table(collection));
        let id_column = self.quote("id");
        let data_column = self.quote("data");
        self.run_with(
            &format!("INSERT INTO {table} ({id_column}, {data_column}) VALUES (?, ?)"),
            &[SqlValue::Text(id), SqlValue::Text(mosql::to_json(&row))],
        )?;
        Ok(row)
    }

    fn update(&self, collection: &str, query: &Value, changes: &Value) -> Result<Option<Row>> {
        self.ensure(collection)?;
        let current = self
            .find_in(collection, query, Some(1), None)?
            .into_iter()
            .next();
        let Some(mut row) = current else {
            return Ok(None);
        };
        let id = row
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("a row needs an id"))?
            .to_string();
        apply_changes(&mut row, changes)?;
        let table = self.quote(&self.table(collection));
        let id_column = self.quote("id");
        let data_column = self.quote("data");
        self.run_with(
            &format!("UPDATE {table} SET {data_column} = ? WHERE {id_column} = ?"),
            &[SqlValue::Text(mosql::to_json(&row)), SqlValue::Text(id)],
        )?;
        Ok(Some(row))
    }

    fn remove(&self, collection: &str, query: &Value) -> Result<u64> {
        self.ensure(collection)?;
        let matching = self.find_in(collection, query, None, None)?;
        let table = self.quote(&self.table(collection));
        let id_column = self.quote("id");
        for row in &matching {
            let Some(id) = row.get("id").and_then(|v| v.as_str()) else {
                continue;
            };
            self.run_with(
                &format!("DELETE FROM {table} WHERE {id_column} = ?"),
                &[SqlValue::Text(id.to_string())],
            )?;
        }
        Ok(matching.len() as u64)
    }
}

/// Whether a URL names a provider this adapter speaks.
///
/// The schemes are the providers' own: `libsql://`, `turso://`, `d1://`,
/// `postgres://`, `postgresql://`, `mysql://`, `tidb://`, `pscale://`. An
/// `http://` or `https://` URL is a service that holds a database and
/// speaks this platform's own protocol — that is [`crate::db::http::HttpDb`],
/// not this.
pub fn is_provider_url(url: &str) -> bool {
    let Some((scheme, _)) = url.split_once("://") else {
        return false;
    };
    matches!(
        scheme.to_ascii_lowercase().as_str(),
        "libsql"
            | "turso"
            | "d1"
            | "postgres"
            | "postgresql"
            | "mysql"
            | "tidb"
            | "pscale"
            | "planetscale"
    )
}

/// The provider a URL names, if it names one.
///
/// The name is the provider's own — `turso`, `d1`, `neon`, `tidb`,
/// `planetscale` — so the platform can say which database an app's rows
/// are in without opening it.
pub fn provider_name(url: &str) -> Option<&'static str> {
    let config = mosql_core::driver::DriverConfig::from_url(url).ok()?;
    Some(config.provider.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mosql_core::driver::MockTransport;

    /// A Neon driver over a mock that answers with these replies, in order.
    fn neon(
        responses: Vec<(u16, String)>,
    ) -> (AnyServerlessDriver<Arc<MockTransport>>, Arc<MockTransport>) {
        let transport = Arc::new(MockTransport::with_responses(responses));
        let driver = AnyServerlessDriver::from_url(
            "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb",
            Arc::clone(&transport),
        )
        .expect("a Neon driver");
        (driver, transport)
    }

    /// The reply Neon gives for one statement.
    ///
    /// `columns` is the field list and `rows` is the whole rows array —
    /// one array of cells per row.
    fn neon_rows(columns: &str, rows: &str) -> String {
        format!(r#"[{{"fields":[{columns}],"rows":{rows},"rowCount":1}}]"#)
    }

    #[test]
    fn a_provider_url_is_one_this_adapter_speaks() {
        assert!(is_provider_url("libsql://db.turso.io"));
        assert!(is_provider_url("d1://acct:token@api.cloudflare.com/db"));
        assert!(is_provider_url("postgres://user:pass@host/db"));
        assert!(is_provider_url("mysql://user:pass@host/db"));
        assert!(!is_provider_url("https://db.example.com"));
        assert!(!is_provider_url("http://127.0.0.1:8090"));
        assert!(!is_provider_url("not a url"));
    }

    #[test]
    fn a_url_says_which_provider_it_is() {
        assert_eq!(provider_name("libsql://db.turso.io"), Some("turso"));
        assert_eq!(
            provider_name("postgres://u:p@ep-cool.us-east-2.aws.neon.tech/mydb"),
            Some("neon")
        );
        assert_eq!(
            provider_name("mysql://u:p@aws.connect.psdb.cloud/mydb"),
            Some("planetscale")
        );
        assert_eq!(provider_name("https://db.example.com"), None);
    }

    #[test]
    fn a_row_goes_in_as_a_json_document() {
        let (driver, transport) = neon(vec![(200, r#"[{"fields":[],"rows":[]}]"#.to_string())]);
        let db = ProviderDb::with_driver(driver, "neon");
        let mut row = mosql::Object::new();
        row.set("title", Value::str("hello"));
        db.insert("posts", Value::Object(row)).unwrap();
        let requests = transport.requests();
        // The table is made, then the row goes in.
        assert_eq!(requests.len(), 2);
        let create = requests[0].json().unwrap();
        assert_eq!(
            create.get("query").and_then(|v| v.as_str()),
            Some("CREATE TABLE IF NOT EXISTS \"c_posts\" (\"id\" TEXT PRIMARY KEY, \"data\" TEXT NOT NULL)")
        );
        let insert = requests[1].json().unwrap();
        let query = insert.get("query").and_then(|v| v.as_str()).unwrap();
        assert_eq!(
            query,
            "INSERT INTO \"c_posts\" (\"id\", \"data\") VALUES ($1, $2)"
        );
        let params = insert.get("params").and_then(|v| v.as_array()).unwrap();
        assert_eq!(params.len(), 2);
        assert!(params[1].as_str().unwrap().contains("\"title\":\"hello\""));
    }

    #[test]
    fn a_mysql_table_has_an_indexable_identifier() {
        let transport = Arc::new(MockTransport::with_responses(vec![
            (200, r#"{"result":{"fields":[],"rows":[]}}"#.to_string()),
            (200, r#"{"result":{"fields":[],"rows":[]}}"#.to_string()),
        ]));
        let driver = AnyServerlessDriver::from_url(
            "mysql://user:pass@aws.connect.psdb.cloud/mydb",
            Arc::clone(&transport),
        )
        .unwrap();
        let db = ProviderDb::with_driver(driver, "planetscale");
        db.rows("posts").unwrap();
        // The first request is the one that makes the table.
        let request = transport.requests().remove(0);
        let body = request.json().unwrap();
        let query = body.get("query").and_then(|v| v.as_str()).unwrap();
        assert!(query.contains("`id` VARCHAR(191) PRIMARY KEY"), "{query}");
        assert!(query.contains("`data` LONGTEXT"), "{query}");
    }

    #[test]
    fn rows_come_back_as_documents() {
        let answer = neon_rows(
            r#"{"name":"data","dataTypeID":25}"#,
            r#"[["{\"id\":\"u1\",\"title\":\"hello\"}"]]"#,
        );
        let (driver, _transport) = neon(vec![(200, answer.clone()), (200, answer.clone())]);
        let db = ProviderDb::with_driver(driver, "neon");
        let rows = db.rows("posts").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("title").and_then(|v| v.as_str()), Some("hello"));
    }

    #[test]
    fn a_filter_is_the_matcher_over_the_rows() {
        let answer = neon_rows(
            r#"{"name":"data","dataTypeID":25}"#,
            r#"[["{\"id\":\"u1\",\"done\":true}"],["{\"id\":\"u2\",\"done\":false}"]]"#,
        );
        let (driver, _transport) = neon(vec![(200, answer.clone()), (200, answer.clone())]);
        let db = ProviderDb::with_driver(driver, "neon");
        let mut query = mosql::Object::new();
        query.set("done", Value::Bool(false));
        let rows = db.find("posts", &Value::Object(query), None, None).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("id").and_then(|v| v.as_str()), Some("u2"));
    }

    #[test]
    fn the_collections_are_the_tables_the_database_has() {
        let (driver, _transport) = neon(vec![(
            200,
            r#"[{"fields":[{"name":"tablename"}],"rows":[["c_posts"],["c_users"]]}]"#.to_string(),
        )]);
        let db = ProviderDb::with_driver(driver, "neon");
        let collections = db.collections().unwrap();
        assert_eq!(collections, vec!["posts".to_string(), "users".to_string()]);
    }
}
