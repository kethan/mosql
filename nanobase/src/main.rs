//! `nanobase` — the command line.
//!
//! Two halves today: the app document (`new`, `validate`, `redact`) and the
//! platform's own account (`init`, `login`, `me`, `status`). The server, and
//! creating apps through it, are added on top of the same types.
//!
//! Setup is interactive when it can be and scripted when it must be: `init`
//! prompts for an email and password if they are not given as flags, and never
//! prompts when they are, so a CI script or a provisioning tool can run it.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use mosql::Value;
use nanobase::platform::PlatformStore;
use nanobase::SqliteStore;
use nanobase::{
    check_email, create_owner, issue_token, now_secs, verify_login, Api, ApiClient, AppDoc,
    ConfigError, Platform, Secrets, Service, VERSION,
};
use uflite::db::block_on;

const USAGE: &str = r#"nanobase — a PocketBase-style backend on your own database

USAGE:
    nanobase init   [--email E] [--password P] [--data FILE]   create the owner
    nanobase login  <email> [--password P] [--data FILE]       print a session token
    nanobase me     [--token T] [--data FILE]                  show who a token belongs to
    nanobase status [--data FILE]                              setup state, app count

SERVER:
    nanobase serve [--host H] [--port P] [--data FILE]        run the HTTP API
    nanobase serve --host 0.0.0.0 --port 8090

APPS:
    nanobase app new <name>                                   create an empty app
    nanobase app import <file>                                create an app from a document
    nanobase app list                                         id, name, engine
    nanobase app show <id|name>                               everything about one app
    nanobase app export <id|name> [--out F] [--redact]        write an app's document
    nanobase app rename <id|name> <new-name>
    nanobase app clone <id|name> <new-name>                   copy config and schema, not rows
    nanobase app remove <id|name> --confirm <id>
    nanobase new <name> [--out <file>]                         write a new, empty app document
    nanobase validate <file>                                   check a document, list every problem
    nanobase redact <file> [--out <file>]                      copy a document without its secrets
    nanobase --version
    nanobase --help

PLATFORM DATABASE:
    --data FILE       where the platform keeps its state (SQLite)
    NANOBASE_DATA     used when --data is absent
    ./data/nanobase.db     the default

REMOTE PLATFORM:
    --url URL         talk to a running platform instead of the local file
    NANOBASE_URL      used when --url is absent
    --token TOKEN     the bearer token to send
    NANOBASE_TOKEN    used when --token is absent

    The database holds the owner's password hash and the key tokens are
    signed with. Protect it: anyone who can read it can sign in.

EXAMPLES:
    nanobase serve --port 8090
    nanobase app list --url http://localhost:8090 --token "$TOKEN"
    nanobase init
    nanobase init --email ada@example.com --password "$(cat ~/.nanobase-pass)"
    nanobase login ada@example.com
    nanobase app new todos
    nanobase app import todos.json
    nanobase app export todos --out todos.json --redact
    nanobase validate todos.json
"#;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };

    let rest = &args[1..];
    let code = match command {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            0
        }
        "-V" | "--version" | "version" => {
            println!("nanobase {VERSION}");
            0
        }
        "serve" => cmd_serve(rest),
        "init" => cmd_init(rest),
        "login" => cmd_login(rest),
        "me" => cmd_me(rest),
        "status" => cmd_status(rest),
        "app" => cmd_app(rest),
        "new" => cmd_new(rest),
        "validate" => cmd_validate(rest),
        "redact" => cmd_redact(rest),
        other => {
            eprintln!("nanobase: unknown command {other:?}\n");
            eprint!("{USAGE}");
            2
        }
    };
    ExitCode::from(code)
}

/// Run the HTTP API.
///
/// The platform serves the same operations the command line runs, so an
/// operator with `curl`, a script, and the `nanobase` binary all reach one
/// implementation. The listener binds every interface by default: this is a
/// server, and the address to reach it from is the operator's business.
fn cmd_serve(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["host", "port", "data", "url"], &[]);
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
    let store = Arc::new(SqliteStore::new(data_path(&parsed)));
    let service = match Service::new("apps", store as Arc<dyn PlatformStore>) {
        Ok(service) => Arc::new(service),
        Err(e) => {
            eprintln!("nanobase serve: {e}");
            return 1;
        }
    };
    let api = Api::new(Arc::clone(&service));
    let addr = format!("{host}:{port}");
    let socket = match addr.parse::<std::net::SocketAddr>() {
        Ok(socket) => socket,
        Err(_) => {
            eprintln!("nanobase serve: {addr} is not an address");
            return 2;
        }
    };
    println!("nanobase {VERSION} serving on http://{addr}");
    println!("  api:  http://{addr}/api");
    let result = nanobase::http::serve(socket, 8, move |req| api.handle(req));
    if let Err(e) = result {
        eprintln!("nanobase serve: {e}");
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
fn data_path(parsed: &Parsed) -> PathBuf {
    parsed
        .get("data")
        .map(PathBuf::from)
        .or_else(|| std::env::var("NANOBASE_DATA").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("./data/nanobase.db"))
}

fn load_platform(store: &SqliteStore) -> Result<Platform, u8> {
    // An install that predates SQLite kept its state in a JSON file beside
    // where the database now lives. Bring it across once, before the first
    // read; the file is renamed so this cannot run twice.
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
    let parsed = parse_args(rest, &["email", "password", "data"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase init: {e}");
        return 2;
    }
    let store = SqliteStore::new(data_path(&parsed));
    let mut platform = match load_platform(&store) {
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
    if let Err(e) = block_on(store.save(&platform)) {
        eprintln!("nanobase: {e}");
        return 1;
    }

    println!("Owner created: {}", user.email);
    println!("Platform database: {}", store.path().display());
    println!("Next: nanobase login {}", user.email);
    0
}

fn cmd_login(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["password", "data"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase login: {e}");
        return 2;
    }
    let Some(email) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase login: an email is required\n");
        eprintln!("example: nanobase login ada@example.com");
        return 2;
    };
    let store = SqliteStore::new(data_path(&parsed));
    let platform = match load_platform(&store) {
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
            nanobase::SESSION_TTL_SECS
        )
    );
    0
}

fn cmd_me(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["token", "data"], &[]);
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
    let store = SqliteStore::new(data_path(&parsed));
    let platform = match load_platform(&store) {
        Ok(p) => p,
        Err(code) => return code,
    };
    match nanobase::jwt::decode(&token, platform.jwt_key(), now_secs()) {
        Ok(claims) => {
            println!("user:  {} ({})", claims.email, claims.role);
            println!("id:    {}", claims.sub);
            println!("expires: {}", nanobase::iso8601(claims.exp));
            0
        }
        Err(e) => {
            eprintln!("nanobase: {e}");
            1
        }
    }
}

