//! The connection URL: parsed, and the provider worked out from it.
//!
//! One URL says everything: which provider, which database, which
//! credentials. The rules are:
//!
//! | URL | provider | dialect |
//! | --- | --- | --- |
//! | `libsql://host?authToken=…`, `turso://…`, `https://*.turso.io` | Turso | SQLite |
//! | `d1://account:token@api.cloudflare.com/database` | Cloudflare D1 | SQLite |
//! | `postgres://…`, `postgresql://…` | Neon / Vercel | PostgreSQL |
//! | `mysql://….tidbcloud.com/…`, `tidb://…` | TiDB Cloud | MySQL |
//! | `mysql://….psdb.cloud/…`, `pscale://…` | PlanetScale | MySQL |
//!
//! and `?provider=tidb` or `?provider=planetscale` settles any
//! ambiguity, on any scheme.
//!
//! A `mysql://` URL whose host is neither a TiDB Cloud host nor a
//! PlanetScale host is **refused**, with a message that says so. Both
//! speak MySQL over HTTP, but they are not interchangeable: their
//! endpoints, their sessions and their errors are different, and
//! guessing would send a caller to the wrong place with no warning.
//! `?provider=tidb` or `?provider=planetscale` says which one.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::driver::encoding::{percent_decode, percent_encode};
use crate::driver::{DbError, Dialect, Provider};

/// Everything a driver needs to know about a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverConfig {
    /// Which provider this is.
    pub provider: Provider,
    /// Which SQL dialect the provider speaks.
    pub dialect: Dialect,
    /// The endpoint to post to.
    pub endpoint: String,
    /// The user name, if the URL had one.
    pub username: Option<String>,
    /// The password, if the URL had one.
    pub password: Option<String>,
    /// The host, without a port.
    pub host: String,
    /// The port, if the URL had one.
    pub port: Option<u16>,
    /// The database name, from the path.
    pub database: Option<String>,
    /// The bearer token, for the providers that use one.
    pub token: Option<String>,
    /// The original URL, normalised.
    ///
    /// Neon and Vercel want the whole connection string back in a
    /// header, so it has to survive the trip.
    pub connection_string: String,
}

impl DriverConfig {
    /// Read a connection URL and work out which provider it is.
    pub fn from_url(raw_url: &str) -> Result<DriverConfig, DbError> {
        Self::from_url_with_provider(raw_url, None)
    }

    /// Read a connection URL, and take the provider as given.
    ///
    /// A URL says a good deal about which cloud it is for — the
    /// scheme, the host, and sometimes a `?provider=` of its own — but
    /// a URL is not always enough. A gateway in front of a database, a
    /// host that looks like nothing in particular, a proxy that
    /// rewrites the address: there are deployments where only the
    /// operator knows which cloud it is.
    ///
    /// When the provider is given, it is the provider: the dialect, the
    /// endpoint and the token all follow it, and the URL is only the
    /// connection string. When the URL says `?provider=` as well, the
    /// two have to agree.
    pub fn from_url_with_provider(
        raw_url: &str,
        provider: Option<Provider>,
    ) -> Result<DriverConfig, DbError> {
        let raw = raw_url.trim();
        if raw.is_empty() {
            return Err(DbError::new("the connection URL is empty"));
        }
        let (scheme, rest) = raw
            .split_once("://")
            .ok_or_else(|| DbError::new("the connection URL has no scheme"))?;
        let scheme = scheme.to_ascii_lowercase();

        // The authority, the path and the query.
        let (authority, after_authority) = match rest.find(['/', '?']) {
            Some(index) => (&rest[..index], &rest[index..]),
            None => (rest, ""),
        };
        let (path, query) = match after_authority.find('?') {
            Some(index) => (&after_authority[..index], &after_authority[index + 1..]),
            None => (after_authority, ""),
        };

        let params = parse_query(query);

        // The path, in pieces: a database name, or an account and a
        // database name, depending on the provider.
        let mut segments: Vec<String> = Vec::new();
        for piece in path.trim_start_matches('/').split('/') {
            if piece.is_empty() {
                continue;
            }
            segments.push(percent_decode(piece)?);
        }

        // The user info: `user:password@host`.
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((userinfo, hostport)) => (Some(userinfo), hostport),
            None => (None, authority),
        };
        let (username, password) = match userinfo {
            Some(userinfo) => match userinfo.split_once(':') {
                Some((user, password)) => (
                    non_empty(&percent_decode(user)?),
                    non_empty(&percent_decode(password)?),
                ),
                None => (non_empty(&percent_decode(userinfo)?), None),
            },
            None => (None, None),
        };

