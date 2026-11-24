//! Drops unreferenced code from this crate's binaries.
//!
//! SQLite arrives as an archive compiled with `-ffunction-sections`, which
//! splits it into one section per function but leaves every section in place.
//! The linker needs telling to throw the unused ones away, and the flag is
//! the linker's, not the platform's: GNU ld and lld take `--gc-sections`,
//! Apple's takes `-dead_strip`, and MSVC's discards unreferenced COMDATs on
//! its own. The GNU flag on Apple's linker is a hard error, not a warning.

fn main() {
    let target = std::env::var("TARGET").expect("TARGET");
    println!("cargo:rerun-if-changed=build.rs");
    if target.contains("msvc") {
        return;
    }
    if target.contains("apple") {
        println!("cargo:rustc-link-arg=-Wl,-dead_strip");
    } else {
        println!("cargo:rustc-link-arg=-Wl,--gc-sections");
    }
}
