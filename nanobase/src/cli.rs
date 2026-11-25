//! `nanobase` — the command line.
//!
//! Two halves today: the app document (`new`, `validate`, `redact`) and the
//! platform's own account (`init`, `login`, `me`, `status`). The server, and
//! creating apps through it, are added on top of the same types.
//!
//! Setup is interactive when it can be and scripted when it must be: `init`
//! prompts for an email and password if they are not given as flags, and never
//! prompts when they are, so a CI script or a provisioning tool can run it.
//!
//! The module lives in the library, not in the binary, because there are two
//! binaries: the server build, which listens on a socket and keeps the
//! platform in a local SQLite file, and the serverless build, which has
//! neither and keeps the platform in a serverless database. Both run the
//! same commands.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::config::DbKind;
use crate::db::AppDb;
use crate::platform::PlatformStore;
#[cfg(feature = "native")]
use crate::Api;
#[cfg(feature = "curl")]
use crate::ServerlessStore;
#[cfg(any(feature = "native", feature = "serverless"))]
use crate::Service;
#[cfg(feature = "sqlite")]
use crate::SqliteStore;
use crate::{
    check_email, create_owner, issue_token, now_secs, verify_login, ApiClient, AppDoc, ConfigError,
    Platform, Secrets, VERSION,
};
use mosql::Value;
use uflite::db::block_on;

/// The usage text, which differs between the two editions: the server
/// build has a local file and a listener, and the serverless build has
/// neither.
fn usage() -> String {
    let mut text = String::from(
        r#"nanobase — a PocketBase-style backend on your own database

USAGE:
    nanobase init   [--email E] [--password P]                  create the owner
    nanobase login  <email> [--password P]                      print a session token
    nanobase me     [--token T]                                 show who a token belongs to
    nanobase status                                                setup state, app count

"#,
    );
    if cfg!(all(feature = "native", feature = "sqlite")) {
        text.push_str(
            r#"SERVER:
    nanobase serve [--host H] [--port P] [--data FILE]        run the HTTP API
    nanobase serve --host 0.0.0.0 --port 8090
    nanobase db serve [--host H] [--port P] [--db FILE]       serve the database over HTTP

"#,
        );
    }
    text.push_str(
        r#"KEYS:
    nanobase key list                                         the API keys
    nanobase key create --name N [--app A] [--scopes a,b]     make a key
                      [--expires-in 30d]
    nanobase key show <id>                                    one key, in detail
    nanobase key remove <id>                                  remove a key

"#,
    );
    text.push_str(
        r#"APPS:
    nanobase app new <name>                                   create an empty app
    nanobase app import <file>                                create an app from a document
    nanobase app list                                         id, name, engine
    nanobase app show <id|name>                               everything about one app
    nanobase app export <id|name> [--out F] [--with-secrets]  write an app's document
    nanobase app rename <id|name> <new-name>
    nanobase app clone <id|name> <new-name> [--copy-data]      copy config and schema
                                     [--share-database]         the rows, or the database itself
                                     [--database URL]           the clone's own database
    nanobase app remove <id|name> --confirm <id>
    nanobase app rows <id|name> <collection> [--filter JSON] [--limit N]
    nanobase app insert <id|name> <collection> --json ROW
    nanobase app update <id|name> <collection> --filter JSON --json CHANGES
    nanobase app delete-rows <id|name> <collection> [--filter JSON]
    nanobase new <name> [--out <file>]                         write a new, empty app document
    nanobase validate <file>                                   check a document, list every problem
    nanobase redact <file> [--out <file>]                      copy a document without its secrets
    nanobase --version
    nanobase --help

"#,
    );
    text.push_str(
        r#"PLATFORM DATABASE:
    --data FILE       where the platform keeps its state (SQLite)
    NANOBASE_DATA     used when --data is absent
    ./data/nanobase.db     the default
    --db-url URL      a serverless database the platform keeps its state in
    NANOBASE_DB_URL   used when --db-url is absent
    --db-token TOKEN  the bearer token for that database
    NANOBASE_DB_TOKEN used when --db-token is absent

    The URL says which database: postgres:// and mysql:// name the
    database itself, and http:// names a service that holds one. With
    --db-url the platform's own state lives there and the local file is
    not used.

"#,
    );
    text.push_str(
        r#"REMOTE PLATFORM:
    --url URL         talk to a running platform instead of the local file
    NANOBASE_URL      used when --url is absent
    --token TOKEN     the bearer token to send
    NANOBASE_TOKEN    used when --token is absent

    The database holds the owner's password hash and the key tokens are
    signed with. Protect it: anyone who can read it can sign in.

EXAMPLES:
    nanobase init
    nanobase init --email ada@example.com --password "$(cat ~/.nanobase-pass)"
    nanobase login ada@example.com
    nanobase app new todos
    nanobase app import todos.json
    nanobase app export todos --out todos.json
    nanobase validate todos.json
"#,
    );
    if cfg!(all(feature = "native", feature = "sqlite")) {
        text.push_str(
            r#"    nanobase serve --port 8090
    nanobase app list --url http://localhost:8090 --token "$TOKEN"
"#,
        );
    }
    text
}

/// Run the command line and answer with the exit code.
///
/// The two binaries are the same command line: the server build adds the
/// `serve` command and the local file, and the serverless build has
/// neither.
pub fn run(args: Vec<String>) -> u8 {
    let Some(command) = args.first().map(String::as_str) else {
        eprint!("{}", usage());
        return 2;
    };

    let rest = &args[1..];
    match command {
        "-h" | "--help" | "help" => {
            print!("{}", usage());
            0
        }
        "-V" | "--version" | "version" => {
            println!("nanobase {VERSION}");
            0
        }
        #[cfg(all(feature = "native", feature = "sqlite"))]
        "serve" => cmd_serve(rest),
        #[cfg(all(feature = "native", feature = "sqlite"))]
        "db" => cmd_db(rest),
        "init" => cmd_init(rest),
        "login" => cmd_login(rest),
        "me" => cmd_me(rest),
        "status" => cmd_status(rest),
        "app" => cmd_app(rest),
        "key" => cmd_key(rest),
        "new" => cmd_new(rest),
        "validate" => cmd_validate(rest),
        "redact" => cmd_redact(rest),
        other => {
            eprintln!("nanobase: unknown command {other:?}\n");
            eprint!("{}", usage());
            2
        }
    }
}