        // The host and the port.
        let (host, port) = split_host_port(hostport)?;

        let provider = detect_provider(&scheme, &host, &params, provider)?;

        let mut config = DriverConfig {
            provider,
            dialect: provider.dialect(),
            endpoint: String::new(),
            username,
            password,
            host,
            port,
            database: non_empty(&percent_decode(path.trim_start_matches('/'))?),
            token: None,
            connection_string: String::new(),
        };

        // The provider's own rules for the rest.
        match provider {
            Provider::Turso => {
                // The token is either in the user info (`libsql://token@host`)
                // or in the query (`?authToken=…`).
                // The query was decoded when it was parsed, and so was
                // the user info: decoding either of them again would
                // change a token that happens to carry a `%`.
                config.token = params
                    .iter()
                    .find(|(key, _)| key == "authToken")
                    .map(|(_, value)| value.clone())
                    .or_else(|| config.username.clone());
                if config.host.is_empty() {
                    return Err(DbError::new("the URL names no host"));
                }
                config.endpoint = format!("https://{}/v2/pipeline", config.host);
            }
            Provider::D1 => {
                // Three layouts, all of them seen in the wild:
                //
                //   d1://<account>:<token>@api.cloudflare.com/<database>
                //   d1://<token>@api.cloudflare.com/<account>/<database>
                //   d1://api.cloudflare.com/<account>/<database>
                //       ?account_id=…&api_token=…
                //
                // The first two are the documented ones: the account and
                // the token in the user info, with the database in the
                // path — or the token alone in the user info, with the
                // account and the database in the path.
                let mut account = query_value(&params, &["account_id", "account"]);
                let mut token = query_value(&params, &["api_token", "token"]);
                let mut database: Option<String> = None;

                match (&config.username, &config.password) {
                    // `account:token`
                    (Some(user), Some(pass)) => {
                        if account.is_none() {
                            account = Some(user.clone());
                        }
                        if token.is_none() {
                            token = Some(pass.clone());
                        }
                    }
                    // `token` on its own.
                    (Some(user), None) => {
                        if token.is_none() {
                            token = Some(user.clone());
                        }
                    }
                    (None, Some(pass)) => {
                        if token.is_none() {
                            token = Some(pass.clone());
                        }
                    }
                    (None, None) => {}
                }

                // The path: a database, or an account and a database.
                match segments.len() {
                    0 => {}
                    1 => database = Some(segments[0].clone()),
                    _ => {
                        if account.is_none() {
                            account = Some(segments[0].clone());
                        }
                        database = Some(segments[1].clone());
                    }
                }

                let account =
                    account.ok_or_else(|| DbError::new("the URL names no Cloudflare account"))?;
                let token =
                    token.ok_or_else(|| DbError::new("the URL names no Cloudflare API token"))?;
                let database =
                    database.ok_or_else(|| DbError::new("the URL names no D1 database"))?;
                config.token = Some(token);
                config.database = Some(database.clone());
                config.endpoint = format!(
                    "https://api.cloudflare.com/client/v4/accounts/{account}/d1/database/{database}/query"
                );
            }
            Provider::Neon => {
                // The whole connection string goes back in a header.
                config.connection_string = normalise_postgres(&config, &params);
                let endpoint = params
                    .iter()
                    .find(|(key, _)| key == "fetch_endpoint")
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| format!("https://{}/sql", config.host));
                config.endpoint = endpoint;
            }
            Provider::Tidb => {
                // TiDB puts a proxy in front of the database, and the
                // proxy's host is the database's host with `http-` in
                // front of it.
                config.endpoint = format!("https://http-{}/v1beta/sql", config.host);
            }
            Provider::PlanetScale => {
                config.endpoint = format!("https://{}/psdb.v1alpha1.Database/Execute", config.host);
            }
        }

        Ok(config)
    }

    /// The `Authorization` header value, if the provider uses one.
    pub fn bearer_token(&self) -> Option<&str> {
        self.token.as_deref()
    }

    /// The `Authorization: Basic` header value, if the provider uses
    /// one.
    pub fn basic_auth(&self) -> Option<String> {
        // A URL with only a token still has a credential: the user name
        // may be empty, and the token is the password. `postgres://:tok@host`
        // says "the password is tok", and dropping it would send the
        // request with no secret at all.
        if self.username.is_none() && self.password.is_none() {
            return None;
        }
        let raw = format!(
            "{}:{}",
            self.username.as_deref().unwrap_or_default(),
            self.password.as_deref().unwrap_or_default()
        );
        Some(format!(
            "Basic {}",
            crate::driver::encoding::base64_encode(raw.as_bytes())
        ))
    }
}

