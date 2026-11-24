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
use better_auth_core::store::SqliteAuthStore;
use mosql::Value;

struct Args {
    listen: Option<String>,
    data: Option<String>,
    help: bool,
    version: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        listen: None,
        data: None,
        help: false,
        version: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--help" | "-h" => args.help = true,
            "--version" | "-V" => args.version = true,
            "--listen" | "-l" => args.listen = Some(it.next().ok_or("--listen needs an address")?),
            "--data" | "-d" => args.data = Some(it.next().ok_or("--data needs a path")?),
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
         -h, --help      show this help\n    -V, --version   show the version\n\n\
         ENVIRONMENT:\n    \
         BETTER_AUTH_SECRET          HS256 key, at least 32 characters (required)\n    \
         BETTER_AUTH_URL             public origin, e.g. https://app.example.com (required)\n    \
         BETTER_AUTH_TRUSTED_ORIGINS extra origins allowed for CORS and redirects\n    \
         BETTER_AUTH_HTTP           curl | plain | none (default: curl when available)\n    \
         BETTER_AUTH_MAILER         console | none (default: console)\n\n\
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
    let config = AuthConfig::from_env().map_err(|e| e.to_string())?;
    let data = args
        .data
        .or_else(|| env("BETTER_AUTH_DATA"))
        .unwrap_or_else(|| "./better-auth.db".into());
    let store = if data == ":memory:" {
        SqliteAuthStore::open_memory()
    } else {
        let path = PathBuf::from(&data);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
            }
        }
        SqliteAuthStore::open(&path)
    }
    .map_err(|e| format!("cannot open {data}: {e}"))?;

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
    let mailer: Option<Arc<dyn better_auth_core::auth::Mailer>> =
        match env("BETTER_AUTH_MAILER").as_deref() {
            Some("none") => None,
            _ => Some(Arc::new(ConsoleMailer)),
        };

    let auth = Arc::new(
        Auth::new(Arc::new(store), config)
            .with_oauth(oauth)
            .with_http_opt(http)
            .with_mailer_opt(mailer),
    );
    let addr: SocketAddr = args
        .listen
        .or_else(|| env("BETTER_AUTH_LISTEN"))
        .unwrap_or_else(|| "0.0.0.0:8787".into())
        .parse()
        .map_err(|_| "listen address must be ip:port")?;
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

fn which_curl() -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    path.split(':')
        .any(|dir| std::path::Path::new(dir).join("curl").is_file())
}
