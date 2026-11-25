//! nanobase — a PocketBase-style backend platform on your own database.
//!
//! One binary. It holds a platform database of apps, and each app brings its
//! own database, auth providers, storage bucket and permission rules. The
//! runtime API is per-app: `/api/v1/apps/{appId}`.
//!
//! This crate has **no external dependencies**. Everything it needs — JSON,
//! the query builder, the in-memory engine, permission evaluation — comes from
//! `mosql` and `uflite`, which are also dependency-free. That is a
//! requirement, not an accident: the platform is meant to be small enough to
//! ship as a single binary, and the build environment may have no registry
//! access at all.
//!
//! Layout:
//!
//! * [`config`] — an app's configuration, schema and seed data, and the
//!   validation that decides whether an app may be imported.

// The module list is the crate's own feature switch. `api`, `client` and
// `service` are the platform server; `sqlite` and `store` are the local
// database. A Worker has neither: the platform provides the database and
// the network, so those modules are not compiled into it at all.
#[cfg(feature = "native")]
pub mod api;
#[cfg(feature = "native")]
pub mod client;
pub mod config;
pub mod crypto;
pub mod db;
pub mod http;
pub mod httpclient;
pub mod jwt;
pub mod platform;
#[cfg(feature = "native")]
pub mod service;
#[cfg(feature = "sqlite")]
pub mod sqlite;
#[cfg(feature = "sqlite")]
pub mod store;
pub mod wire;

#[cfg(feature = "native")]
pub use api::Api;
#[cfg(feature = "native")]
pub use client::ApiClient;
pub use config::{
    AclConfig, AppConfig, AppDoc, AppMeta, AppType, AuthConfig, CollectionDef, ColumnDef,
    ColumnType, ConfigError, Data, DatabaseConfig, DbKind, Features, Issue, LocalStorage, Meta,
    Provider, RedisConfig, S3Storage, Schema, Secrets, StorageConfig, StorageKind,
};
#[cfg(feature = "native")]
pub use db::DbServer;
pub use db::{AppDb, DbOp, DbRequest, DbResponse};
#[cfg(feature = "native")]
pub use http::{read_request, serve, serve_on, write_response, Incoming};
pub use http::{ApiError, Request, Response};
pub use httpclient::{AsyncHttpClient, HttpRequest, HttpResponse};
#[cfg(feature = "native")]
pub use httpclient::{CurlHttpClient, HttpClient, PlainHttpClient, SyncHttpClient};
#[cfg(feature = "native")]
pub use platform::FileStore;
pub use platform::{
    check_email, check_password, create_owner, iso8601, issue_token, normalize_email, now_secs,
    set_clock, verify_login, AppRecord, BoxFuture, MemoryStore, PasswordHash, Platform,
    PlatformStore, User, MIN_PASSWORD_LEN, PBKDF2_ITERATIONS, ROLE_OWNER, SESSION_TTL_SECS,
};
#[cfg(feature = "native")]
pub use service::{CustomMethod, Event, EventContext, Hook, Hooks, Listener, Phase, Service};
#[cfg(feature = "sqlite")]
pub use sqlite::{Connection, Row, Statement};
#[cfg(feature = "sqlite")]
pub use store::SqliteStore;

/// The version of the platform, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