/// Run the HTTP API.
///
/// The platform serves the same operations the command line runs, so an
/// operator with `curl`, a script, and the `nanobase` binary all reach one
/// implementation. The listener binds every interface by default: this is a
/// server, and the address to reach it from is the operator's business.
#[cfg(all(feature = "native", feature = "sqlite"))]
fn cmd_serve(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &["host", "port", "data", "url", "cors", "rate-limit"],
        &["no-console"],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase serve: {e}");
        return 2;
    }
    let host = parsed
        .get("host")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_HOST").ok())
        .unwrap_or_else(|| "0.0.0.0".to_string());
    let port: u16 = match parsed.get("port").map(str::to_string) {
        Some(p) => match p.parse() {
            Ok(port) => port,
            Err(_) => {
                eprintln!("nanobase serve: --port needs a number");
                return 2;
            }
        },
        None => 8090,
    };
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let service = match Service::new("apps", catalog.store()) {
        Ok(service) => Arc::new(service.with_dbs(db_resolver(&parsed, &catalog))),
        Err(e) => {
            eprintln!("nanobase serve: {e}");
            return 1;
        }
    };
    // The keys of an older platform are moved into the apps they belong
    // to before anybody asks for one. The ones that cannot be moved are
    // said out loud, and wait for somebody to say which app they are
    // for.
    match service.ensure_migrated() {
        Ok(report) => {
            for (app, _id, name) in &report.moved {
                eprintln!("nanobase: the key {name:?} now belongs to {app}.");
            }
            for id in &report.unbound {
                eprintln!(
                    "nanobase: the key {id} was made for the whole platform. \
                     Give it an app with `nanobase key bind APP {id}`."
                );
            }
            for id in &report.orphaned {
                eprintln!("nanobase: the key {id} belongs to an app that is not here.");
            }
        }
        Err(e) => eprintln!("nanobase: the old API keys could not be moved: {e}"),
    }

    // The browser's rules: which origins may call this API, and how much
    // of it any one client may have.
    let mut options = crate::middleware::HttpOptions::default_closed();
    let origins = parsed
        .get("cors")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_CORS_ORIGINS").ok())
        .unwrap_or_default();
    if !origins.trim().is_empty() {
        options.cors = crate::middleware::CorsPolicy::from_origins(&origins);
    }
    let rate_limit = match parsed.get("rate-limit").map(str::to_string) {
        Some(text) => match text.trim().parse::<u32>() {
            Ok(0) => None,
            Ok(per_minute) => Some(per_minute),
            Err(_) => {
                eprintln!("nanobase serve: --rate-limit needs a number");
                return 2;
            }
        },
        None => match std::env::var("NANOBASE_RATE_LIMIT")
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok())
        {
            Some(0) => None,
            Some(per_minute) => Some(per_minute),
            None => None,
        },
    };
    if let Some(per_minute) = rate_limit {
        options = options.with_rate_limit(per_minute, std::time::Duration::from_secs(60));
    }

    let api = Arc::new(
        Api::with_options(Arc::clone(&service), options)
            .with_db_tester(db_tester(&parsed, &catalog)),
    );
    let addr = format!("{host}:{port}");
    let socket = match addr.parse::<std::net::SocketAddr>() {
        Ok(socket) => socket,
        Err(_) => {
            eprintln!("nanobase serve: {addr} is not an address");
            return 2;
        }
    };
    let console = !parsed.has("no-console");
    println!("nanobase {VERSION} serving on http://{addr}");
    println!("  api:  http://{addr}/api");
    if console {
        println!("  ui:   http://{addr}/");
    }
    if let Some(per_minute) = rate_limit {
        println!("  limit: {per_minute} requests a minute per client");
    }
    if !origins.trim().is_empty() {
        println!("  cors: {origins}");
    }
    let result = crate::http::serve(socket, 8, move |req| {
        // The console comes first: it is the one thing that is answered
        // without a token.
        if console {
            if let Some(response) = crate::console::respond(&req) {
                return response;
            }
        }
        api.handle(req)
    });
    if let Err(e) = result {
        eprintln!("nanobase serve: {e}");
        return 1;
    }
    0
}

/// A way to try a database configuration and see whether it answers.
///
/// The console asks this before saving a URL, so an operator finds out
/// that the URL is wrong while they are still looking at it.
#[cfg(all(feature = "native", feature = "sqlite"))]
fn db_tester(_parsed: &Parsed, catalog: &Catalog) -> crate::api::DbTester {
    let root = catalog.root().unwrap_or_else(|| PathBuf::from("."));
    Arc::new(
        move |app_key: &str, config: &crate::config::DatabaseConfig| {
            // In the app's own directory, because that is where the
            // database would live if the configuration were saved.
            let path = root.join("apps").join(app_key);
            let db = crate::db::factory::open(config, &path).map_err(|e| e.to_string())?;
            db.collections().map_err(|e| e.to_string())
        },
    )
}

/// The database service: the platform's rows over HTTP.
///
/// This is what the serverless build talks to. The server build keeps the
/// platform in a local file and serves it itself; the serverless build
/// has neither, so the platform lives in a database reached over HTTP —
/// this service, or a Cloudflare Worker in front of a D1 database.
#[cfg(all(feature = "native", feature = "sqlite"))]
fn cmd_db(rest: &[String]) -> u8 {
    let Some(sub) = rest.first().map(String::as_str) else {
        eprintln!("nanobase db: a subcommand is required\n");
        eprintln!("  serve [--host H] [--port P] [--db FILE]   serve the database over HTTP");
        return 2;
    };
    if sub != "serve" {
        eprintln!("nanobase db: unknown subcommand {sub:?}\n");
        eprintln!("  serve [--host H] [--port P] [--db FILE]   serve the database over HTTP");
        return 2;
    }
    let parsed = parse_args(&rest[1..], &["host", "port", "db"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase db serve: {e}");
        return 2;
    }
    let host = parsed
        .get("host")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_HOST").ok())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    let port: u16 = match parsed.get("port").map(str::to_string) {
        Some(p) => match p.parse() {
            Ok(port) => port,
            Err(_) => {
                eprintln!("nanobase db serve: --port needs a number");
                return 2;
            }
        },
        None => 8091,
    };
    let path = parsed
        .get("db")
        .map(PathBuf::from)
        .or_else(|| std::env::var("NANOBASE_DB").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("./data/service.db"));
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("nanobase db serve: cannot create {}: {e}", dir.display());
            return 1;
        }
    }
    let db = match crate::db::sqlite::SqliteDb::open(&path) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("nanobase db serve: {e}");
            return 1;
        }
    };
    let server = Arc::new(crate::db::DbServer::new(db));
    let addr = format!("{host}:{port}");
    let socket = match addr.parse::<std::net::SocketAddr>() {
        Ok(socket) => socket,
        Err(_) => {
            eprintln!("nanobase db serve: {addr} is not an address");
            return 2;
        }
    };
    println!("nanobase {VERSION} serving the database on http://{addr}");
    println!("  database: {}", path.display());
    let result = crate::http::serve(socket, 8, move |req| server.handle_http(&req));
    if let Err(e) = result {
        eprintln!("nanobase db serve: {e}");
        return 1;
    }
    0
}

// ------------------------------------------------------------------ args ---

/// Split flags from positional arguments.
///
/// `--flag value` and a bare `--flag` are different things, so both lists are
/// given: flags that take a value, and flags that stand alone. A caller that
/// forgets one gets a usage error instead of a silent misparse.
struct Parsed {
    values: Vec<(String, String)>,
    flags: Vec<String>,
    positional: Vec<String>,
    error: Option<String>,
}

impl Parsed {
    fn get(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }

    fn first(&self) -> Option<&str> {
        self.positional.first().map(String::as_str)
    }

    fn second(&self) -> Option<&str> {
        self.positional.get(1).map(String::as_str)
    }
}

fn parse_args(rest: &[String], valued: &[&str], flags: &[&str]) -> Parsed {
    let mut values = Vec::new();
    let mut seen_flags = Vec::new();
    let mut positional = Vec::new();
    let mut error = None;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if let Some(name) = arg.strip_prefix("--") {
            if valued.contains(&name) {
                if i + 1 >= rest.len() {
                    error = Some(format!("--{name} needs a value"));
                    break;
                }
                values.push((name.to_string(), rest[i + 1].clone()));
                i += 2;
            } else if flags.contains(&name) {
                seen_flags.push(name.to_string());
                i += 1;
            } else {
                error = Some(format!("unknown option --{name}"));
                break;
            }
        } else {
            positional.push(arg.to_string());
            i += 1;
        }
    }
    Parsed {
        values,
        flags: seen_flags,
        positional,
        error,
    }
}

/// Where the platform's state lives: `--data`, then `NANOBASE_DATA`, then
/// `./data/nanobase.db`.
#[cfg(feature = "sqlite")]
fn data_path(parsed: &Parsed) -> PathBuf {
    parsed
        .get("data")
        .map(PathBuf::from)
        .or_else(|| std::env::var("NANOBASE_DATA").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("./data/nanobase.db"))
}

/// Where the platform's own state lives.
///
/// This is the owner's choice, and it is chosen at run time rather than
/// at build time:
///
/// * a SQLite file beside the binary's data directory — the default,
///   and the only choice in a build with no network client;
/// * a database reached over the network — a serverless PostgreSQL, a
///   serverless MySQL, a Turso, a D1, or a service that speaks this
///   platform's app-database protocol.
///
/// The platform's state and an app's rows are separate decisions. An
/// owner may keep the catalog in a local file while the apps live in
/// Neon, or keep the catalog in Neon while the apps live wherever they
/// like. Nothing here assumes the two are the same kind of thing.
enum Catalog {
    /// A local SQLite file.
    #[cfg(feature = "sqlite")]
    File(SqliteStore),
    /// A database reached over the network.
    #[cfg(feature = "curl")]
    Serverless(ServerlessStore),
}

