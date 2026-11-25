//! The two things a Worker cannot do for itself.
//!
//! A Worker has no clock of its own — `Date.now()` is the only time there
//! is — and no entropy of its own beyond `crypto.getRandomValues()`. Both
//! are reached through two imports the host provides, under the module name
//! `host`:
//!
//! * `host_now_ms() -> f64` — milliseconds since the Unix epoch.
//! * `host_random(ptr: *mut u8, len: usize)` — fill `len` bytes at `ptr`.
//!
//! The platform crate asks for the time and for randomness through hooks
//! ([`nanobase::platform::set_clock`] and
//! [`nanobase::crypto::set_random_source`]), so the engine never has to
//! know where either came from. This module is the Worker half of that:
//! it points the hooks at the host's imports.

// The host's clock and randomness.
#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "host")]
extern "C" {
    /// Milliseconds since the Unix epoch.
    fn host_now_ms() -> f64;
    /// Fill `len` bytes at `ptr` with random bytes.
    fn host_random(ptr: *mut u8, len: usize);
}

/// Milliseconds since the Unix epoch, as the host counts them.
pub fn now_ms() -> f64 {
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: the import takes no arguments and returns a number.
        unsafe { host_now_ms() }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native_now_ms()
    }
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> i64 {
    let millis = now_ms();
    if millis.is_finite() && millis > 0.0 {
        (millis / 1000.0).floor() as i64
    } else {
        0
    }
}

/// Fill `buf` with random bytes from the host.
pub fn random_bytes(buf: &mut [u8]) {
    if buf.is_empty() {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    {
        // SAFETY: the pointer and the length describe the whole buffer, and
        // the host writes exactly that many bytes into it.
        unsafe { host_random(buf.as_mut_ptr(), buf.len()) }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        native_random(buf)
    }
}

/// Point the platform crate's clock and random hooks at the host.
///
/// The first call wins, so this is safe to call from every request: the
/// hooks are installed once and stay installed.
pub fn install() {
    nanobase::platform::set_clock(now_secs);
    nanobase::crypto::set_random_source(random_bytes);
}

// --------------------------------------------------------------- native ---

/// The native build has no host, so it uses the operating system's clock.
#[cfg(not(target_arch = "wasm32"))]
fn native_now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// The native build has no host, so it reads the operating system's random
/// device.
#[cfg(not(target_arch = "wasm32"))]
fn native_random(buf: &mut [u8]) {
    use std::io::Read;
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        if file.read_exact(buf).is_ok() {
            return;
        }
    }
    // A last resort, so a test never sees a run of zeroes: the standard
    // library's per-process hash keys, mixed with the clock.
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(native_now_ms() as u64);
    let seed = hasher.finish();
    let mut x = seed | 1;
    for byte in buf.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *byte = (x & 0xff) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_the_epoch() {
        assert!(now_secs() > 1_700_000_000, "{}", now_secs());
    }

    #[test]
    fn the_random_source_fills_the_buffer() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        random_bytes(&mut a);
        random_bytes(&mut b);
        assert_ne!(a, b, "two draws of 32 bytes should not be equal");
        assert!(
            a.iter().any(|byte| *byte != 0),
            "the buffer should not be all zeroes"
        );
    }
}
