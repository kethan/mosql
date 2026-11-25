//! `nanobase` — the server build.
//!
//! The command line lives in the library ([`nanobase::cli`]), because
//! there are two binaries: this one, which listens on a socket and keeps
//! the platform in a local SQLite file, and `nanobase-serverless`, which
//! has neither.

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(nanobase::cli::run(std::env::args().skip(1).collect()))
}