impl Catalog {
    /// The catalog the command is pointed at.
    ///
    /// `--db-url` — or `NANOBASE_DB_URL` — says the platform's state
    /// lives in a database reached over the network, and it wins over the
    /// local file. With no such URL, the local file is used, and a build
    /// without one says so.
    fn open(parsed: &Parsed) -> Result<Catalog, u8> {
        let url = parsed
            .get("db-url")
            .map(str::to_string)
            .or_else(|| std::env::var("NANOBASE_DB_URL").ok());
        if let Some(url) = url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
            return Catalog::open_remote(url, parsed);
        }
        #[cfg(feature = "sqlite")]
        {
            Ok(Catalog::File(SqliteStore::new(data_path(parsed))))
        }
        #[cfg(not(feature = "sqlite"))]
        {
            eprintln!("nanobase: this build has no local file; name the database with\n");
            eprintln!("  --db-url URL, or NANOBASE_DB_URL in the environment.");
            Err(2)
        }
    }

    /// The catalog in a database reached over the network.
    ///
    /// The URL says which one it is. A `postgres://` or `mysql://` URL
    /// names the database itself, and the platform speaks its protocol
    /// — so the catalog can live in a serverless PostgreSQL or a
    /// serverless MySQL with no service in front of it. An `http://` URL
    /// names a service that holds a database and speaks this platform's
    /// own protocol.
    #[cfg(feature = "curl")]
    fn open_remote(url: &str, parsed: &Parsed) -> Result<Catalog, u8> {
        let token = parsed
            .get("db-token")
            .map(str::to_string)
            .or_else(|| std::env::var("NANOBASE_DB_TOKEN").ok());
        let db: Arc<dyn crate::db::AppDb> = if crate::db::provider::is_provider_url(url) {
            let client: Arc<dyn crate::httpclient::HttpClient> =
                Arc::new(crate::httpclient::CurlHttpClient);
            Arc::new(
                crate::db::provider::ProviderDb::new(url, client).map_err(|e| {
                    eprintln!("nanobase: {e}");
                    1
                })?,
            )
        } else {
            let mut db =
                crate::db::http::HttpDb::new(url, Arc::new(crate::httpclient::CurlHttpClient));
            if let Some(token) = token.clone() {
                db = db.with_token(token);
            }
            Arc::new(db)
        };
        Ok(Catalog::Serverless(ServerlessStore::new(db, url)))
    }

    /// The store the service and the commands read through.
    fn store(&self) -> Arc<dyn PlatformStore> {
        match self {
            #[cfg(feature = "sqlite")]
            Catalog::File(store) => Arc::new(store.clone()),
            #[cfg(feature = "curl")]
            Catalog::Serverless(store) => Arc::new(store.clone()),
        }
    }

    /// The platform's state.
    fn load(&self) -> Result<Platform, u8> {
        match self {
            #[cfg(feature = "sqlite")]
            Catalog::File(store) => load_platform(store),
            #[cfg(feature = "curl")]
            Catalog::Serverless(store) => block_on(store.load()).map_err(|e| {
                eprintln!("nanobase: {e}");
                1
            }),
        }
    }

    /// Write the platform's state back.
    fn save(&self, platform: &Platform) -> Result<(), u8> {
        let store = self.store();
        block_on(store.save(platform)).map_err(|e| {
            eprintln!("nanobase: {e}");
            1
        })
    }

    /// Where the state lives, for the command line to print.
    fn describe(&self) -> String {
        match self {
            #[cfg(feature = "sqlite")]
            Catalog::File(store) => store.path().display().to_string(),
            #[cfg(feature = "curl")]
            Catalog::Serverless(store) => store.url().to_string(),
        }
    }

    /// The directory the apps' own data hangs off, when there is one.
    fn root(&self) -> Option<PathBuf> {
        match self {
            #[cfg(feature = "sqlite")]
            Catalog::File(store) => Some(store.root()),
            #[cfg(feature = "curl")]
            Catalog::Serverless(_) => None,
        }
    }
}

/// The platform's state, from a local file.
///
/// An install that predates SQLite kept its state in a JSON file beside
/// where the database now lives. Bring it across once, before the first
/// read; the file is renamed so this cannot run twice.
#[cfg(feature = "sqlite")]
fn load_platform(store: &SqliteStore) -> Result<Platform, u8> {
    if let Err(e) = store.migrate_from_json() {
        eprintln!("nanobase: {e}");
        return Err(1);
    }
    block_on(store.load()).map_err(|e| {
        eprintln!("nanobase: {e}");
        1
    })
}

// -------------------------------------------------------------- platform ---

fn cmd_init(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &["email", "password", "data", "db-url", "db-token"],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase init: {e}");
        return 2;
    }
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let mut platform = match catalog.load() {
        Ok(p) => p,
        Err(code) => return code,
    };
    if !platform.needs_setup() {
        eprintln!(
            "nanobase: this platform already has an owner ({}).",
            platform
                .owner()
                .map(|u| u.email.as_str())
                .unwrap_or("unknown")
        );
        eprintln!("         run `nanobase login` to get a session token.");
        return 1;
    }

    let email = match parsed.get("email").map(str::to_string) {
        Some(e) => e,
        None => match prompt("Email: ") {
            Some(e) => e,
            None => {
                eprintln!("nanobase init: no email given and nothing to read");
                return 2;
            }
        },
    };

    let password = match parsed
        .get("password")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_PASSWORD").ok())
    {
        Some(p) => p,
        None => {
            // There is no portable way to stop a terminal echoing in `std`,
            // so say so rather than let someone type a password believing it
            // is hidden.
            eprintln!("(the password is shown as you type it; use --password or");
            eprintln!(" NANOBASE_PASSWORD to avoid that)");
            match prompt("Password: ") {
                Some(p) => p,
                None => {
                    eprintln!("nanobase init: no password given and nothing to read");
                    return 2;
                }
            }
        }
    };

    if let Err(e) = check_email(&email) {
        eprintln!("nanobase init: {e}");
        return 1;
    }

    platform.ensure_jwt_secret();
    let user = match create_owner(&mut platform, &email, &password, now_secs()) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("nanobase init: {e}");
            return 1;
        }
    };
    if let Err(e) = catalog.save(&platform) {
        eprintln!("nanobase: {e}");
        return 1;
    }

    println!("Owner created: {}", user.email);
    println!("Platform database: {}", catalog.describe());
    println!("Next: nanobase login {}", user.email);
    0
}

fn cmd_login(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["password", "data", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase login: {e}");
        return 2;
    }
    let Some(email) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase login: an email is required\n");
        eprintln!("example: nanobase login ada@example.com");
        return 2;
    };
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let platform = match catalog.load() {
        Ok(p) => p,
        Err(code) => return code,
    };
    if platform.needs_setup() {
        eprintln!("nanobase: this platform has no owner yet; run `nanobase init` first.");
        return 1;
    }

    let password = match parsed
        .get("password")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_PASSWORD").ok())
    {
        Some(p) => p,
        None => match prompt("Password: ") {
            Some(p) => p,
            None => {
                eprintln!("nanobase login: no password given and nothing to read");
                return 2;
            }
        },
    };

    let user = match verify_login(&platform, &email, &password) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    println!(
        "{}",
        issue_token(
            user,
            platform.jwt_key(),
            now_secs(),
            crate::SESSION_TTL_SECS
        )
    );
    0
}

fn cmd_me(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["token", "data", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase me: {e}");
        return 2;
    }
    let Some(token) = parsed
        .get("token")
        .map(str::to_string)
        .or_else(|| std::env::var("NANOBASE_TOKEN").ok())
    else {
        eprintln!("nanobase me: a token is required (--token or NANOBASE_TOKEN)");
        return 2;
    };
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let platform = match catalog.load() {
        Ok(p) => p,
        Err(code) => return code,
    };
    match crate::jwt::decode(&token, platform.jwt_key(), now_secs()) {
        Ok(claims) => {
            println!("user:  {} ({})", claims.email, claims.role);
            println!("id:    {}", claims.sub);
            println!("expires: {}", crate::iso8601(claims.exp));
            0
        }
        Err(e) => {
            eprintln!("nanobase: {e}");
            1
        }
    }
}