fn cmd_status(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase status: {e}");
        return 2;
    }
    let store = SqliteStore::new(data_path(&parsed));
    let platform = match load_platform(&store) {
        Ok(p) => p,
        Err(code) => return code,
    };
    println!("platform database: {}", store.path().display());
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
        other => {
            eprintln!("nanobase app: unknown subcommand {other:?}\n");
            eprintln!("  new | import | list | show | export | rename | clone | remove");
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
    let store = SqliteStore::new(data_path(parsed));
    match Service::new("apps", Arc::new(store)) {
        Ok(svc) => Some(svc),
        Err(e) => {
            eprintln!("nanobase: {e}");
            None
        }
    }
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
    fn of(record: &nanobase::platform::AppRecord) -> AppView {
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
    let store = SqliteStore::new(data_path(parsed));
    match load_platform(&store) {
        Ok(p) => Some(p),
        Err(code) => {
            let _ = code;
            None
        }
    }
}

fn save_or_exit(parsed: &Parsed, platform: &Platform) -> Option<()> {
    let store = SqliteStore::new(data_path(parsed));
    match block_on(store.save(platform)) {
        Ok(()) => Some(()),
        Err(e) => {
            eprintln!("nanobase: {e}");
            None
        }
    }
}

fn app_new(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase app new: {e}");
        return 2;
    }
    let Some(name) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase app new: an app name is required\n");
        eprintln!("example: nanobase app new todos");
        return 2;
    };
    if let Some(problem) = nanobase::config::check_app_name(&name) {
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
    println!(
        "  data:      {}",
        SqliteStore::new(data_path(&parsed))
            .root()
            .join("apps")
            .join(&id)
            .display()
    );
    println!("  database:  sqlite (default — edit the document to change it)");
    0
}

fn app_import(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
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

fn app_list(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
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
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
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
    let root = SqliteStore::new(data_path(&parsed)).root();
    let view = AppView::of(app);
    print_app(&view, Some(&root.join("apps")));
    0
}

fn app_export(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["out", "data", "url", "token"], &["redact"]);
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
    let body = if parsed.has("redact") {
        doc.to_json_pretty_redacted()
    } else {
        doc.to_json_pretty()
    };
    write_out(parsed.get("out"), &body)
}

fn app_rename(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
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
    let parsed = parse_args(rest, &["data", "url", "token"], &[]);
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
        let mut doc = view.doc.to_value();
        if let Some(app) = doc.get("app").and_then(|v| v.as_object()) {
            let mut copy = app.clone();
            copy.set("name", Value::str(new_name.clone()));
            if let Some(root) = doc.as_object() {
                let mut updated = root.clone();
                updated.set("app", Value::Object(copy));
                doc = Value::Object(updated);
            }
        }
        let created = match client.request("POST", "/api/apps", Some(&doc)) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("nanobase: {e}");
                return 1;
            }
        };
        let id = created
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        println!("Cloned to {new_name} ({id})");
        println!("  configuration and schema only; rows were not copied");
        return 0;
    }
    let mut platform = match platform_or_exit(&parsed) {
        Some(p) => p,
        None => return 1,
    };
    let (id, name) = match platform.clone_app(&key, &new_name, now_secs()) {
        Ok(app) => (app.id.clone(), app.doc.app.name.clone()),
        Err(e) => {
            eprintln!("nanobase: {e}");
            return 1;
        }
    };
    if save_or_exit(&parsed, &platform).is_none() {
        return 1;
    }
    println!("Cloned to {name} ({id})");
    println!("  configuration and schema only; rows were not copied");
    0
}

fn app_remove(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["confirm", "data", "url", "token"], &[]);
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
    println!(
        "  {}",
        SqliteStore::new(data_path(&parsed))
            .root()
            .join("apps")
            .join(&id)
            .display()
    );
    0
}
