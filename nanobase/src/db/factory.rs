//! Opening the database an app's document asks for.
//!
//! The document says which engine the rows are in. This reads that and
//! opens it: memory, a local SQLite file, a serverless database behind
//! an HTTP API, a database service on a socket, or a serverless database
//! speaking its own protocol.
//!
//! # How the engine is chosen
//!
//! The *kind* says what the app meant — `postgres`, `mysql`, `sqlite`,
//! `memory`, `libsql`, `d1`. The *URL* says how to reach it, and the URL
//! is what settles the last question: whether the rows are behind a
//! service that speaks this platform's protocol, or in the database
//! itself.
//!
//! | URL | what it opens |
//! | --- | --- |
//! | `libsql://`, `turso://` | Turso, in libSQL's own pipeline protocol |
//! | `d1://` | Cloudflare D1, in its query API |
//! | `postgres://`, `postgresql://` | Neon or Vercel Postgres, in its `/sql` API |
//! | `mysql://`, `tidb://`, `pscale://` | TiDB Cloud or PlanetScale, in their own APIs |
//! | `http://`, `https://` | a service that speaks this platform's app-database protocol |
//! | `tcp://` | a service on a socket, speaking the same protocol |
//! | none | memory, or a local SQLite file |
//!
//! So the same app may sit in a PostgreSQL database on one deployment and
//! in a SQLite file on the next, and the rows mean the same thing in
//! both: the collections are documents, the filters are `mosql`'s
//! matcher, and the operations are [`AppDb`]'s.
//!
//! The SQL engines needed no new dependency: [`mosql_core`] already
//! speaks all five protocols over HTTP, so the adapter here is a thin
//! layer that turns the platform's document operations into statements.

use std::path::Path;
use std::sync::Arc;

use uflite::error::{Error, Result};

use crate::config::{DatabaseConfig, DbKind};
#[cfg(feature = "curl")]
use crate::db::http::HttpDb;
use crate::db::memory::MemoryDb;
#[cfg(feature = "curl")]
use crate::db::provider::{is_provider_url, ProviderDb};
#[cfg(feature = "sqlite")]
use crate::db::sqlite::SqliteDb;
#[cfg(feature = "native")]
use crate::db::tcp::TcpDb;
use crate::db::AppDb;
#[cfg(feature = "curl")]
use crate::httpclient::HttpClient;

/// The client a caller may hand over, when this build has one.
///
/// A build without `curl` has no blocking HTTP client of its own — a
/// Worker is given one by its host, and this factory is not what opens
/// the Worker’s databases — so there is nothing to hand over and
/// nothing here can open a remote database.
#[cfg(feature = "curl")]
type MaybeClient = Option<Arc<dyn HttpClient>>;
#[cfg(not(feature = "curl"))]
type MaybeClient = Option<()>;

/// The prefix that says a database is on a socket rather than on the web.
#[cfg(feature = "native")]
const TCP_SCHEME: &str = "tcp://";

/// Open the database an app's configuration asks for.
///
/// `path` is where a local file goes: the app's own data directory. A
/// remote engine uses the URL in the configuration instead.
#[cfg_attr(not(feature = "sqlite"), allow(unused_variables))]
pub fn open(config: &DatabaseConfig, path: &Path) -> Result<Arc<dyn AppDb>> {
    open_inner(config, path, None)
}

/// Open the database an app's configuration asks for, over a client the
/// caller chose.
///
/// A caller that has its own HTTP client — a Worker with a `fetch`, a
/// test with a mock — hands it over here, and the request goes through
/// it. With no client, the host's own is used: `curl` for `https://`,
/// and the plain socket client for `http://`.
#[cfg(feature = "curl")]
#[cfg_attr(not(feature = "sqlite"), allow(unused_variables))]
pub fn open_with(
    config: &DatabaseConfig,
    path: &Path,
    client: Option<Arc<dyn HttpClient>>,
) -> Result<Arc<dyn AppDb>> {
    open_inner(config, path, client)
}