fn cmd_status(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase status: {e}");
        return 2;
    }
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let platform = match catalog.load() {
        Ok(p) => p,
        Err(code) => return code,
    };
    println!("platform database: {}", catalog.describe());
    if platform.needs_setup() {
        println!("owner:         none — run `nanobase init`");
    } else {
        println!(
            "owner:         {}",
            platform
                .owner()
                .map(|u| u.email.as_str())
                .unwrap_or("unknown")
        );
    }
    println!("apps:          {}", platform.apps.len());
    0
}

/// Read one line from stdin, trimmed. `None` when there is nothing to read —
/// a closed pipe, or no terminal at all.
fn prompt(label: &str) -> Option<String> {
    use std::io::{BufRead, Write};
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(label.as_bytes()).ok()?;
    stdout.flush().ok()?;
    let mut line = String::new();
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let read = handle.read_line(&mut line).ok()?;
    if read == 0 {
        return None;
    }
    let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// ----------------------------------------------------------- app documents ---

/// The API keys: `nanobase key list|create|show|remove|bind`.
///
/// A key is a credential a program holds. It belongs to one app: it is
/// made for that app, it may only see that app, and it lives in that
/// app's own database. The platform keeps a directory of which keys
/// exist and which app they belong to, and nothing that could be used
/// as a key.
fn cmd_key(rest: &[String]) -> u8 {
    let Some(sub) = rest.first().map(String::as_str) else {
        eprintln!("nanobase key: what should it do?\n");
        eprintln!("  nanobase key list APP");
        eprintln!("  nanobase key create APP --name NAME [--scopes a,b] [--expires-in 30d]");
        eprintln!("  nanobase key show APP ID");
        eprintln!("  nanobase key remove APP ID");
        eprintln!("  nanobase key bind APP ID");
        return 2;
    };
    let parsed = parse_args(
        &rest[1..],
        &[
            "name",
            "scopes",
            "expires-in",
            "expires-at",
            "data",
            "url",
            "token",
            "db-url",
            "db-token",
        ],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase key: {e}");
        return 2;
    }
    let catalog = match Catalog::open(&parsed) {
        Ok(catalog) => catalog,
        Err(code) => return code,
    };
    let platform = match catalog.load() {
        Ok(p) => p,
        Err(code) => return code,
    };
    if platform.needs_setup() {
        eprintln!("nanobase: this platform has no owner yet; run `nanobase init` first.");
        return 1;
    }

    match sub {
        "list" => {
            let Some(app) = parsed.first().map(str::to_string) else {
                eprintln!("nanobase key list: an app is required");
                return 2;
            };
            if let Some(client) = api_client(&parsed) {
                let value = match client.request("GET", &format!("/api/apps/{app}/keys"), None) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("nanobase: {e}");
                        return 1;
                    }
                };
                let code = print_keys(&value, &platform);
                report_waiting_keys(&platform);
                return code;
            }
            let service = match service_or_exit(&parsed) {
                Some(service) => service,
                None => return 1,
            };
            let keys = match service.list_keys(&app) {
                Ok(keys) => keys,
                Err(e) => {
                    eprintln!("nanobase: {e}");
                    return 1;
                }
            };
            let code = print_key_rows(&keys, &platform);
            report_waiting_keys(&platform);
            code
        }
        "create" => {
            let Some(app) = parsed.first().map(str::to_string) else {
                eprintln!("nanobase key create: an app is required");
                return 2;
            };
            let Some(name) = parsed.get("name").map(str::to_string) else {
                eprintln!("nanobase key create: --name is required");
                return 2;
            };
            let scopes = match parsed.get("scopes") {
                Some(list) => list
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<String>>(),
                None => vec![crate::apikey::SCOPE_ROWS_READ.to_string()],
            };
            if let Some(unknown) = crate::apikey::first_unknown_scope(&scopes) {
                eprintln!(
                    "nanobase key create: there is no scope called {unknown}\n\
                     the scopes are: {}",
                    crate::apikey::SCOPES.join(", ")
                );
                return 2;
            }
            let expires_at = match expiry_of(&parsed) {
                Ok(at) => at,
                Err(message) => {
                    eprintln!("nanobase key create: {message}");
                    return 2;
                }
            };
            if let Some(client) = api_client(&parsed) {
                let mut body = mosql::Object::new();
                body.set("name", Value::str(name.clone()));
                body.set(
                    "scopes",
                    Value::Array(scopes.iter().map(|s| Value::str(s.clone())).collect()),
                );
                if let Some(at) = expires_at {
                    body.set("expiresAt", Value::Int(at));
                }
                let value = match client.request(
                    "POST",
                    &format!("/api/apps/{app}/keys"),
                    Some(&Value::Object(body)),
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("nanobase: {e}");
                        return 1;
                    }
                };
                let secret = value
                    .get("secret")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string();
                return print_secret(&secret);
            }
            let service = match service_or_exit(&parsed) {
                Some(service) => service,
                None => return 1,
            };
            let (_, secret) = match service.create_key(&app, name, scopes, expires_at) {
                Ok(pair) => pair,
                Err(e) => {
                    eprintln!("nanobase: {e}");
                    return 1;
                }
            };
            print_secret(&secret)
        }
        "show" => {
            let (Some(app), Some(id)) = (
                parsed.first().map(str::to_string),
                parsed.second().map(str::to_string),
            ) else {
                eprintln!("nanobase key show: an app and a key id are required");
                return 2;
            };
            if let Some(client) = api_client(&parsed) {
                let value = match client.request("GET", &format!("/api/apps/{app}/keys/{id}"), None)
                {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("nanobase: {e}");
                        return 1;
                    }
                };
                return print_one_key(&value, &platform);
            }
            let service = match service_or_exit(&parsed) {
                Some(service) => service,
                None => return 1,
            };
            let key = match service.find_key(&app, &id) {
                Ok(key) => key,
                Err(e) => {
                    eprintln!("nanobase: {e}");
                    return 1;
                }
            };
            print_one_key(&crate::keys::to_public_row(&key), &platform)
        }
        "remove" => {
            let (Some(app), Some(id)) = (
                parsed.first().map(str::to_string),
                parsed.second().map(str::to_string),
            ) else {
                eprintln!("nanobase key remove: an app and a key id are required");
                return 2;
            };
            if let Some(client) = api_client(&parsed) {
                let value =
                    match client.request("DELETE", &format!("/api/apps/{app}/keys/{id}"), None) {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("nanobase: {e}");
                            return 1;
                        }
                    };
                println!(
                    "removed {}",
                    value.get("name").and_then(|v| v.as_str()).unwrap_or("")
                );
                return 0;
            }
            let service = match service_or_exit(&parsed) {
                Some(service) => service,
                None => return 1,
            };
            let removed = match service.remove_key(&app, &id) {
                Ok(removed) => removed,
                Err(e) => {
                    eprintln!("nanobase: {e}");
                    return 1;
                }
            };
            println!(
                "removed {}",
                removed.get("name").and_then(|v| v.as_str()).unwrap_or("")
            );
            0
        }
        "bind" => {
            // A key from an older platform, which was made for the whole
            // platform rather than for an app. It is given an app here,
            // and from then on it is that app's key.
            let (Some(app), Some(id)) = (
                parsed.first().map(str::to_string),
                parsed.second().map(str::to_string),
            ) else {
                eprintln!("nanobase key bind: an app and a key id are required");
                return 2;
            };
            let service = match service_or_exit(&parsed) {
                Some(service) => service,
                None => return 1,
            };
            match service.bind_legacy_key(&app, &id) {
                Ok(key) => {
                    println!(
                        "bound {} to {}",
                        key.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("the key"),
                        app
                    );
                    0
                }
                Err(e) => {
                    eprintln!("nanobase: {e}");
                    1
                }
            }
        }
        other => {
            eprintln!("nanobase key: unknown subcommand {other:?}\n");
            eprintln!("  nanobase key list APP");
            eprintln!("  nanobase key create APP --name NAME [--scopes a,b] [--expires-in 30d]");
            eprintln!("  nanobase key show APP ID");
            eprintln!("  nanobase key remove APP ID");
            eprintln!("  nanobase key bind APP ID");
            2
        }
    }
}

