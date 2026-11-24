//! `nanobase` — the command line.
//!
//! Today this is the part of the platform that touches app documents: create
//! one, check one, and hand one to someone else without the secrets. The
//! server, onboarding and app CRUD are added on top of the same
//! [`AppDoc`](nanobase::AppDoc) type.

use std::process::ExitCode;

use nanobase::{AppDoc, ConfigError, Secrets, VERSION};

const USAGE: &str = r#"nanobase — a PocketBase-style backend on your own database

USAGE:
    nanobase new <name> [--out <file>]    write a new, empty app document
    nanobase validate <file>              check a document, list every problem
    nanobase redact <file> [--out <file>] copy a document without its secrets
    nanobase --version
    nanobase --help

An app document is the JSON file `nanobase import` reads and `nanobase export`
writes: { meta, app, config, schema, data }.

EXAMPLES:
    nanobase new todos --out todos.json
    nanobase validate todos.json
    nanobase redact todos.json --out todos.shared.json
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

/// `--out <file>`, if present, and the positional arguments.
fn split_out(rest: &[String]) -> (Option<String>, Vec<&str>) {
    let mut out = None;
    let mut positional = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "--out" {
            if i + 1 >= rest.len() {
                eprintln!("nanobase: --out needs a file path");
                std::process::exit(2);
            }
            out = Some(rest[i + 1].clone());
            i += 2;
        } else {
            positional.push(rest[i].as_str());
            i += 1;
        }
    }
    (out, positional)
}

fn cmd_new(rest: &[String]) -> u8 {
    let (out, positional) = split_out(rest);
    let Some(name) = positional.first().copied() else {
        eprintln!("nanobase new: an app name is required\n");
        eprintln!("example: nanobase new todos --out todos.json");
        return 2;
    };
    let doc = AppDoc::empty(name);
    if let Err(e) = doc.validate() {
        return report(&e);
    }
    write_out(out.as_deref(), &doc.to_json_pretty())
}

fn cmd_validate(rest: &[String]) -> u8 {
    let (out, positional) = split_out(rest);
    let Some(path) = positional.first().copied() else {
        eprintln!("nanobase validate: a file path is required");
        return 2;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nanobase: cannot read {path}: {e}");
            return 1;
        }
    };
    let secrets = Secrets::from_env();
    let doc = match AppDoc::load(&text, &secrets) {
        Ok(d) => d,
        Err(e) => return report(&e),
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
    if let Some(path) = out {
        return write_out(Some(&path), &doc.to_json_pretty());
    }
    0
}

fn cmd_redact(rest: &[String]) -> u8 {
    let (out, positional) = split_out(rest);
    let Some(path) = positional.first().copied() else {
        eprintln!("nanobase redact: a file path is required");
        return 2;
    };
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nanobase: cannot read {path}: {e}");
            return 1;
        }
    };
    match AppDoc::parse(&text) {
        Ok(doc) => write_out(out.as_deref(), &doc.to_json_pretty_redacted()),
        Err(e) => report(&e),
    }
}

/// Print the problems, one per line, most specific first. The JSON envelope
/// goes to stderr so a caller can still parse stdout.
fn report(err: &ConfigError) -> u8 {
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
