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

pub mod config;

pub use config::{
    AclConfig, AppConfig, AppDoc, AppMeta, AppType, AuthConfig, CollectionDef, ColumnDef,
    ColumnType, ConfigError, Data, DatabaseConfig, DbKind, Features, Issue, LocalStorage, Meta,
    Provider, RedisConfig, S3Storage, Schema, Secrets, StorageConfig, StorageKind,
};

/// The version of the platform, from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