/// Which provider a URL is for.
///
/// The provider may be given — by the caller, because the operator said
/// so — and then the URL is only the connection string. Otherwise it is
/// worked out from the scheme, the host, and a `?provider=` in the
/// query.
pub fn detect_provider(
    scheme: &str,
    host: &str,
    params: &[(String, String)],
    given: Option<Provider>,
) -> Result<Provider, DbError> {
    // A `?provider=` in the URL settles it, whatever the scheme.
    if let Some((_, wanted)) = params.iter().find(|(key, _)| key == "provider") {
        let wanted = Provider::from_name(wanted).ok_or_else(|| {
            DbError::new(format!(
                "there is no provider called {wanted:?}; the providers are {}",
                Provider::names().join(", ")
            ))
        })?;
        // And a provider given by the caller has to agree with it: two
        // instructions that contradict each other are a mistake, not a
        // thing to guess about.
        if let Some(given) = given {
            if given != wanted {
                return Err(DbError::new(format!(
                    "the URL says the provider is {} and the configuration says it is {}",
                    wanted.as_str(),
                    given.as_str()
                )));
            }
        }
        return Ok(wanted);
    }
    // A caller that knows which cloud it is talking to does not have to
    // be guessed at.
    if let Some(given) = given {
        return Ok(given);
    }
    match scheme {
        "libsql" | "turso" => Ok(Provider::Turso),
        "d1" | "cloudflare" => Ok(Provider::D1),
        "postgres" | "postgresql" => Ok(Provider::Neon),
        "tidb" => Ok(Provider::Tidb),
        "pscale" | "planetscale" => Ok(Provider::PlanetScale),
        "https" | "http" => {
            if host.ends_with(".turso.io") {
                Ok(Provider::Turso)
            } else {
                Err(DbError::new(
                    "the URL names no provider: use a libsql://, d1://, postgres://, tidb:// or pscale:// URL, or say ?provider=",
                ))
            }
        }
        "mysql" => {
            if host.ends_with(".tidbcloud.com") || host.ends_with(".tidbcloud.net") {
                Ok(Provider::Tidb)
            } else if host.ends_with(".psdb.cloud") || host.ends_with(".planetscale.com") {
                Ok(Provider::PlanetScale)
            } else {
                // The two MySQL providers speak the same dialect, but
                // they are not interchangeable: the endpoints, the
                // sessions and the errors are all different. So a host
                // that is neither is refused, and the message says how
                // to say which one it is.
                Err(DbError::new(format!(
                    "the MySQL host {host:?} is neither a TiDB Cloud host nor a PlanetScale host: use ?provider=tidb or ?provider=planetscale to say which one"
                )))
            }
        }
        other => Err(DbError::new(format!(
            "the scheme {other:?} is not one this crate knows: use libsql://, d1://, postgres://, mysql://, tidb:// or pscale://"
        ))),
    }
}

