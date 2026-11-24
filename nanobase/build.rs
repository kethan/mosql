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
        build_msvc(&target, &src, &obj, defines);
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
    run(Command::new(cc).args(&args), "the C compiler");
}

/// MSVC: `cl.exe`, given the environment a Developer Command Prompt would have.
///
/// `cl.exe` is not on `PATH` on a clean machine, and it needs `INCLUDE` and
/// `LIB` to point at the MSVC headers and the Windows SDK. The usual way to
/// get those is `vcvarsall.bat`, but that is a chain of batch files whose
/// behaviour depends on how it was launched — and when it fails, it fails
/// from inside a build script with a message that names nothing. So the
/// paths are computed here instead, from the two facts vcvarsall itself
/// starts from: where Visual Studio is, which `vswhere` tells (it sits at a
/// fixed path in every Visual Studio 2017+ install, Build Tools included),
/// and where the Windows SDK is. Every failure is then a plain statement of
/// what was looked for and where.
///
/// An environment that already has `INCLUDE` and `LIB` — a Developer
/// Command Prompt — is used as is.
fn build_msvc(target: &str, src: &Path, obj: &Path, defines: &[&str]) {
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

    let mut command = match msvc_toolchain(arch) {
        Some((cl, vars)) => {
            let mut c = Command::new(cl);
            c.envs(vars);
            c
        }
        None => Command::new("cl.exe"),
    };
    command.args(&args);
    run(&mut command, "cl.exe");
}

/// The newest Visual Studio's `cl.exe` for `arch`, with the `INCLUDE`,
/// `LIB` and `PATH` it needs — or `None` when the environment is already
/// set up.
fn msvc_toolchain(arch: &str) -> Option<(PathBuf, Vec<(&'static str, std::ffi::OsString)>)> {
    if env::var_os("INCLUDE").is_some() && env::var_os("LIB").is_some() {
        return None;
    }

    let host = match env::consts::ARCH {
        "x86_64" => "Hostx64",
        "aarch64" => "Hostarm64",
        "x86" => "Hostx86",
        other => panic!("no MSVC host directory for {other}"),
    };

    let vs = vs_install_dir();
    let tools = vs.join("VC").join("Tools").join("MSVC");
    let version = newest_version(&tools, "the MSVC tools (VC\\Tools\\MSVC)");
    let msvc = tools.join(&version);
    let bin = msvc.join("bin").join(host).join(arch);
    let cl = bin.join("cl.exe");
    if !cl.is_file() {
        panic!("{} does not exist: no cl.exe for {arch}", cl.display());
    }

    let kits = windows_kits_dir();
    let sdk = newest_version(&kits.join("Include"), "the Windows SDK (Include)");

    let include = vec![
        msvc.join("include"),
        kits.join("Include").join(&sdk).join("ucrt"),
        kits.join("Include").join(&sdk).join("shared"),
        kits.join("Include").join(&sdk).join("um"),
    ];
    let lib = vec![
        msvc.join("lib").join(arch),
        kits.join("Lib").join(&sdk).join("ucrt").join(arch),
        kits.join("Lib").join(&sdk).join("um").join(arch),
    ];
    // Checked one by one, so a missing directory is named here rather than
    // reported by cl.exe as "cannot open include file".
    for dir in include.iter().chain(lib.iter()) {
        assert!(dir.is_dir(), "{} does not exist", dir.display());
    }
    let list = |dirs: &[PathBuf]| -> std::ffi::OsString {
        env::join_paths(dirs).expect("an MSVC path contains a quote character")
    };
    let path = env::join_paths(
        std::iter::once(bin).chain(env::split_paths(&env::var_os("PATH").unwrap_or_default())),
    )
    .expect("an MSVC path contains a quote character");

    Some((
        cl,
        vec![
            ("INCLUDE", list(&include)),
            ("LIB", list(&lib)),
            ("PATH", path),
        ],
    ))
}

/// The installation directory of the newest Visual Studio with the C++
/// tools, as `vswhere` reports it.
///
/// The answer is checked rather than trusted: vswhere's banner and error
/// messages also go to stdout, and a line of help text pasted into a path
/// fails somewhere far from the cause. If nothing in the output is a Visual
/// Studio, the whole output becomes the error message — it is the only thing
/// that can explain why.
fn vs_install_dir() -> PathBuf {
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
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-property",
            "installationPath",
        ])
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", vswhere.display()));
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let install = PathBuf::from(line);
        if install.join("VC").join("Tools").join("MSVC").is_dir() {
            return install;
        }
    }
    panic!(
        "{} found no Visual Studio with the C++ tools. It said:\n{}\n{}",
        vswhere.display(),
        stdout.trim(),
        String::from_utf8_lossy(&output.stderr).trim()
    )
}

/// Where the Windows SDK is installed: `%WindowsSdkDir%` if the environment
/// has it, else the default location shared by every SDK since Windows 10.
fn windows_kits_dir() -> PathBuf {
    if let Some(dir) = env::var_os("WindowsSdkDir").map(PathBuf::from) {
        return dir;
    }
    let program_files = env::var_os("ProgramFiles(x86)")
        .or_else(|| env::var_os("ProgramFiles"))
        .map(PathBuf::from)
        .expect("neither ProgramFiles(x86) nor ProgramFiles is set");
    program_files.join("Windows Kits").join("10")
}

/// The name of the highest-versioned subdirectory of `dir` — `14.44.35207`
/// under `VC\Tools\MSVC`, `10.0.26100.0` under a Windows SDK's `Include`.
fn newest_version(dir: &Path, what: &str) -> String {
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {what} at {}: {e}", dir.display()));
    let mut newest: Option<(Vec<u64>, String)> = None;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let parts: Option<Vec<u64>> = name.split('.').map(|p| p.parse().ok()).collect();
        let Some(parts) = parts else { continue };
        if newest.as_ref().is_none_or(|(best, _)| parts > *best) {
            newest = Some((parts, name));
        }
    }
    match newest {
        Some((_, name)) => name,
        None => panic!("no versioned directory under {} ({what})", dir.display()),
    }
}

/// A path for cl.exe: backslashes only. Cargo's `OUT_DIR` mixes separators,
/// and a tool that reads `/` as an option prefix should never be handed one
/// inside a path.
fn win_path(path: &Path) -> String {
    path.display().to_string().replace('/', "\\")
}

fn run(command: &mut Command, what: &str) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("cannot run {what}: {e}"));
    if !status.success() {
        panic!("{what} failed with {status}");
    }
}
