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
use std::path::{Path, PathBuf};
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

    if msvc {
        build_msvc(&out, &target, &src, &obj, defines);
    } else {
        build_unix(&target, &src, &obj, defines);
    }

    // Drop unreferenced code. Without this, `-ffunction-sections` splits
    // everything up but nothing is thrown away. The flags differ by linker:
    // GNU ld and lld take `--gc-sections`, Apple's takes `-dead_strip`, and
    // the MSVC linker discards unreferenced COMDATs on its own given `/Gy`.
    // Passing the GNU flag to Apple's linker is a hard error, not a warning.
    let apple = target.contains("apple");
    if !msvc && apple {
        println!("cargo:rustc-link-arg=-Wl,-dead_strip");
    } else if !msvc {
        println!("cargo:rustc-link-arg=-Wl,--gc-sections");
    }
    println!("cargo:rustc-link-arg={}", obj.display());
}

/// gcc, clang, and anything else that takes the usual flags.
fn build_unix(target: &str, src: &Path, obj: &Path, defines: &[&str]) {
    let cc = env::var(format!("CC_{}", target.to_lowercase().replace('-', "_")))
        .or_else(|_| env::var("TARGET_CC"))
        .or_else(|_| env::var("CC"))
        .unwrap_or_else(|_| "cc".to_string());
    let mut args = vec![
        "-O2".to_string(),
        // Keep SQLite's many symbols out of the export table: they are
        // internal, and exporting them both grows the binary and risks
        // colliding with a host application that links its own SQLite.
        "-fvisibility=hidden".to_string(),
        "-ffunction-sections".to_string(),
        "-fdata-sections".to_string(),
        // SQLite never unwinds, so its frames need no `.eh_frame`; Rust
        // emits unwind info for its own code regardless.
        "-fno-asynchronous-unwind-tables".to_string(),
        "-c".to_string(),
        src.display().to_string(),
        "-o".to_string(),
        obj.display().to_string(),
        "-Ivendor/sqlite".to_string(),
    ];
    for d in defines {
        args.push(format!("-D{d}"));
    }
    run(&cc, &args);
}

/// MSVC: `cl.exe`, with its environment set up by `vcvarsall.bat`.
///
/// `cl.exe` is not on `PATH` on a clean machine — it needs the `INCLUDE`,
/// `LIB` and `PATH` that `vcvarsall.bat` exports, and cargo's own builds find
/// that script through `vswhere`, which sits at a fixed path in every Visual
/// Studio 2017+ install (Build Tools included). Same route here, so the build
/// needs nothing but a Visual Studio install and no third-party crate.
///
/// The commands are written to a batch file rather than passed through
/// `cmd /C` as one string: every path is then quoted once, by us, instead of
/// being re-parsed by the shell with its own rules.
fn build_msvc(out: &Path, target: &str, src: &Path, obj: &Path, defines: &[&str]) {
    let arch = if target.starts_with("aarch64") {
        "arm64"
    } else if target.starts_with("i686") || target.starts_with("thumbv7a") {
        "x86"
    } else {
        "x64"
    };

    let mut args = vec![
        "/nologo".to_string(),
        "/O2".to_string(),
        // The dynamic CRT, which is what Rust's MSVC target links against.
        "/MD".to_string(),
        // Function-level linking, so the linker can drop what is unused.
        "/Gy".to_string(),
        "/c".to_string(),
        format!("/Fo{}", win_path(obj)),
    ];
    for d in defines {
        args.push(format!("/D{d}"));
    }
    // Absolute, because there is no reason for the source path to depend on
    // where cargo ran us. Not `canonicalize`: on Windows it yields a
    // `\\?\D:\…` verbatim path, which cl.exe refuses to open.
    let src = env::current_dir()
        .expect("cannot read the current directory")
        .join(src);
    args.push(win_path(&src));

    // Each step says what it is doing, and on failure prints its exit code
    // and whatever it printed. The build script's output is the only place a
    // broken toolchain can explain itself, and these lines are what CI shows.
    let log = out.join("build-sqlite.log");
    let mut script = String::from("@echo off\r\n");
    script.push_str("[nanobase] setting up the MSVC environment\r\n");
    script.push_str(&format!(
        "call \"{}\" {arch} >\"{}\" 2>&1\r\n",
        win_path(&vcvarsall()),
        win_path(&log)
    ));
    script.push_str(&format!(
        "if errorlevel 1 (type \"{}\" & echo [nanobase] vcvarsall failed with exit code %errorlevel% & exit /b 1)\r\n",
        win_path(&log)
    ));
    script.push_str("where cl.exe\r\n");
    let quoted: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
    script.push_str(&format!("cl.exe {}\r\n", quoted.join(" ")));
    script.push_str(
        "if errorlevel 1 (echo [nanobase] cl.exe failed with exit code %errorlevel% & exit /b 1)\r\n",
    );
    script.push_str("[nanobase] sqlite3.obj built\r\n");

    let script_path = out.join("build-sqlite.bat");
    std::fs::write(&script_path, script).expect("cannot write build-sqlite.bat");
    // Spawning a `.bat` goes through `cmd.exe`, and Rust quotes the script
    // path itself; it takes no arguments, so there is nothing to escape.
    run(&win_path(&script_path), &[]);
}

/// A path for `cmd.exe` and `cl.exe`: backslashes only. Cargo's `OUT_DIR`
/// mixes separators, and a tool that reads `/` as an option prefix should
/// never be handed one inside a path.
fn win_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

/// The `vcvarsall.bat` of the newest Visual Studio with the C++ tools.
fn vcvarsall() -> PathBuf {
    let program_files = env::var_os("ProgramFiles(x86)")
        .or_else(|| env::var_os("ProgramFiles"))
        .map(PathBuf::from)
        .expect("neither ProgramFiles(x86) nor ProgramFiles is set");
    let vswhere = program_files
        .join("Microsoft Visual Studio")
        .join("Installer")
        .join("vswhere.exe");
    let output = Command::new(&vswhere)
        .args([
            "-latest",
            "-products",
            "*",
            // UTF-8 regardless of the console's code page, so the path below
            // is read correctly even when it contains non-ASCII characters.
            "-utf",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", vswhere.display()));
    let install = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        !install.is_empty(),
        "{} found no Visual Studio with the C++ tools",
        vswhere.display()
    );
    PathBuf::from(install)
        .join("VC")
        .join("Auxiliary")
        .join("Build")
        .join("vcvarsall.bat")
}

fn run(compiler: &str, args: &[String]) {
    let status = Command::new(compiler)
        .args(args)
        .status()
        .unwrap_or_else(|e| panic!("cannot run {compiler}: {e}"));
    if !status.success() {
        panic!("{compiler} failed with {status}");
    }
}
