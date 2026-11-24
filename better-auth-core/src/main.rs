//! `better-auth-core` — the auth engine as a standalone server.
//!
//! ```text
//! better-auth-core [--listen 0.0.0.0:8787] [--data ./auth.db]
//! ```
//!
//! Everything else is configuration by environment:
//!
//! | variable | meaning |
//! |---|---|
//! | `BETTER_AUTH_SECRET` | HS256 key, 32+ characters. Required. |
//! | `BETTER_AUTH_URL` | Public origin of this server, e.g. `https://app.example.com`. Required. |
//! | `BETTER_AUTH_DATA` | SQLite file, `:memory:` for an ephemeral server. Default `./better-auth.db`. |
//! | `BETTER_AUTH_LISTEN` | Bind address. Default `0.0.0.0:8787`. |
//! | `BETTER_AUTH_TRUSTED_ORIGINS` | Comma-separated origins allowed for CORS and redirects. |
//! | `BETTER_AUTH_HTTP` | `curl` (default when `curl` exists), `plain` (http only), or `none`. |
//! | `BETTER_AUTH_MAILER` | `console` (default) or `none`. |
//! | `BETTER_AUTH_REQUIRE_EMAIL_VERIFICATION` | `1` to refuse sign-in until verified. |
//! | `BETTER_AUTH_DISABLE_ANONYMOUS` | `1` to turn anonymous sign-in off. |
//! | `<PROVIDER>_CLIENT_ID`, `<PROVIDER>_CLIENT_SECRET` | Enables that OAuth provider. |
//!
//! Run with `--help` for the provider list.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use better_auth_core::auth::{
    Auth, AuthConfig, ConsoleMailer, CurlHttpClient, HttpClient, PlainHttpClient,
};
use better_auth_core::http::{self, Request, Response};
use better_auth_core::oauth_presets::PRESETS;
use better_auth_core::routes;
use better_auth_core::store::http_store::HttpAuthStore;
use better_auth_core::store::tcp_store::TcpAuthStore;
use better_auth_core::store::{AuthStore, MemoryStore, SqliteAuthStore};
use mosql::Value;

struct Args {
    listen: Option<String>,
    data: Option<String>,
    /// Run the store server instead of the authentication server.
    /// `http` or `tcp`.
    store_server: Option<String>,
    help: bool,
    version: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        listen: None,
        data: None,
        store_server: None,
        help: false,
        version: false,
    };
    let all: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < all.len() {
        let arg = all[i].clone();
        i += 1;
        match arg.as_str() {
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            "--listen" | "-l" => {
                let value = all.get(i).cloned().ok_or("--listen needs an address")?;
                i += 1;
                args.listen = Some(value);
            }
            "--data" | "-d" => {
                let value = all.get(i).cloned().ok_or("--data needs a path")?;
                i += 1;
                args.data = Some(value);
            }
            "--store-server" => {
                // The kind may follow as its own word, or be left out.
                let kind = match all.get(i) {
                    Some(k) if k == "http" || k == "tcp" => {
                        i += 1;
                        k.clone()
                    }
                    _ => "http".to_string(),
                };
                args.store_server = Some(kind);
            }
            other if other.starts_with("--listen=") => {
                args.listen = Some(other["--listen=".len()..].to_string())
            }
            other if other.starts_with("--data=") => {
                args.data = Some(other["--data=".len()..].to_string())
            }
            other => return Err(format!("unknown argument {other:?}; try --help")),
        }
    }
    Ok(args)
}