/// The keys of an older platform that have not been moved yet.
///
/// They are said out loud, because a key that has not been moved into
/// an app cannot be used: it waits for somebody to say which app it is
/// for.
fn report_waiting_keys(platform: &crate::platform::Platform) -> u8 {
    if platform.legacy_keys.is_empty() {
        return 0;
    }
    eprintln!();
    eprintln!("These keys are from an older platform and have no app yet:");
    for key in &platform.legacy_keys {
        let app = key.app.clone().unwrap_or_else(|| "none".to_string());
        eprintln!("  {}  {}  (app: {})", key.id, key.name, app);
    }
    eprintln!();
    eprintln!("Bind one to an app with `nanobase key bind APP ID`.");
    0
}

/// The keys, as a table.
fn print_key_rows(keys: &[Value], platform: &crate::platform::Platform) -> u8 {
    if keys.is_empty() {
        println!("no API keys yet");
        return 0;
    }
    println!(
        "{:<22} {:<20} {:<14} {:<20} EXPIRES",
        "ID", "NAME", "SCOPES", "APP"
    );
    for key in keys {
        let text = |name: &str| {
            key.get(name)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };
        // The app is named rather than numbered: a person reading a
        // list wants to know which app it is for.
        let app = platform
            .find_app(&text("app"))
            .map(|record| record.doc.app.name.clone())
            .unwrap_or_else(|| text("app"));
        let scopes = match key.get("scopes").and_then(|v| v.as_array()) {
            Some(list) if !list.is_empty() => list
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<&str>>()
                .join(","),
            _ => "-".to_string(),
        };
        println!(
            "{:<22} {:<20} {:<14} {:<20} {}",
            text("id"),
            text("name"),
            scopes,
            app,
            match key.get("expiresAt").and_then(|v| v.as_str()) {
                Some(at) if !at.is_empty() => at.to_string(),
                _ => "never".to_string(),
            }
        );
    }
    0
}

/// The keys, as the API sends them.
fn print_keys(value: &Value, platform: &crate::platform::Platform) -> u8 {
    let keys = value
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    print_key_rows(&keys, platform)
}

/// One key, in words.
fn print_one_key(key: &Value, platform: &crate::platform::Platform) -> u8 {
    let text = |name: &str| {
        key.get(name)
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string()
    };
    println!("id:        {}", text("id"));
    println!("name:      {}", text("name"));
    println!("preview:   {}", text("preview"));
    let scopes = match key.get("scopes").and_then(|v| v.as_array()) {
        Some(list) if !list.is_empty() => list
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<&str>>()
            .join(", "),
        _ => "-".to_string(),
    };
    println!("scopes:    {scopes}");
    let app = platform
        .find_app(&text("app"))
        .map(|record| record.doc.app.name.clone())
        .unwrap_or_else(|| text("app"));
    println!("app:       {app}");
    println!("created:   {}", text("createdAt"));
    println!("last used: {}", text("lastUsedAt"));
    0
}

/// The key itself, and the warning that comes with it.
fn print_secret(secret: &str) -> u8 {
    println!("{secret}");
    eprintln!();
    eprintln!("This is the only time the key is shown. Put it in your secret store.");
    0
}

/// When a key stops working, from `--expires-in 30d` or `--expires-at`.
fn expiry_of(parsed: &Parsed) -> Result<Option<i64>, String> {
    if let Some(at) = parsed.get("expires-at") {
        return match at.trim().parse::<i64>() {
            Ok(seconds) => Ok(Some(seconds)),
            Err(_) => Err("--expires-at must be a number of seconds since the epoch".to_string()),
        };
    }
    let Some(text) = parsed.get("expires-in") else {
        return Ok(None);
    };
    let text = text.trim().to_ascii_lowercase();
    let (number, unit) = match text.chars().last() {
        Some(last) if !last.is_ascii_digit() => (&text[..text.len() - 1], last.to_string()),
        _ => (text.as_str(), "d".to_string()),
    };
    let amount: i64 = number
        .trim()
        .parse()
        .map_err(|_| "--expires-in needs a number, like 30d".to_string())?;
    let seconds = match unit.as_str() {
        "s" => amount,
        "m" => amount * 60,
        "h" => amount * 3_600,
        "d" => amount * 86_400,
        "w" => amount * 604_800,
        other => return Err(format!("--expires-in: unknown unit {other:?}")),
    };
    Ok(Some(now_secs() + seconds))
}

fn cmd_new(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["out"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase new: {e}");
        return 2;
    }
    let Some(name) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase new: an app name is required\n");
        eprintln!("example: nanobase new todos --out todos.json");
        return 2;
    };
    let doc = AppDoc::empty(name);
    if let Err(e) = doc.validate() {
        return report_config(&e);
    }
    write_out(parsed.get("out"), &doc.to_json_pretty())
}

fn cmd_validate(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["out"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase validate: {e}");
        return 2;
    }
    let Some(path) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase validate: a file path is required");
        return 2;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nanobase: cannot read {path}: {e}");
            return 1;
        }
    };
    let secrets = Secrets::from_env();
    let doc = match AppDoc::load(&text, &secrets) {
        Ok(d) => d,
        Err(e) => return report_config(&e),
    };

    let undeclared = doc.undeclared_data_collections();
    let incomplete = doc.incomplete_providers();
    println!("{path}: ok");
    println!(
        "  app       {} ({})",
        doc.app.name,
        doc.app.app_type.as_str()
    );
    println!("  database  {}", doc.config.database.raw_type);
    println!("  storage   {}", doc.config.storage.kind.as_str());
    println!(
        "  auth      {}",
        if doc.config.auth.is_enabled() {
            "enabled"
        } else {
            "off"
        }
    );
    println!("  schema    {} collection(s)", doc.schema.len());
    let rows: usize = doc.data.values().map(Vec::len).sum();
    println!("  data      {rows} row(s)");
    if !undeclared.is_empty() {
        println!(
            "  note      data for undeclared collection(s): {}",
            undeclared.join(", ")
        );
    }
    if !incomplete.is_empty() {
        println!(
            "  note      provider(s) with no credentials: {} \
                     (set SECRET_<PROVIDER>_CLIENT_ID / _CLIENT_SECRET)",
            incomplete.join(", ")
        );
    }
    if let Some(path) = parsed.get("out") {
        return write_out(Some(path), &doc.to_json_pretty());
    }
    0
}

fn cmd_redact(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["out"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase redact: {e}");
        return 2;
    }
    let Some(path) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase redact: a file path is required");
        return 2;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nanobase: cannot read {path}: {e}");
            return 1;
        }
    };
    match AppDoc::parse(&text) {
        Ok(doc) => write_out(parsed.get("out"), &doc.to_json_pretty_redacted()),
        Err(e) => report_config(&e),
    }
}

fn report_config(err: &ConfigError) -> u8 {
    eprintln!("nanobase: {} problem(s)", err.issues.len());
    for issue in &err.issues {
        eprintln!("  {issue}");
    }
    1
}

fn write_out(path: Option<&str>, body: &str) -> u8 {
    match path {
        None => {
            print!("{body}");
            0
        }
        Some(p) => match std::fs::write(p, body) {
            Ok(()) => {
                eprintln!("nanobase: wrote {p}");
                0
            }
            Err(e) => {
                eprintln!("nanobase: cannot write {p}: {e}");
                1
            }
        },
    }
}

// ------------------------------------------------------------------- apps ---

fn cmd_app(rest: &[String]) -> u8 {
    let Some(sub) = rest.first().map(String::as_str) else {
        eprintln!("nanobase app: a subcommand is required\n");
        eprintln!("  new <name> | import <file> | list | show <id|name> | export <id|name>");
        eprintln!("  rename <id|name> <new-name> | clone <id|name> <new-name>");
        eprintln!("  remove <id|name> --confirm <id>");
        return 2;
    };
    let args = &rest[1..];
    match sub {
        "new" => app_new(args),
        "import" => app_import(args),
        "list" => app_list(args),
        "show" => app_show(args),
        "export" => app_export(args),
        "rename" => app_rename(args),
        "clone" => app_clone(args),
        "remove" | "rm" | "delete" => app_remove(args),
        "rows" => app_rows(args),
        "insert" => app_insert(args),
        "update" => app_update(args),
        "delete-rows" => app_delete_rows(args),
        other => {
            eprintln!("nanobase app: unknown subcommand {other:?}\n");
            eprintln!("  new | import | list | show | export | rename | clone | remove");
            eprintln!("  rows | insert | update | delete-rows");
            2
        }
    }
}