/// The first value of the first of these query parameters.
fn query_value(params: &[(String, String)], names: &[&str]) -> Option<String> {
    for name in names {
        if let Some((_, value)) = params.iter().find(|(key, _)| key == name) {
            if !value.is_empty() {
                return Some(value.clone());
            }
        }
    }
    None
}

/// Split a host and a port.
fn split_host_port(hostport: &str) -> Result<(String, Option<u16>), DbError> {
    if hostport.is_empty() {
        return Ok((String::new(), None));
    }
    // An IPv6 address is in brackets.
    if let Some(rest) = hostport.strip_prefix('[') {
        let (host, after) = rest
            .split_once(']')
            .ok_or_else(|| DbError::new("the URL has an unclosed IPv6 address"))?;
        let port = match after.strip_prefix(':') {
            Some(port) if !port.is_empty() => Some(parse_port(port)?),
            _ => None,
        };
        return Ok((format!("[{host}]"), port));
    }
    match hostport.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            Ok((host.to_string(), Some(parse_port(port)?)))
        }
        _ => Ok((hostport.to_string(), None)),
    }
}

/// A port number.
fn parse_port(text: &str) -> Result<u16, DbError> {
    text.parse::<u16>()
        .map_err(|_| DbError::new(format!("{text:?} is not a port number")))
}

/// The query parameters of a URL, decoded.
fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        let key = percent_decode(key).unwrap_or_else(|_| key.to_string());
        let value = percent_decode(value).unwrap_or_else(|_| value.to_string());
        out.push((key, value));
    }
    out
}

