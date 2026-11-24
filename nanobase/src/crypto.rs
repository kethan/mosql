//! Hashing, key derivation and randomness, with no dependency to provide them.
//!
//! `nanobase` is built where crates.io is unreachable, so the primitives a
//! password and a session token need are here rather than in a crate. They are
//! standard constructions — SHA-256 (FIPS 180-4), HMAC (RFC 2104), PBKDF2
//! (RFC 8018) — implemented against the published test vectors in
//! `tests/crypto.rs`, not invented for this project.
//!
//! What this buys: a password is never stored, a session token cannot be
//! forged without the secret, and comparing a hash does not leak how much of
//! it matched.
//!
//! What it does not buy: this is not audited cryptography. If `nanobase` grows
//! a dependency budget, `argon2` and `getrandom` are the first things to
//! replace here — PBKDF2 is the weakest of the memory-hard options and only
//! remains defensible because the iteration count is high and the salt is
//! random per password.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

// -------------------------------------------------------------- SHA-256 ---

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Streaming SHA-256. [`Sha256::update`] takes any number of slices, so HMAC
/// never has to build an intermediate buffer.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    len: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256 {
            state: H0,
            buf: [0; 64],
            buf_len: 0,
            len: 0,
        }
    }
}

impl Sha256 {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.len = self.len.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let take = (64 - self.buf_len).min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                compress(&mut self.state, &block);
                self.buf_len = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            compress(&mut self.state, &block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finalize(mut self) -> [u8; 32] {
        // Length is captured before padding; `update` keeps counting, but
        // nothing reads the counter again.
        let bit_len = self.len.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buf_len != 56 {
            self.update(&[0]);
        }
        self.update(&bit_len.to_be_bytes());
        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

/// SHA-256 of one slice, for when a stream is not needed.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finalize()
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for (i, word) in w.iter_mut().take(16).enumerate() {
        *word = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let mut h = *state;
    for i in 0..64 {
        let s1 = h[4].rotate_right(6) ^ h[4].rotate_right(11) ^ h[4].rotate_right(25);
        let ch = (h[4] & h[5]) ^ ((!h[4]) & h[6]);
        let t1 = h[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = h[0].rotate_right(2) ^ h[0].rotate_right(13) ^ h[0].rotate_right(22);
        let maj = (h[0] & h[1]) ^ (h[0] & h[2]) ^ (h[1] & h[2]);
        let t2 = s0.wrapping_add(maj);
        h[7] = h[6];
        h[6] = h[5];
        h[5] = h[4];
        h[4] = h[3].wrapping_add(t1);
        h[3] = h[2];
        h[2] = h[1];
        h[1] = h[0];
        h[0] = t1.wrapping_add(t2);
    }
    for (s, v) in state.iter_mut().zip(h) {
        *s = s.wrapping_add(v);
    }
}

// ----------------------------------------------------------------- HMAC ---

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }

    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    for b in block.iter() {
        inner.update(&[b ^ 0x36]);
        outer.update(&[b ^ 0x5c]);
    }
    inner.update(data);
    outer.update(&inner.finalize());
    outer.finalize()
}

/// HMAC over several slices, without concatenating them.
pub fn hmac_sha256_parts(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&sha256(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    for b in block.iter() {
        inner.update(&[b ^ 0x36]);
        outer.update(&[b ^ 0x5c]);
    }
    for part in parts {
        inner.update(part);
    }
    outer.update(&inner.finalize());
    outer.finalize()
}

// --------------------------------------------------------------- PBKDF2 ---

/// PBKDF2-HMAC-SHA256 (RFC 8018 §5.2).
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let mut block_index: u32 = 1;
    while out.len() < out_len {
        let mut h = hmac_sha256_parts(password, &[salt, &block_index.to_be_bytes()]);
        let mut acc = h;
        for _ in 1..iterations {
            h = hmac_sha256(password, &h);
            for (a, b) in acc.iter_mut().zip(h) {
                *a ^= b;
            }
        }
        out.extend_from_slice(&acc);
        block_index = block_index.wrapping_add(1);
    }
    out.truncate(out_len);
    out
}

// ------------------------------------------------------ constant-time eq ---

/// `a == b` without branching on the first differing byte.
///
/// A normal comparison returns as soon as it finds a mismatch, which makes the
/// time taken depend on how much of the guess was right. That is enough to
/// recover a hash a byte at a time.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

// ------------------------------------------------------------- base64url ---

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// URL-safe base64 without padding — the form a JWT uses, and safe in a query
/// string or a header.
pub fn b64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(B64URL[(n >> 18) as usize & 63] as char);
        out.push(B64URL[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[n as usize & 63] as char);
        }
    }
    out
}

fn b64url_value(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

/// Inverse of [`b64url_encode`]. Padding is accepted and ignored.
pub fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let bytes: Vec<u8> = input
        .bytes()
        .filter(|c| *c != b'=')
        .map(b64url_value)
        .collect::<Option<Vec<u8>>>()?;
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        match chunk.len() {
            4 => {
                let n = (u32::from(chunk[0]) << 18)
                    | (u32::from(chunk[1]) << 12)
                    | (u32::from(chunk[2]) << 6)
                    | u32::from(chunk[3]);
                out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8, n as u8]);
            }
            3 => {
                let n = (u32::from(chunk[0]) << 18)
                    | (u32::from(chunk[1]) << 12)
                    | (u32::from(chunk[2]) << 6);
                out.extend_from_slice(&[(n >> 16) as u8, (n >> 8) as u8]);
            }
            2 => {
                let n = (u32::from(chunk[0]) << 18) | (u32::from(chunk[1]) << 12);
                out.push((n >> 16) as u8);
            }
            _ => return None,
        }
    }
    Some(out)
}

// ------------------------------------------------------------ randomness ---

/// `n` bytes of randomness for salts and tokens.
///
/// Tries the operating system's random device first. Where there is none
/// (Windows has no `/dev/urandom`), falls back to mixing the standard
/// library's per-process random hash keys with a counter, the clock and the
/// process id. `RandomState` is seeded by the OS, so the fallback is not
/// predictable from outside the process — but it is a fallback, and
/// `getrandom` is the right replacement for it.
pub fn random_bytes(n: usize) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    // `/dev/urandom` is an endless stream, so it has to be read with an exact
    // length: `fs::read` would consume it forever.
    let mut buf = vec![0u8; n];
    if let Ok(mut file) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        if file.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    fallback_random(n)
}

fn fallback_random(n: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    let pid = std::process::id() as u64;
    let mut counter = 0u64;
    while out.len() < n {
        let state = RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(counter);
        hasher.write_u64(pid);
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        );
        out.extend_from_slice(&hasher.finish().to_le_bytes());
        counter += 1;
    }
    out.truncate(n);
    out
}

/// A random id in the form `u` + 32 hex characters.
pub fn random_id() -> String {
    let bytes = random_bytes(16);
    let mut s = String::with_capacity(33);
    s.push('u');
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_answers() {
        let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_streams_identically() {
        let mut h = Sha256::new();
        h.update(b"ab");
        h.update(b"c");
        assert_eq!(h.finalize(), sha256(b"abc"));
    }

    #[test]
    fn hmac_known_answer() {
        let out = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        let hex: String = out.iter().map(|x| format!("{x:02x}")).collect();
        assert_eq!(
            hex,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
