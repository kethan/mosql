//! `nanobase-serverless` — the client build.
//!
//! The same command line as the server build, with two things missing:
//! there is no listener, and there is no local file. The platform's own
//! state lives in a serverless database, reached over HTTP, and the
//! databases the apps use are serverless databases too.
//!
//! ```sh
//! nanobase-serverless init --email ada@example.com --password "..."
//! nanobase-serverless login ada@example.com
//! nanobase-serverless app list
//! ```
//!
//! The database is named by `--db-url`, or by `NANOBASE_DB_URL`:
//!
//! ```sh
//! export NANOBASE_DB_URL="https://your-database.turso.io"
//! export NANOBASE_DB_TOKEN="..."
//! ```

use std::process::ExitCode;

fn main() -> ExitCode {
    ExitCode::from(nanobase::cli::run(std::env::args().skip(1).collect()))
}