/// The opening itself, with or without a client.
#[cfg_attr(not(feature = "sqlite"), allow(unused_variables))]
#[cfg_attr(not(feature = "curl"), allow(unused_variables))]
fn open_inner(config: &DatabaseConfig, path: &Path, client: MaybeClient) -> Result<Arc<dyn AppDb>> {
    // A socket is a socket whatever the engine says: the protocol is the
    // app-database one, and it does not care which engine is behind it.
    // The scheme in the URL is what picks the transport.
    #[cfg(feature = "native")]
    if let Some(address) = config
        .url
        .as_deref()
        .and_then(|url| url.strip_prefix(TCP_SCHEME))
    {
        return open_tcp_at(config, address);
    }
    match config.kind {
        DbKind::None => Err(Error::internal(
            "this app has no database yet; point it at one first",
        )),
        DbKind::Memory => Ok(Arc::new(MemoryDb::new())),
        #[cfg(feature = "sqlite")]
        DbKind::Sqlite => Ok(Arc::new(SqliteDb::open(path.join("app.db"))?)),
        _ => {
            // A URL that names a provider is the database itself.
            #[cfg(feature = "curl")]
            {
                let url = config
                    .url
                    .as_deref()
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| {
                        Error::internal(format!("the {} engine needs a url", config.kind.as_str()))
                    })?;
                if is_provider_url(url) {
                    let client = client.unwrap_or_else(|| default_client(url));
                    let db = ProviderDb::new(&with_token(url, config.token.as_deref()), client)?;
                    return Ok(Arc::new(db));
                }
                // Anything else on the web is a service that speaks this
                // platform's own app-database protocol.
                let client = client.unwrap_or_else(|| default_client(url));
                let mut db = HttpDb::new(url, client);
                if let Some(token) = &config.token {
                    db = db.with_token(token.clone());
                }
                Ok(Arc::new(db))
            }
            #[cfg(not(feature = "curl"))]
            Err(Error::internal(
                "this build has no HTTP client; a remote database needs one",
            ))
        }
    }
}

/// The client the host uses for a URL.
///
/// `https://` needs TLS, and the TLS this build has is `curl`'s.
/// `http://` is a plain socket, and the plain socket client is enough.
#[cfg(feature = "curl")]
fn default_client(url: &str) -> Arc<dyn HttpClient> {
    if url.starts_with("https://") {
        Arc::new(crate::httpclient::CurlHttpClient)
    } else {
        #[cfg(feature = "native")]
        {
            Arc::new(crate::httpclient::PlainHttpClient)
        }
        #[cfg(not(feature = "native"))]
        {
            Arc::new(crate::httpclient::CurlHttpClient)
        }
    }
}

/// The URL with the token folded in.
///
/// A token in the configuration and a token in the URL are the same
/// secret in two places, and the URL wins when it has one — a URL with
/// credentials in it is a complete connection string. When the URL has
/// no credentials, the token goes where the provider expects it: in the
/// query for libSQL and D1, and in the user information for the SQL
/// engines.
///
/// What "has credentials" means is the provider's own: for libSQL the
/// user information *is* the token, for D1 it is the account, and for
/// the SQL engines it is the user name and the password.
///
/// A URL with no credentials of its own and a token to go with it gets
/// the token where the provider expects it. A URL that already has
/// credentials is left alone: it is a complete connection string.
#[cfg(feature = "curl")]
pub fn with_token(url: &str, token: Option<&str>) -> String {
    let Some(token) = token.filter(|t| !t.is_empty()) else {
        return url.to_string();
    };
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let (authority, after) = match rest.find(['/', '?']) {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, ""),
    };
    let scheme = scheme.to_ascii_lowercase();
    let encoded = percent_encode(token);
    // The user information, if the URL has any, and the host without it.
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((userinfo, hostport)) => (Some(userinfo), hostport),
        None => (None, authority),
    };
    match scheme.as_str() {
        // libSQL: the token is the user information, or `?authToken=`.
        "libsql" | "turso" => {
            if userinfo.is_some() || after.contains("authToken=") {
                return url.to_string();
            }
            let separator = if after.contains('?') { '&' } else { '?' };
            format!("{url}{separator}authToken={encoded}")
        }
        // D1: `account:token` in the user information, or `?api_token=`.
        // A user information with only an account still needs the token.
        "d1" | "cloudflare" => {
            let has_token = userinfo.map(|u| u.contains(':')).unwrap_or(false);
            if has_token || after.contains("api_token=") || after.contains("token=") {
                return url.to_string();
            }
            let separator = if after.contains('?') { '&' } else { '?' };
            format!("{url}{separator}api_token={encoded}")
        }
        // The SQL engines: the token is the password, and the port stays
        // where it was.
        _ => {
            let has_password = userinfo.map(|u| u.contains(':')).unwrap_or(false);
            if has_password {
                return url.to_string();
            }
            let (host, port) = match hostport.rsplit_once(':') {
                Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => {
                    (host, Some(port))
                }
                _ => (hostport, None),
            };
            // The user name, and the colon that says the token is the
            // password. A URL with no user name still has the password
            // written after the colon: `postgres://:secret@host`.
            let user = match userinfo {
                Some(user) if !user.is_empty() => format!("{user}:"),
                _ => ":".to_string(),
            };
            match port {
                Some(port) => {
                    format!("{scheme}://{user}{encoded}@{host}:{port}{after}")
                }
                None => format!("{scheme}://{user}{encoded}@{host}{after}"),
            }
        }
    }
}