/// Load the platform, or report why that is impossible.
/// The app service, over the same store the command line uses.
///
/// Every app command goes through it, so the command line and the API are
/// one code path: an event fires for a change made either way.
fn service_or_exit(parsed: &Parsed) -> Option<Service> {
    let catalog = match Catalog::open(parsed) {
        Ok(catalog) => catalog,
        Err(code) => {
            let _ = code;
            return None;
        }
    };
    match Service::new("apps", catalog.store()) {
        Ok(svc) => Some(svc.with_dbs(db_resolver(parsed, &catalog))),
        Err(e) => {
            eprintln!("nanobase: {e}");
            None
        }
    }
}

/// What opened an app's database: the engine, where it is, and the token
/// that opens it.
#[derive(PartialEq)]
struct DbSettings {
    kind: DbKind,
    url: Option<String>,
    token: Option<String>,
}

/// The open database handles, one per app: the settings that opened it
/// and the handle itself.
type OpenApps = Mutex<BTreeMap<String, (DbSettings, Arc<dyn AppDb>)>>;

/// How an app's rows are reached.
///
/// The app's document says which engine they are in; this opens it. A
/// local app's rows live in `apps/<id>/app.db` beside the platform's own
/// database.
///
/// The handle is kept, so a request does not open the database again for
/// every row it reads. A change to the app's database settings replaces
/// it, because the old handle points at the old engine.
fn db_resolver(_parsed: &Parsed, catalog: &Catalog) -> crate::service::DbResolver {
    let root = catalog.root().unwrap_or_else(|| PathBuf::from("."));
    let open: OpenApps = Mutex::new(BTreeMap::new());
    Arc::new(move |record: &crate::platform::AppRecord| {
        let database = &record.doc.config.database;
        let settings = DbSettings {
            kind: database.kind,
            url: database.url.clone(),
            token: database.token.clone(),
        };
        let mut open = open.lock().expect("not poisoned");
        if let Some((known, db)) = open.get(&record.id) {
            if known == &settings {
                return Ok(db.clone());
            }
        }
        let path = root.join("apps").join(&record.id);
        let db = crate::db::factory::open(&record.doc.config.database, &path)?;
        open.insert(record.id.clone(), (settings, db.clone()));
        Ok(db)
    })
}

/// The API client, when the command is pointed at a running platform.
///
/// `--url` (or `NANOBASE_URL`) makes the command a thin client: it asks the
/// platform over HTTP instead of opening the database, so a script and the
/// platform's own API reach exactly the same code.
fn api_client(parsed: &Parsed) -> Option<ApiClient> {
    ApiClient::from_options(parsed.get("url"), parsed.get("token"))
}

/// One app, as the commands print it.
///
/// The same shape whether it came from the local database or from the API,
/// so a command prints the same thing either way.
struct AppView {
    id: String,
    name: String,
    app_type: String,
    database: String,
    storage: String,
    auth: bool,
    acl: bool,
    created_at: String,
    schema: Vec<(String, usize)>,
    seed_rows: usize,
    doc: AppDoc,
}

impl AppView {
    /// A view of an app in the local database.
    fn of(record: &crate::platform::AppRecord) -> AppView {
        let doc = record.doc.clone();
        let schema = doc
            .schema
            .iter()
            .map(|(name, def)| (name.clone(), def.columns.len()))
            .collect();
        let seed_rows = doc.data.values().map(Vec::len).sum();
        AppView {
            id: record.id.clone(),
            name: doc.app.name.clone(),
            app_type: doc.app.app_type.as_str().to_string(),
            database: doc.config.database.raw_type.clone(),
            storage: doc.config.storage.kind.as_str().to_string(),
            auth: doc.config.auth.is_enabled(),
            acl: doc.config.acl.enabled,
            created_at: record.created_at.clone(),
            schema,
            seed_rows,
            doc,
        }
    }

    /// A view of an app as the API returns it.
    fn from_value(value: &Value) -> Option<AppView> {
        let id = value.get("id")?.as_str()?.to_string();
        let created_at = value
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let doc = AppDoc::from_value(value.get("doc")?).ok()?;
        let schema = doc
            .schema
            .iter()
            .map(|(name, def)| (name.clone(), def.columns.len()))
            .collect();
        let seed_rows = doc.data.values().map(Vec::len).sum();
        Some(AppView {
            id,
            name: doc.app.name.clone(),
            app_type: doc.app.app_type.as_str().to_string(),
            database: doc.config.database.raw_type.clone(),
            storage: doc.config.storage.kind.as_str().to_string(),
            auth: doc.config.auth.is_enabled(),
            acl: doc.config.acl.enabled,
            created_at,
            schema,
            seed_rows,
            doc,
        })
    }
}

/// Print the app's details, the way `app show` does.
fn print_app(view: &AppView, root: Option<&std::path::Path>) {
    println!("name:       {}", view.name);
    println!("id:         {}", view.id);
    println!("type:       {}", view.app_type);
    println!("created:    {}", view.created_at);
    println!("database:   {}", view.database);
    println!("storage:    {}", view.storage);
    println!("auth:       {}", if view.auth { "enabled" } else { "off" });
    println!("acl:        {}", if view.acl { "enabled" } else { "off" });
    if let Some(root) = root {
        println!("data dir:   {}", root.join(&view.id).display());
    }
    println!("schema:     {} collection(s)", view.schema.len());
    for (name, columns) in &view.schema {
        println!("  {name}: {columns} column(s)");
    }
    println!("seed rows:  {}", view.seed_rows);
}

fn platform_or_exit(parsed: &Parsed) -> Option<Platform> {
    match Catalog::open(parsed) {
        Ok(catalog) => match catalog.load() {
            Ok(p) => Some(p),
            Err(code) => {
                let _ = code;
                None
            }
        },
        Err(code) => {
            let _ = code;
            None
        }
    }
}

fn save_or_exit(parsed: &Parsed, platform: &Platform) -> Option<()> {
    match Catalog::open(parsed) {
        Ok(catalog) => match catalog.save(platform) {
            Ok(()) => Some(()),
            Err(code) => {
                let _ = code;
                None
            }
        },
        Err(code) => {
            let _ = code;
            None
        }
    }
}

fn app_new(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app new: {e}");
        return 2;
    }
    let Some(name) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app new: an app name is required\n");
        eprintln!("example: nanobase app new todos");
        return 2;
    };
    if let Some(problem) = crate::config::check_app_name(&name) {
        eprintln!("nanobase app new: app name {problem}");
        return 1;
    }
    if let Some(client) = api_client(&parsed) {
        // The platform makes the app; the command only asks and prints.
        let mut body = mosql::Object::new();
        body.set("name", Value::str(name.clone()));
        let value = match client.request("POST", "/api/apps", Some(&Value::Object(body))) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let Some(view) = AppView::from_value(&value) else {
            eprintln!("nanobase: the platform sent an app this command cannot read");
            return 1;
        };
        println!("Created {}", view.name);
        println!("  id:        {}", view.id);
        println!("  created:   {}", view.created_at);
        println!("  database:  {}", view.database);
        return 0;
    }
    let service = match service_or_exit(&parsed) {
        Some(svc) => svc,
        None => return 1,
    };
    if service.platform().map(|p| p.needs_setup()).unwrap_or(false) {
        eprintln!("nanobase: this platform has no owner yet; run `nanobase init` first.");
        return 1;
    }
    let doc = AppDoc::empty(name);
    let record = match service.create(doc) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    let id = record
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let name = record
        .get("doc")
        .and_then(|d| d.get("app"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let created = record
        .get("createdAt")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    println!("Created {name}");
    println!("  id:        {id}");
    println!("  created:   {created}");
    let root = Catalog::open(&parsed)
        .ok()
        .and_then(|catalog| catalog.root());
    if let Some(root) = root {
        println!("  data:      {}", root.join("apps").join(&id).display());
    }
    println!("  database:  sqlite (default — edit the document to change it)");
    0
}

fn app_import(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app import: {e}");
        return 2;
    }
    let Some(path) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app import: a file path is required\n");
        eprintln!("example: nanobase app import todos.json");
        return 2;
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nanobase: cannot read {path}: {e}");
            return 1;
        }
    };
    let secrets = Secrets::from_env();
    let doc = match AppDoc::load(&text, &secrets) {
        Ok(d) => d,
        Err(e) => return report_config(&e),
    };
    if let Some(client) = api_client(&parsed) {
        // The platform holds the apps; the document goes to it whole.
        let value = match client.request("POST", "/api/apps", Some(&doc.to_value())) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        println!("Imported {}", doc.app.name);
        println!("  id: {id}");
        return 0;
    }
    let mut platform = match platform_or_exit(&parsed) {
        Some(p) => p,
        None => return 1,
    };
    if platform.needs_setup() {
        eprintln!("nanobase: this platform has no owner yet; run `nanobase init` first.");
        return 1;
    }
    // Importing over an app of the same name replaces it: that is what a user
    // with an edited export means, and creating a second app called the same
    // thing would be the surprise.
    if let Some(existing) = platform.app_by_name(&doc.app.name) {
        let id = existing.id.clone();
        let name = doc.app.name.clone();
        if let Err(e) = platform.replace_app(&id, doc) {
            eprintln!("nanobase: {e}");
            return 1;
        }
        if save_or_exit(&parsed, &platform).is_none() {
            return 1;
        }
        println!("Updated {name} ({id})");
        return 0;
    }
    let (name, id) = match platform.create_app(doc, now_secs()) {
        Ok(app) => (app.doc.app.name.clone(), app.id.clone()),
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    if save_or_exit(&parsed, &platform).is_none() {
        return 1;
    }
    println!("Imported {name}");
    println!("  id: {id}");
    0
}

