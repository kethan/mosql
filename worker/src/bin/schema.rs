//! Print the database schema the engine expects.
//!
//! The Worker deployment creates its tables from this, and the file it is
//! written to is checked against it by a test, so the two cannot drift.

fn main() {
    print!("{}", better_auth_core::models::schema_sql());
}