fn usage() -> String {
    let providers: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
    format!(
        "better-auth-core {}\n\n\
         A Better Auth compatible authentication server with bearer tokens.\n\n\
         USAGE:\n    better-auth-core [OPTIONS]\n\n\
         OPTIONS:\n    --listen ADDR   address to bind (default: BETTER_AUTH_LISTEN or 0.0.0.0:8787)\n    \
         --data PATH     SQLite file, or :memory: (default: BETTER_AUTH_DATA or ./better-auth.db)\n    \
         --store-server [http|tcp]  serve the store protocol instead of the auth routes\n    \
         -h, --help      show this help\n    -V, --version   show the version\n\n\
         ENVIRONMENT:\n    \
         BETTER_AUTH_SECRET          HS256 key, at least 32 characters (required)\n    \
         BETTER_AUTH_URL             public origin, e.g. https://app.example.com (required)\n    \
         BETTER_AUTH_TRUSTED_ORIGINS extra origins allowed for CORS and redirects\n    \
         BETTER_AUTH_HTTP           curl | plain | none (default: curl when available)\n    \
         BETTER_AUTH_MAILER         console | none (default: console)\n\n\
         STORE:\n    \
         BETTER_AUTH_STORE          sqlite | memory | http | tcp (default: sqlite)\n    \
         BETTER_AUTH_STORE_URL      the store's address, for http and tcp\n    \
         BETTER_AUTH_STORE_TOKEN    a bearer token for the store, if it wants one\n\n\
         OAUTH PROVIDERS  (set <NAME>_CLIENT_ID and <NAME>_CLIENT_SECRET):\n    {}\n\n\
         Routes hang off /api/auth; see the README for the full table.",
        env!("CARGO_PKG_VERSION"),
        providers.join(", ")
    )
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!("{}", usage());
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("better-auth-core {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("better-auth-core: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: Args) -> Result<(), String> {
    let data = args
        .data
        .clone()
        .or_else(|| env("BETTER_AUTH_DATA"))
        .unwrap_or_else(|| "./better-auth.db".into());
    let addr: SocketAddr = args
        .listen
        .clone()
        .or_else(|| env("BETTER_AUTH_LISTEN"))
        .unwrap_or_else(|| "0.0.0.0:8787".into())
        .parse()
        .map_err(|_| "listen address must be ip:port")?;
    if let Some(kind) = &args.store_server {
        return run_store_server(&data, addr, kind);
    }
    let config = AuthConfig::from_env().map_err(|e| e.to_string())?;
    let (oauth, problems) = better_auth_core::oauth_presets::OAuthRegistry::from_env();
    for problem in &problems {
        eprintln!("warning: {problem}");
    }

    let http: Option<Arc<dyn HttpClient>> = match env("BETTER_AUTH_HTTP").as_deref() {
        Some("none") => None,
        Some("plain") => Some(Arc::new(PlainHttpClient)),
        Some("curl") => Some(Arc::new(CurlHttpClient)),
        _ => {
            if oauth.is_empty() {
                None
            } else if which_curl() {
                Some(Arc::new(CurlHttpClient))
            } else {
                eprintln!("warning: curl not found; OAuth over https:// will not work (set BETTER_AUTH_HTTP=plain for http:// providers)");
                Some(Arc::new(PlainHttpClient))
            }
        }
    };

    let store = open_store(&data, &http)?;
    let mailer: Option<Arc<dyn better_auth_core::auth::Mailer>> =
        match env("BETTER_AUTH_MAILER").as_deref() {
            Some("none") => None,
            _ => Some(Arc::new(ConsoleMailer)),
        };

    let auth = Arc::new(
        Auth::new(store, config)
            .with_oauth(oauth)
            .with_http_opt(http)
            .with_mailer_opt(mailer),
    );
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(2, 16);

    let handler_auth = Arc::clone(&auth);
    eprintln!(
        "better-auth-core {} on http://{addr}  data={data}  providers={}  mailer={}",
        env!("CARGO_PKG_VERSION"),
        if auth.oauth.is_empty() {
            "none".to_string()
        } else {
            auth.oauth.ids().join(",")
        },
        if auth.can_mail() { "on" } else { "off" },
    );
    let engine = handler_auth;
    http::serve(addr, threads, move |req: Request| {
        if req.path == "/healthz" || req.path == "/api/auth/healthz" {
            return Response::json(
                200,
                &Value::Object(better_auth_core::object! {
                    "ok" => Value::Bool(true),
                    "service" => Value::str("better-auth-core"),
                }),
            );
        }
        routes::respond(Arc::clone(&engine), req)
    })
    .map_err(|e| format!("server stopped: {e}"))
}

/// Serve the store protocol, for a deployment whose database is elsewhere.
///
/// The engine on the other side talks to this with the HTTP or TCP adapter;
/// the records live in the SQLite file named by `--data`.
fn run_store_server(data: &str, addr: SocketAddr, kind: &str) -> Result<(), String> {
    let store = if data == ":memory:" {
        SqliteAuthStore::open_memory()
    } else {
        let path = PathBuf::from(data);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
        }
        SqliteAuthStore::open(&path)
    }
    .map_err(|e| format!("cannot open {data}: {e}"))?;
    let server = Arc::new(better_auth_core::query_server::QueryServer::new(
        Arc::new(store) as Arc<dyn AuthStore>,
    ));
    eprintln!(
        "better-auth-core {} store server ({kind}) on {addr}  data={data}",
        env!("CARGO_PKG_VERSION"),
    );
    match kind {
        "tcp" => {
            let listener = std::net::TcpListener::bind(addr)
                .map_err(|e| format!("cannot bind {addr}: {e}"))?;
            better_auth_core::query_server::serve_tcp(server, listener)
                .map_err(|e| format!("store server stopped: {e}"))
        }
        "http" => better_auth_core::query_server::serve_http(server, addr, 4)
            .map_err(|e| format!("store server stopped: {e}")),
        other => Err(format!(
            "unknown store server kind {other:?}: use http or tcp"
        )),
    }
}

