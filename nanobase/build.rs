//! Compiles the vendored SQLite amalgamation into the binary.
//!
//! SQLite is here as source rather than as a crate for two reasons. The
//! platform is a single small binary, and compiling the engine ourselves lets
//! every optional feature be omitted at the preprocessor level — a prebuilt
//! library cannot be trimmed that way. And there is no build-time download and
//! no version to drift: the amalgamation is the official distribution (public
//! domain), copied from `libsqlite3-sys`, which is what `rusqlite` ships.
//!
//! The object file is handed to the linker directly instead of being archived
//! into `.a`/`.lib`: that avoids needing `ar` on Unix and `lib.exe` on Windows,
//! neither of which is guaranteed to be on `PATH` from a build script.
//!
//! Flags are chosen for a small, fast, single-tenant engine: everything
//! optional is omitted, and double-quoted string literals are refused
//! (`SQLITE_DQS=0`) so a typo in a column name cannot silently become a
//! string constant — which is a real bug class in dynamically built SQL.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let target = env::var("TARGET").expect("TARGET");
    let msvc = target.contains("msvc");

    // The amalgamation is one file, but a change to the header must rebuild.
    println!("cargo:rerun-if-changed=vendor/sqlite/sqlite3.c");
    println!("cargo:rerun-if-changed=vendor/sqlite/sqlite3.h");

    let defines: &[&str] = &[
        "SQLITE_DQS=0",
        "SQLITE_THREADSAFE=1",
        "SQLITE_DEFAULT_MEMSTATUS=0",
        "SQLITE_LIKE_DOESNT_MATCH_BLOBS",
        "SQLITE_OMIT_LOAD_EXTENSION",
        "SQLITE_OMIT_DEPRECATED",
        "SQLITE_OMIT_SHARED_CACHE",
        "SQLITE_OMIT_TRACE",
        "SQLITE_OMIT_GET_TABLE",
        // Nothing here needs a UTF-16 database, `sqlite3_column_decltype`,
        // `PRAGMA compile_options`, or auto-vacuum. Each is a chunk of code
        // that would otherwise be linked in for nothing.
        "SQLITE_OMIT_UTF16",
        "SQLITE_OMIT_DECLTYPE",
        "SQLITE_OMIT_COMPILEOPTION_DIAGS",
        "SQLITE_OMIT_AUTOVACUUM",
    ];

    let src = PathBuf::from("vendor/sqlite/sqlite3.c");
    let obj = out.join(if msvc { "sqlite3.obj" } else { "sqlite3.o" });

    let (compiler, mut args): (String, Vec<String>) = if msvc {
        // `/MD` matches the dynamic CRT Rust's MSVC target links against.
        let mut a = vec![
            "/O2".into(),
            "/MD".into(),
            "/c".into(),
            "/nologo".into(),
            format!("/Fo{}", obj.display()),
        ];
        for d in defines {
            a.push(format!("/D{d}"));
        }
        ("cl.exe".to_string(), a)
    } else {
        let cc = env::var(format!("CC_{}", target.to_lowercase().replace('-', "_")))
            .or_else(|_| env::var("TARGET_CC"))
            .or_else(|_| env::var("CC"))
            .unwrap_or_else(|_| "cc".to_string());
        let mut a = vec![
            "-O2".into(),
            // Keep SQLite's many symbols out of the export table: they are
            // internal, and exporting them both grows the binary and risks
            // colliding with a host application that links its own SQLite.
            "-fvisibility=hidden".into(),
            "-ffunction-sections".into(),
            "-fdata-sections".into(),
            // SQLite never unwinds, so its frames need no `.eh_frame`; Rust
            // emits unwind info for its own code regardless.
            "-fno-asynchronous-unwind-tables".into(),
            "-c".into(),
            src.display().to_string(),
            "-o".into(),
            obj.display().to_string(),
            "-Ivendor/sqlite".into(),
        ];
        for d in defines {
            a.push(format!("-D{d}"));
        }
        (cc, a)
    };

    if msvc {
        args.push(src.display().to_string());
    }

    let status = Command::new(&compiler)
        .args(&args)
        .status()
        .unwrap_or_else(|e| panic!("cannot run {compiler}: {e}"));
    if !status.success() {
        panic!("{compiler} failed with {status}");
    }

    // Drop unreferenced code on Unix. Without this, `-ffunction-sections`
    // splits everything up but nothing is thrown away.
    if !msvc {
        println!("cargo:rustc-link-arg=-Wl,--gc-sections");
    }
    println!("cargo:rustc-link-arg={}", obj.display());
}
