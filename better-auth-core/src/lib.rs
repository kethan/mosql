pub mod auth;
pub mod bigint;
pub mod crypto;
pub mod error;
pub mod http;
pub mod middleware;
pub mod models;
pub mod oauth_presets;
pub mod oidc;
pub mod query;
#[cfg(feature = "native")]
pub mod query_server;
pub mod routes;
pub mod sql;
pub mod store;

/// An async test: the body runs under `uflite::db::block_on`, since this
/// crate has no async runtime of its own. Available to integration tests
/// as `better_auth_core::async_test!`.
#[macro_export]
macro_rules! async_test {
    ($(#[$meta:meta])* async fn $name:ident() $body:block) => {
        $(#[$meta])*
        #[test]
        fn $name() {
            ::uflite::db::block_on(async $body)
        }
    };
}