/// An empty string is no value.
fn non_empty(text: &str) -> Option<String> {
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// The Postgres connection string, as Neon and Vercel want it.
///
/// They take the whole thing — scheme, user, password, host, port,
/// database — in a header, and they are fussy about it. So the string is
/// put back together from the parts that were parsed: a password with
/// an `@` in it has to arrive as `%40`, or the string means something
/// else.
fn normalise_postgres(config: &DriverConfig, params: &[(String, String)]) -> String {
    let mut out = String::from("postgres://");
    // The user information is written when there is a user name *or* a
    // password: a URL with only a token — `postgres://:tok@host/db` —
    // keeps its token, because the token is the credential.
    if config.username.is_some() || config.password.is_some() {
        if let Some(user) = &config.username {
            out.push_str(&percent_encode(user));
        }
        if let Some(password) = &config.password {
            out.push(':');
            out.push_str(&percent_encode(password));
        }
        out.push('@');
    }
    out.push_str(&config.host);
    if let Some(port) = config.port {
        out.push(':');
        out.push_str(&port.to_string());
    }
    if let Some(database) = &config.database {
        out.push('/');
        out.push_str(&percent_encode(database));
    }
    if !params.is_empty() {
        out.push('?');
        let mut first = true;
        for (key, value) in params {
            if !first {
                out.push('&');
            }
            first = false;
            out.push_str(&percent_encode(key));
            out.push('=');
            out.push_str(&percent_encode(value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider_of(url: &str) -> Provider {
        DriverConfig::from_url(url)
            .expect("a configuration")
            .provider
    }

    #[test]
    fn turso_urls() {
        assert_eq!(provider_of("libsql://db.turso.io"), Provider::Turso);
        assert_eq!(provider_of("turso://db.turso.io"), Provider::Turso);
        assert_eq!(provider_of("https://db.turso.io"), Provider::Turso);
        let config = DriverConfig::from_url("libsql://db.turso.io?authToken=secret").unwrap();
        assert_eq!(config.token.as_deref(), Some("secret"));
        assert_eq!(config.endpoint, "https://db.turso.io/v2/pipeline");
        assert_eq!(config.dialect, Dialect::Sqlite);
    }

    #[test]
    fn a_turso_token_may_be_in_the_user_info() {
        let config = DriverConfig::from_url("libsql://token@db.turso.io").unwrap();
        assert_eq!(config.token.as_deref(), Some("token"));
    }

    #[test]
    fn a_turso_token_is_decoded_once() {
        // The user info and the query are decoded when the URL is
        // parsed. Decoding either of them a second time would change a
        // token that carries a `%` of its own.
        let config = DriverConfig::from_url("libsql://a%25b@db.turso.io").unwrap();
        assert_eq!(config.token.as_deref(), Some("a%b"));
        let config = DriverConfig::from_url("libsql://db.turso.io?authToken=a%25b").unwrap();
        assert_eq!(config.token.as_deref(), Some("a%b"));
    }

    #[test]
    fn a_d1_url() {
        let config =
            DriverConfig::from_url("d1://account123:token456@api.cloudflare.com/mydb").unwrap();
        assert_eq!(config.provider, Provider::D1);
        assert_eq!(config.dialect, Dialect::Sqlite);
        assert_eq!(config.token.as_deref(), Some("token456"));
        assert_eq!(
            config.endpoint,
            "https://api.cloudflare.com/client/v4/accounts/account123/d1/database/mydb/query"
        );
    }

    #[test]
    fn a_d1_url_with_the_token_on_its_own() {
        // The second documented layout: the token in the user info, the
        // account and the database in the path.
        let config =
            DriverConfig::from_url("d1://token456@api.cloudflare.com/account123/mydb").unwrap();
        assert_eq!(config.provider, Provider::D1);
        assert_eq!(config.token.as_deref(), Some("token456"));
        assert_eq!(
            config.endpoint,
            "https://api.cloudflare.com/client/v4/accounts/account123/d1/database/mydb/query"
        );
    }

    #[test]
    fn a_d1_url_with_the_account_and_the_token_in_the_query() {
        let config = DriverConfig::from_url(
            "d1://api.cloudflare.com/account123/mydb?account_id=account123&api_token=token456",
        )
        .unwrap();
        assert_eq!(config.token.as_deref(), Some("token456"));
        assert_eq!(
            config.endpoint,
            "https://api.cloudflare.com/client/v4/accounts/account123/d1/database/mydb/query"
        );
    }

    #[test]
    fn a_d1_url_with_nothing_in_it_is_refused() {
        let err = DriverConfig::from_url("d1://api.cloudflare.com/mydb").unwrap_err();
        assert!(err.message.contains("account"), "{}", err.message);
        let err = DriverConfig::from_url("d1://acct:token@api.cloudflare.com").unwrap_err();
        assert!(err.message.contains("database"), "{}", err.message);
    }

    #[test]
    fn a_neon_url() {
        let config = DriverConfig::from_url(
            "postgres://user:pass@ep-cool.us-east-2.aws.neon.tech/mydb?sslmode=require",
        )
        .unwrap();
        assert_eq!(config.provider, Provider::Neon);
        assert_eq!(config.dialect, Dialect::Postgres);
        assert_eq!(
            config.endpoint,
            "https://ep-cool.us-east-2.aws.neon.tech/sql"
        );
        assert_eq!(config.username.as_deref(), Some("user"));
        assert_eq!(config.password.as_deref(), Some("pass"));
        assert_eq!(config.database.as_deref(), Some("mydb"));
        assert!(config
            .connection_string
            .starts_with("postgres://user:pass@"));
    }

    #[test]
    fn a_postgresql_url_is_the_same_as_a_postgres_url() {
        assert_eq!(
            provider_of("postgresql://user:pass@host/db"),
            Provider::Neon
        );
    }

    #[test]
    fn a_neon_endpoint_may_be_overridden() {
        let config = DriverConfig::from_url(
            "postgres://user:pass@host/db?fetch_endpoint=https://other.example/sql",
        )
        .unwrap();
        assert_eq!(config.endpoint, "https://other.example/sql");
    }

    #[test]
    fn a_tidb_url() {
        let config = DriverConfig::from_url(
            "mysql://user:pass@gateway-01.us-east-1.aws.tidbcloud.com:4000/mydb",
        )
        .unwrap();
        assert_eq!(config.provider, Provider::Tidb);
        assert_eq!(config.dialect, Dialect::Mysql);
        assert_eq!(config.host, "gateway-01.us-east-1.aws.tidbcloud.com");
        assert_eq!(config.port, Some(4000));
        assert_eq!(config.database.as_deref(), Some("mydb"));
        assert_eq!(
            config.endpoint,
            "https://http-gateway-01.us-east-1.aws.tidbcloud.com/v1beta/sql"
        );
    }

    #[test]
    fn a_tidb_url_by_scheme() {
        assert_eq!(provider_of("tidb://user:pass@host/db"), Provider::Tidb);
    }

    #[test]
    fn a_planetscale_url() {
        let config =
            DriverConfig::from_url("mysql://user:pass@aws.connect.psdb.cloud/mydb").unwrap();
        assert_eq!(config.provider, Provider::PlanetScale);
        assert_eq!(config.dialect, Dialect::Mysql);
        assert_eq!(
            config.endpoint,
            "https://aws.connect.psdb.cloud/psdb.v1alpha1.Database/Execute"
        );
    }

    #[test]
    fn a_planetscale_url_by_scheme() {
        assert_eq!(
            provider_of("pscale://user:pass@host/db"),
            Provider::PlanetScale
        );
    }

    #[test]
    fn a_mysql_url_with_an_unknown_host_is_refused() {
        // Guessing would send the caller to the wrong provider with no
        // warning, so the URL is refused and the message says what to do.
        let err = DriverConfig::from_url("mysql://user:pass@db.example.com/mydb").unwrap_err();
        assert!(err.message.contains("db.example.com"), "{}", err.message);
        assert!(err.message.contains("provider=tidb"), "{}", err.message);
    }

    #[test]
    fn a_mysql_url_with_an_unknown_host_may_name_its_provider() {
        let config =
            DriverConfig::from_url("mysql://user:pass@db.example.com/mydb?provider=planetscale")
                .unwrap();
        assert_eq!(config.provider, Provider::PlanetScale);
        let config =
            DriverConfig::from_url("mysql://user:pass@db.example.com/mydb?provider=tidb").unwrap();
        assert_eq!(config.provider, Provider::Tidb);
    }

    #[test]
    fn the_provider_may_be_named() {
        assert_eq!(
            provider_of("mysql://user:pass@db.example.com/mydb?provider=planetscale"),
            Provider::PlanetScale
        );
        assert_eq!(
            provider_of("libsql://host?provider=tidb"),
            Provider::Tidb,
            "the override works on any scheme"
        );
    }

    #[test]
    fn a_password_with_punctuation_survives() {
        let config = DriverConfig::from_url("postgres://user:p%40ss%2Fw%3Ard@host/db").unwrap();
        assert_eq!(config.password.as_deref(), Some("p@ss/w:rd"));
    }

    #[test]
    fn the_connection_string_is_put_back_together() {
        // The string Neon is given has to mean the same thing as the
        // URL it came from, so the password goes back in encoded.
        let config = DriverConfig::from_url(
            "postgres://user:p%40ss@ep-cool.us-east-2.aws.neon.tech:5432/mydb?sslmode=require",
        )
        .unwrap();
        assert_eq!(
            config.connection_string,
            "postgres://user:p%40ss@ep-cool.us-east-2.aws.neon.tech:5432/mydb?sslmode=require"
        );
    }

    #[test]
    fn an_unknown_scheme_is_refused() {
        let err = DriverConfig::from_url("redis://host").unwrap_err();
        assert!(err.message.contains("redis"), "{}", err.message);
    }

    #[test]
    fn an_empty_url_is_refused() {
        assert!(DriverConfig::from_url("").is_err());
        assert!(DriverConfig::from_url("   ").is_err());
    }

    #[test]
    fn a_url_with_no_scheme_is_refused() {
        assert!(DriverConfig::from_url("db.turso.io").is_err());
    }

    #[test]
    fn a_token_on_its_own_is_still_a_credential() {
        // A URL with a token and no user name: the token is the
        // password, and it must survive the trip.
        let config = DriverConfig::from_url("postgres://:s3cret@host/mydb").unwrap();
        assert_eq!(config.username, None);
        assert_eq!(config.password.as_deref(), Some("s3cret"));
        assert_eq!(config.connection_string, "postgres://:s3cret@host/mydb");
        // And it is sent: the basic auth header carries it.
        assert_eq!(config.basic_auth().as_deref(), Some("Basic OnMzY3JldA=="));
    }

    #[test]
    fn a_token_only_sql_url_reaches_its_driver() {
        let config = DriverConfig::from_url("mysql://:s3cret@host/db?provider=tidb").unwrap();
        assert_eq!(config.password.as_deref(), Some("s3cret"));
        assert!(config.basic_auth().is_some());
    }

    #[test]
    fn a_provider_may_be_given_rather_than_guessed() {
        // A URL that says nothing about which cloud it is: the
        // operator says, and the operator is right.
        let config = DriverConfig::from_url_with_provider(
            "https://db.example.com/mydb",
            Some(Provider::Neon),
        )
        .unwrap();
        assert_eq!(config.provider, Provider::Neon);
        assert_eq!(config.dialect, Dialect::Postgres);
        assert_eq!(config.endpoint, "https://db.example.com/sql");

        // The same URL, read as MySQL, is a different database.
        let config = DriverConfig::from_url_with_provider(
            "https://db.example.com/mydb",
            Some(Provider::Tidb),
        )
        .unwrap();
        assert_eq!(config.dialect, Dialect::Mysql);
        assert_eq!(config.endpoint, "https://http-db.example.com/v1beta/sql");
    }

    #[test]
    fn a_url_that_says_a_provider_and_a_caller_that_says_one_must_agree() {
        let config = DriverConfig::from_url_with_provider(
            "libsql://db.turso.io?provider=turso",
            Some(Provider::Turso),
        )
        .unwrap();
        assert_eq!(config.provider, Provider::Turso);

        let err = DriverConfig::from_url_with_provider(
            "libsql://db.turso.io?provider=turso",
            Some(Provider::Neon),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("the URL says"),
            "the contradiction is said out loud: {err}"
        );
    }

    #[test]
    fn a_provider_may_be_named() {
        assert_eq!(Provider::from_name("Neon"), Some(Provider::Neon));
        assert_eq!(Provider::from_name(" turso "), Some(Provider::Turso));
        assert_eq!(Provider::from_name("libsql"), Some(Provider::Turso));
        assert_eq!(Provider::from_name("d1"), Some(Provider::D1));
        assert_eq!(Provider::from_name("tidb"), Some(Provider::Tidb));
        assert_eq!(
            Provider::from_name("planetscale"),
            Some(Provider::PlanetScale)
        );
        assert_eq!(Provider::from_name("sqlite"), None);
    }

    #[test]
    fn a_url_with_no_credential_has_no_basic_auth() {
        let config = DriverConfig::from_url("postgres://host/mydb").unwrap();
        assert_eq!(config.basic_auth(), None);
    }

    #[test]
    fn basic_auth_is_base64() {
        let config = DriverConfig::from_url("mysql://user:pass@host/db?provider=tidb").unwrap();
        assert_eq!(config.basic_auth().as_deref(), Some("Basic dXNlcjpwYXNz"));
    }
}
