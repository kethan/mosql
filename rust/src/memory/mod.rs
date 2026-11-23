//! In-memory MongoDB-style engine — a port of `umosql/memory`.
//!
//! ```no_run
//! use mosql::memory::{Collection, IdStrategy};
//! use mosql::Value;
//!
//! let mut users = Collection::new("users", vec![], Default::default());
//! users.insert_one(mosql::parse_json(r#"{"name":"Ada","age":36}"#).unwrap()).unwrap();
//! let found = users
//!     .find(&mosql::parse_json(r#"{"age":{"$gte":18}}"#).unwrap())
//!     .sort(&mosql::parse_json(r#"{"age":-1}"#).unwrap())
//!     .to_vec()
//!     .unwrap();
//! assert_eq!(found.len(), 1);
//! ```

pub mod regex;

mod collection;
mod eval;
mod stages;
mod update;

pub use collection::{
    Collection, CollectionOptions, Database, DeleteResult, IdStrategy, MemFind, Stats, UpdateResult,
};
pub use eval::civil_from_days;
pub use eval::{deep_equals, del_path, eval_expr, get_path, matches, set_path};
pub use stages::{aggregate, find_projection};
pub use update::apply_update;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::value::Value;

// `AtomicUsize`, not `AtomicU64`: 32-bit bare-metal targets (Cortex-M,
// RISC-V-32) have no 64-bit atomics, so `AtomicU64` does not exist there and
// the crate would not compile. A counter only has to be strictly increasing;
// on a 32-bit target it wraps after 2^32 calls, which a fake clock never sees.
static TICK: AtomicUsize = AtomicUsize::new(0);

/// A monotonic fake clock: `no_std` has no wall clock, so `$currentDate`
/// produces a deterministic, strictly increasing timestamp.
pub(crate) fn tick_iso() -> String {
    let n = TICK.fetch_add(1, Ordering::Relaxed) as u64;
    let ms = 1_704_067_200_000i64 + n as i64; // 2024-01-01T00:00:00Z
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    let (y, mo, d) = eval::civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        mo,
        d,
        rem / 3_600_000,
        (rem % 3_600_000) / 60_000,
        (rem % 60_000) / 1_000,
        rem % 1_000
    )
}

/// `db(name)` — an isolated set of collections.
pub fn db(name: &str) -> Database {
    Database::new(name)
}

/// `collection(name, data, options)` — a single in-memory collection.
pub fn collection(name: &str, data: Vec<Value>, options: CollectionOptions) -> Collection {
    Collection::new(name, data, options)
}

/// Convenience: parse a JSON query document.
pub fn json(src: &str) -> crate::value::Value {
    crate::parse_json(src).expect("invalid JSON")
}
