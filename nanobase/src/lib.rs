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

pub mod api;
pub mod client;
pub mod config;
pub mod crypto;
pub mod db;
pub mod http;
pub mod httpclient;
pub mod jwt;
pub mod platform;
pub mod service;
pub mod sqlite;
pub mod store;
pub mod wire;

pub use api::Api;
pub use client::ApiClient;
pub use config::{
    AclConfig, AppConfig, AppDoc, AppMeta, AppType, AuthConfig, CollectionDef, ColumnDef,
    ColumnType, ConfigError, Data, DatabaseConfig, DbKind, Features, Issue, LocalStorage, Meta,
    Provider, RedisConfig, S3Storage, Schema, Secrets, StorageConfig, StorageKind,
};
pub use db::{AppDb, DbOp, DbRequest, DbResponse, DbServer};
pub use http::{ApiError, Incoming, Request, Response};
pub use httpclient::{CurlHttpClient, HttpClient, HttpRequest, HttpResponse, PlainHttpClient};
pub use platform::{
    check_email, check_password, create_owner, iso8601, issue_token, normalize_email, now_secs,
    verify_login, AppRecord, BoxFuture, FileStore, MemoryStore, PasswordHash, Platform,
    PlatformStore, User, MIN_PASSWORD_LEN, PBKDF2_ITERATIONS, ROLE_OWNER, SESSION_TTL_SECS,
};
pub use service::{CustomMethod, Event, EventContext, Hook, Hooks, Listener, Phase, Service};
pub use sqlite::{Connection, Row, Statement};
pub use store::SqliteStore;

/// The version of the platform, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