/// The rows of an app's collection.
///
/// The same operation whether the platform is reached through its API or
/// opened directly, so a script and the platform agree.
fn app_rows(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &[
            "data", "url", "token", "filter", "limit", "offset", "db-url", "db-token",
        ],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app rows: {e}");
        return 2;
    }
    let (Some(app), Some(collection)) = (parsed.first(), parsed.second()) else {
        eprintln!("nanobase app rows: an app and a collection are required");
        return 2;
    };
    let filter = match filter_of(&parsed) {
        Ok(filter) => filter,
        Err(e) => {
            eprintln!("nanobase app rows: {e}");
            return 2;
        }
    };
    let rows = if let Some(client) = api_client(&parsed) {
        let mut path = format!("/api/apps/{app}/db/{collection}");
        let query = query_string(&parsed, &filter);
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        match client.request("GET", &path, None) {
            Ok(value) => match value.get("items").and_then(|v| v.as_array()) {
                Some(items) => items.clone(),
                None => Vec::new(),
            },
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    } else {
        let Some(service) = service_or_exit(&parsed) else {
            return 1;
        };
        match service.find_rows(
            app,
            collection,
            &filter,
            limit_of(&parsed),
            offset_of(&parsed),
        ) {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    };
    if rows.is_empty() {
        println!("no rows");
        return 0;
    }
    for row in &rows {
        println!("{}", mosql::to_json(row));
    }
    0
}

/// Put a row in an app's collection.
fn app_insert(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &["data", "url", "token", "json", "db-url", "db-token"],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app insert: {e}");
        return 2;
    }
    let (Some(app), Some(collection)) = (parsed.first(), parsed.second()) else {
        eprintln!("nanobase app insert: an app and a collection are required");
        return 2;
    };
    let Some(text) = parsed.get("json") else {
        eprintln!("nanobase app insert: --json is required");
        return 2;
    };
    let row = match mosql::parse_json(text) {
        Ok(row) => row,
        Err(e) => {
            eprintln!("nanobase app insert: --json is not JSON: {e}");
            return 2;
        }
    };
    let inserted = if let Some(client) = api_client(&parsed) {
        let mut body = mosql::Object::new();
        body.set("row", row);
        match client.request(
            "POST",
            &format!("/api/apps/{app}/db/{collection}"),
            Some(&Value::Object(body)),
        ) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    } else {
        let Some(service) = service_or_exit(&parsed) else {
            return 1;
        };
        match service.insert_row(app, collection, &row) {
            Ok(row) => row,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    };
    println!("{}", mosql::to_json(&inserted));
    0
}

/// Change the rows of an app's collection that match a filter.
fn app_update(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &[
            "data", "url", "token", "filter", "json", "db-url", "db-token",
        ],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app update: {e}");
        return 2;
    }
    let (Some(app), Some(collection)) = (parsed.first(), parsed.second()) else {
        eprintln!("nanobase app update: an app and a collection are required");
        return 2;
    };
    let Some(text) = parsed.get("json") else {
        eprintln!("nanobase app update: --json is required");
        return 2;
    };
    let changes = match mosql::parse_json(text) {
        Ok(changes) => changes,
        Err(e) => {
            eprintln!("nanobase app update: --json is not JSON: {e}");
            return 2;
        }
    };
    let filter = match filter_of(&parsed) {
        Ok(filter) => filter,
        Err(e) => {
            eprintln!("nanobase app update: {e}");
            return 2;
        }
    };
    let updated = if let Some(client) = api_client(&parsed) {
        let mut body = mosql::Object::new();
        body.set("filter", filter);
        body.set("changes", changes);
        match client.request(
            "PATCH",
            &format!("/api/apps/{app}/db/{collection}"),
            Some(&Value::Object(body)),
        ) {
            Ok(value) => value,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    } else {
        let Some(service) = service_or_exit(&parsed) else {
            return 1;
        };
        match service.update_row(app, collection, &filter, &changes) {
            Ok(row) => row,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    };
    println!("{}", mosql::to_json(&updated));
    0
}

/// Take the rows of an app's collection that match a filter out.
fn app_delete_rows(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &["data", "url", "token", "filter", "db-url", "db-token"],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app delete-rows: {e}");
        return 2;
    }
    let (Some(app), Some(collection)) = (parsed.first(), parsed.second()) else {
        eprintln!("nanobase app delete-rows: an app and a collection are required");
        return 2;
    };
    let filter = match filter_of(&parsed) {
        Ok(filter) => filter,
        Err(e) => {
            eprintln!("nanobase app delete-rows: {e}");
            return 2;
        }
    };
    let removed = if let Some(client) = api_client(&parsed) {
        let mut path = format!("/api/apps/{app}/db/{collection}");
        let query = query_string(&parsed, &filter);
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query);
        }
        match client.request("DELETE", &path, None) {
            Ok(value) => value.get("removed").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    } else {
        let Some(service) = service_or_exit(&parsed) else {
            return 1;
        };
        match service.remove_rows(app, collection, &filter) {
            Ok(removed) => removed,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        }
    };
    println!("{removed} row(s) removed");
    0
}

/// The filter a command asks for.
fn filter_of(parsed: &Parsed) -> Result<Value, String> {
    match parsed.get("filter") {
        None => Ok(Value::Object(mosql::Object::new())),
        Some(text) => mosql::parse_json(text).map_err(|e| format!("--filter is not JSON: {e}")),
    }
}

/// The query string a request carries: the filter, the limit, the offset.
fn query_string(parsed: &Parsed, filter: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    let filter_text = mosql::to_json(filter);
    if filter_text != "{}" {
        parts.push(format!(
            "filter={}",
            crate::http::percent_encode(&filter_text)
        ));
    }
    if let Some(limit) = parsed.get("limit") {
        parts.push(format!("limit={}", crate::http::percent_encode(limit)));
    }
    if let Some(offset) = parsed.get("offset") {
        parts.push(format!("offset={}", crate::http::percent_encode(offset)));
    }
    parts.join("&")
}

/// A number a command asked for.
fn limit_of(parsed: &Parsed) -> Option<u64> {
    parsed.get("limit").and_then(|v| v.parse().ok())
}

/// A number a command asked for.
fn offset_of(parsed: &Parsed) -> Option<u64> {
    parsed.get("offset").and_then(|v| v.parse().ok())
}