/// A secret, written the way a URL wants it.
#[cfg(feature = "curl")]
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The database an app's configuration asks for, over a socket.
///
/// The URL is a `host:port`, so a database service on the same network
/// answers the same protocol the HTTP service does.
#[cfg(feature = "native")]
pub fn open_tcp(config: &DatabaseConfig) -> Result<Arc<dyn AppDb>> {
    match config.kind {
        DbKind::Postgres | DbKind::Mysql | DbKind::Libsql | DbKind::D1 => {
            let address = config
                .url
                .as_deref()
                .ok_or_else(|| Error::internal("this database needs a url"))?;
            open_tcp_at(config, address)
        }
        other => Err(Error::internal(format!(
            "the {} engine is not reached over a socket",
            other.as_str()
        ))),
    }
}

/// The same, at an address the caller already knows.
#[cfg(feature = "native")]
fn open_tcp_at(config: &DatabaseConfig, address: &str) -> Result<Arc<dyn AppDb>> {
    let mut db = TcpDb::new(address);
    if let Some(token) = &config.token {
        db = db.with_token(token.clone());
    }
    Ok(Arc::new(db))
}

#[cfg(all(test, feature = "curl"))]
mod tests {
    use super::*;

    #[test]
    fn a_token_goes_where_the_provider_expects_it() {
        // libSQL takes it in the query.
        assert_eq!(
            with_token("libsql://db.turso.io", Some("s3cret")),
            "libsql://db.turso.io?authToken=s3cret"
        );
        // D1 takes it in the query too.
        assert_eq!(
            with_token("d1://acct@api.cloudflare.com/mydb", Some("s3cret")),
            "d1://acct@api.cloudflare.com/mydb?api_token=s3cret"
        );
        // The SQL engines take it as the password.
        assert_eq!(
            with_token("postgres://host/mydb", Some("s3cret")),
            "postgres://:s3cret@host/mydb"
        );
        // A user name is kept.
        assert_eq!(
            with_token("postgres://me@host/mydb", Some("s3cret")),
            "postgres://me:s3cret@host/mydb"
        );
        // A port stays a port.
        assert_eq!(
            with_token("postgres://host:5432/mydb", Some("s3cret")),
            "postgres://:s3cret@host:5432/mydb"
        );
    }

    #[test]
    fn a_url_that_already_has_credentials_keeps_them() {
        let url = "postgres://user:pass@host/mydb";
        assert_eq!(with_token(url, Some("other")), url);
        assert_eq!(
            with_token("libsql://db.turso.io?authToken=one", Some("two")),
            "libsql://db.turso.io?authToken=one"
        );
        // libSQL's user information is the token.
        assert_eq!(
            with_token("libsql://one@db.turso.io", Some("two")),
            "libsql://one@db.turso.io"
        );
        // D1's account is not a token.
        assert_eq!(
            with_token("d1://acct:one@api.cloudflare.com/mydb", Some("two")),
            "d1://acct:one@api.cloudflare.com/mydb"
        );
    }

    #[test]
    fn a_token_is_written_the_way_a_url_wants_it() {
        assert_eq!(percent_encode("a b/c"), "a%20b%2Fc");
        assert_eq!(percent_encode("plain-Text_1.0~"), "plain-Text_1.0~");
        assert_eq!(percent_encode("p@ss:w/rd"), "p%40ss%3Aw%2Frd");
    }

    #[test]
    fn memory_needs_no_url() {
        let config = DatabaseConfig {
            kind: DbKind::Memory,
            ..Default::default()
        };
        let db = open(&config, Path::new("/tmp/nanobase-does-not-use-this")).unwrap();
        assert_eq!(db.kind(), "memory");
    }
}