/// Open the store the environment asks for.
///
/// `BETTER_AUTH_STORE` chooses the kind: `sqlite` (the default), `memory`,
/// `http` — a serverless database behind an HTTP API — or `tcp`, a database
/// service on a socket. The address comes from `BETTER_AUTH_STORE_URL`, and
/// a bearer token for the service from `BETTER_AUTH_STORE_TOKEN`.
fn open_store(
    data: &str,
    http: &Option<Arc<dyn HttpClient>>,
) -> Result<Arc<dyn AuthStore>, String> {
    let kind = env("BETTER_AUTH_STORE").unwrap_or_else(|| "sqlite".to_string());
    match kind.as_str() {
        "sqlite" | "file" => {
            let store = if data == ":memory:" {
                SqliteAuthStore::open_memory()
            } else {
                let path = PathBuf::from(data);
                if let Some(parent) = path.parent() {
                    if !parent.as_os_str().is_empty() {
                        std::fs::create_dir_all(parent)
                            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
                    }
                }
                SqliteAuthStore::open(&path)
            }
            .map_err(|e| format!("cannot open {data}: {e}"))?;
            Ok(Arc::new(store))
        }
        "memory" => Ok(Arc::new(MemoryStore::new())),
        "http" | "serverless" => {
            let url = env("BETTER_AUTH_STORE_URL")
                .ok_or_else(|| "BETTER_AUTH_STORE_URL must name the store's address".to_string())?;
            let client: Arc<dyn HttpClient> = match http {
                Some(client) => Arc::clone(client),
                None => Arc::new(CurlHttpClient),
            };
            let mut store = HttpAuthStore::new(url, client);
            if let Some(token) = env("BETTER_AUTH_STORE_TOKEN") {
                store = store.with_token(token);
            }
            Ok(Arc::new(store))
        }
        "tcp" => {
            let address = env("BETTER_AUTH_STORE_URL")
                .ok_or_else(|| "BETTER_AUTH_STORE_URL must name the store's address".to_string())?;
            let mut store = TcpAuthStore::new(address);
            if let Some(token) = env("BETTER_AUTH_STORE_TOKEN") {
                store = store.with_token(token);
            }
            Ok(Arc::new(store))
        }
        other => Err(format!(
            "unknown store {other:?}: use sqlite, memory, http or tcp"
        )),
    }
}

fn which_curl() -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    path.split(':')
        .any(|dir| std::path::Path::new(dir).join("curl").is_file())
}
