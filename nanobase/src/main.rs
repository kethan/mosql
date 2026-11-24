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

use nanobase::platform::{FileStore, PlatformStore};
use nanobase::{
    check_email, create_owner, issue_token, now_secs, verify_login, AppDoc, ConfigError, Platform,
    Secrets, VERSION,
};
use uflite::db::block_on;

const USAGE: &str = r#"nanobase — a PocketBase-style backend on your own database

USAGE:
    nanobase init   [--email E] [--password P] [--data FILE]   create the owner
    nanobase login  <email> [--password P] [--data FILE]       print a session token
    nanobase me     [--token T] [--data FILE]                  show who a token belongs to
    nanobase status [--data FILE]                              setup state, app count
    nanobase new <name> [--out <file>]                         write a new, empty app document
    nanobase validate <file>                                   check a document, list every problem
    nanobase redact <file> [--out <file>]                      copy a document without its secrets
    nanobase --version
    nanobase --help

PLATFORM FILE:
    --data FILE       where the platform keeps its state
    NANOBASE_DATA     used when --data is absent
    ./data/nanobase.json   the default

    The file holds the owner's password hash and the key tokens are signed
    with. Protect it like a database: anyone who can read it can sign in.

EXAMPLES:
    nanobase init
    nanobase init --email ada@example.com --password "$(cat ~/.nanobase-pass)"
    nanobase login ada@example.com
    nanobase new todos --out todos.json
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
        "init" => cmd_init(rest),
        "login" => cmd_login(rest),
        "me" => cmd_me(rest),
        "status" => cmd_status(rest),
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

// ------------------------------------------------------------------ args ---

/// Split `--flag value` pairs from positional arguments.
///
/// Returns the flags seen, the positionals, and whether an unknown flag or a
/// missing value was passed — the caller turns that into a usage error rather
/// than every command re-implementing the check.
struct Parsed {
    flags: Vec<(String, String)>,
    positional: Vec<String>,
    error: Option<String>,
}

impl Parsed {
    fn get(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn first(&self) -> Option<&str> {
        self.positional.first().map(String::as_str)
    }
}

fn parse_args(rest: &[String], known: &[&str]) -> Parsed {
    let mut flags = Vec::new();
    let mut positional = Vec::new();
    let mut error = None;
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i].as_str();
        if let Some(name) = arg.strip_prefix("--") {
            if !known.contains(&name) {
                error = Some(format!("unknown option --{name}"));
                break;
            }
            if i + 1 >= rest.len() {
                error = Some(format!("--{name} needs a value"));
                break;
            }
            flags.push((name.to_string(), rest[i + 1].clone()));
            i += 2;
        } else {
            positional.push(arg.to_string());
            i += 1;
        }
    }
    Parsed {
        flags,
        positional,
        error,
    }
}

/// Where the platform's state lives: `--data`, then `NANOBASE_DATA`, then
/// `./data/nanobase.json`.
fn data_path(parsed: &Parsed) -> PathBuf {
    parsed
        .get("data")
        .map(PathBuf::from)
        .or_else(|| std::env::var("NANOBASE_DATA").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("./data/nanobase.json"))
}

fn load_platform(store: &FileStore) -> Result<Platform, u8> {
    block_on(store.load()).map_err(|e| {
        eprintln!("nanobase: {e}");
        1
    })
}

// -------------------------------------------------------------- platform ---

fn cmd_init(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["email", "password", "data"]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase init: {e}");
        return 2;
    }
    let store = FileStore::new(data_path(&parsed));
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
    println!("Platform file: {}", store.path().display());
    println!("Next: nanobase login {}", user.email);
    0
}

fn cmd_login(rest: &[String]) -> u8 {
    let parsed = parse_args(rest, &["password", "data"]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase login: {e}");
        return 2;
    }
    let Some(email) = parsed.first().map(str::to_string) else {
        eprintln!("nanobase login: an email is required\n");
        eprintln!("example: nanobase login ada@example.com");
        return 2;
    };
    let store = FileStore::new(data_path(&parsed));
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
    let parsed = parse_args(rest, &["token", "data"]);
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
    let store = FileStore::new(data_path(&parsed));
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
    let parsed = parse_args(rest, &["data"]);
    if let Some(e) = parsed.error {
        eprintln!("nanobase status: {e}");
        return 2;
    }
    let store = FileStore::new(data_path(&parsed));
    let platform = match load_platform(&store) {
        Ok(p) => p,
        Err(code) => return code,
    };
    println!("platform file: {}", store.path().display());
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
    let parsed = parse_args(rest, &["out"]);
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
    let parsed = parse_args(rest, &["out"]);
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
    let parsed = parse_args(rest, &["out"]);
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