fn app_list(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app list: {e}");
        return 2;
    }
    let mut views: Vec<AppView> = Vec::new();
    if let Some(client) = api_client(&parsed) {
        let value = match client.request("GET", "/api/apps", None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(view) = AppView::from_value(item) {
                    views.push(view);
                }
            }
        }
    } else {
        let platform = match platform_or_exit(&parsed) {
            Some(p) => p,
            None => return 1,
        };
        for name in platform.app_names() {
            if let Some(app) = platform.app_by_name(name) {
                views.push(AppView::of(app));
            }
        }
    }
    if views.is_empty() {
        println!("no apps yet — `nanobase app new <name>` creates one");
        return 0;
    }
    println!("NAME                           ID                       TYPE       DATABASE");
    for view in &views {
        println!(
            "{:<30} {:<24} {:<10} {}",
            view.name, view.id, view.app_type, view.database
        );
    }
    0
}

fn app_show(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app show: {e}");
        return 2;
    }
    let Some(key) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app show: an app id or name is required");
        return 2;
    };
    if let Some(client) = api_client(&parsed) {
        let value = match client.request("GET", &format!("/api/apps/{key}"), None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let Some(view) = AppView::from_value(&value) else {
            eprintln!("nanobase: the platform sent an app this command cannot read");
            return 1;
        };
        print_app(&view, None);
        return 0;
    }
    let platform = match platform_or_exit(&parsed) {
        Some(p) => p,
        None => return 1,
    };
    let Some(app) = platform.find_app(&key) else {
        eprintln!("nanobase: no app for {key:?}");
        return 1;
    };
    let view = AppView::of(app);
    let root = Catalog::open(&parsed).ok().and_then(|c| c.root());
    print_app(
        &view,
        root.as_ref().map(|root| root.join("apps")).as_deref(),
    );
    0
}

fn app_export(rest: &[String]) -> u8 {
    // The secrets are out unless they are asked for: an export is a
    // document somebody else reads, and a document with the database
    // password in it is not a document anybody else should have.
    let parsed = parse_args(
        rest,
        &["out", "data", "url", "token", "db-url", "db-token"],
        &["with-secrets"],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app export: {e}");
        return 2;
    }
    let Some(key) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app export: an app id or name is required");
        return 2;
    };
    let doc = if let Some(client) = api_client(&parsed) {
        let value = match client.request("GET", &format!("/api/apps/{key}"), None) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        match AppView::from_value(&value) {
            Some(view) => view.doc,
            None => {
                eprintln!("nanobase: the platform sent an app this command cannot read");
                return 1;
            }
        }
    } else {
        let platform = match platform_or_exit(&parsed) {
            Some(p) => p,
            None => return 1,
        };
        let Some(app) = platform.find_app(&key) else {
            eprintln!("nanobase: no app for {key:?}");
            return 1;
        };
        app.doc.clone()
    };
    let body = if parsed.has("with-secrets") {
        doc.to_json_pretty()
    } else {
        doc.to_json_pretty_redacted()
    };
    write_out(parsed.get("out"), &body)
}

fn app_rename(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token", "db-url", "db-token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app rename: {e}");
        return 2;
    }
    let (Some(key), Some(new_name)) = (
        parsed.first().map(str::to_string),
        parsed.second().map(str::to_string),
    ) else {
        eprintln!("nanobase app rename: needs an app and a new name\n");
        eprintln!("example: nanobase app rename todos todo-list");
        return 2;
    };
    if let Some(client) = api_client(&parsed) {
        let mut app = mosql::Object::new();
        app.set("name", Value::str(new_name.clone()));
        let mut body = mosql::Object::new();
        body.set("app", Value::Object(app));
        let value = match client.request(
            "PATCH",
            &format!("/api/apps/{key}"),
            Some(&Value::Object(body)),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        println!("Renamed to {new_name} ({id})");
        return 0;
    }
    let mut platform = match platform_or_exit(&parsed) {
        Some(p) => p,
        None => return 1,
    };
    let id = match platform.rename_app(&key, &new_name) {
        Ok(app) => app.id.clone(),
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    if save_or_exit(&parsed, &platform).is_none() {
        return 1;
    }
    println!("Renamed to {new_name} ({id})");
    0
}

fn app_clone(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &[
            "data",
            "url",
            "token",
            "db-url",
            "db-token",
            "database",
            "database-token",
        ],
        &["copy-data", "share-database"],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app clone: {e}");
        return 2;
    }
    let (Some(key), Some(new_name)) = (
        parsed.first().map(str::to_string),
        parsed.second().map(str::to_string),
    ) else {
        eprintln!("nanobase app clone: needs an app and a new name\n");
        eprintln!("example: nanobase app clone todos todos-staging");
        return 2;
    };
    // What happens to the rows. The default is that they do not come:
    // a clone that quietly shared a live database would be a way to
    // write to production by accident.
    let data = if parsed.has("share-database") {
        crate::clone::CloneData::Share
    } else if parsed.has("copy-data") {
        crate::clone::CloneData::Copy
    } else {
        crate::clone::CloneData::Omit
    };
    let request = crate::clone::CloneRequest {
        name: new_name.clone(),
        data,
        database: parsed.get("database").map(str::to_string),
        token: parsed.get("database-token").map(str::to_string),
    };
    if let Some(client) = api_client(&parsed) {
        let mut body = mosql::Object::new();
        body.set("name", Value::str(new_name.clone()));
        body.set("data", Value::str(data.as_str()));
        if let Some(url) = request.database.as_deref() {
            body.set("database", Value::str(url));
        }
        if let Some(token) = request.token.as_deref() {
            body.set("token", Value::str(token));
        }
        let value = match client.request(
            "POST",
            &format!("/api/apps/{key}/clone"),
            Some(&Value::Object(body)),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        print_clone_summary(&value);
        return 0;
    }
    let service = match service_or_exit(&parsed) {
        Some(service) => service,
        None => return 1,
    };
    let summary = match service.clone_app(&key, &request) {
        Ok(summary) => summary,
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    print_clone_summary(&summary.to_value());
    0
}

/// What a clone did, in words.
///
/// The last line is the one that matters: whether the clone's rows live
/// in a database of their own, in the source's database, or nowhere yet.
fn print_clone_summary(summary: &Value) {
    let id = summary.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = summary.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let rows = summary
        .get("rowsCopied")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as usize;
    let shares = summary
        .get("sharesDatabase")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let has = summary
        .get("hasDatabase")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    println!("Cloned to {name} ({id})");
    if shares {
        println!("  the clone and the source share one database");
    } else if rows > 0 {
        println!("  copied {rows} row(s) into the clone's own database");
    } else {
        println!("  configuration and schema only; the rows were not copied");
    }
    if !has {
        println!("  the clone has no database yet; give it one with");
        println!("    nanobase app update {name} --database URL");
    }
}

fn app_remove(rest: &[String]) -> u8 {
    let parsed = parse_args(
        rest,
        &["confirm", "data", "url", "token", "db-url", "db-token"],
        &[],
    );
    if let Some(e) = parsed.error {
        eprintln!("nanobase app remove: {e}");
        return 2;
    }
    let Some(key) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app remove: an app id or name is required");
        return 2;
    };
    let confirm = parsed.get("confirm").unwrap_or_default().to_string();
    if let Some(client) = api_client(&parsed) {
        let mut body = mosql::Object::new();
        body.set("confirm", Value::str(confirm.clone()));
        let value = match client.request(
            "DELETE",
            &format!("/api/apps/{key}"),
            Some(&Value::Object(body)),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let id = value.get("id").and_then(|v| v.as_str()).unwrap_or_default();
        let name = AppView::from_value(&value)
            .map(|v| v.name)
            .unwrap_or_else(|| key.clone());
        println!("Deleted {name} ({id})");
        println!("  the app's data directory was left in place");
        return 0;
    }
    let service = match service_or_exit(&parsed) {
        Some(svc) => svc,
        None => return 1,
    };
    let removed = match service.remove(&key, &confirm) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    let id = removed
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let name = removed
        .get("doc")
        .and_then(|d| d.get("app"))
        .and_then(|a| a.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    println!("Deleted {name} ({id})");
    println!("  the app's data directory was left in place:");
    let root = Catalog::open(&parsed)
        .ok()
        .and_then(|catalog| catalog.root());
    if let Some(root) = root {
        println!("  {}", root.join("apps").join(&id).display());
    }
    0
}
