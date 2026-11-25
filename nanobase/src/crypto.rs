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

#[cfg(feature = "native")]
use std::collections::hash_map::RandomState;
#[cfg(feature = "native")]
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
///
/// The key schedule is built once and kept. The key does not change from
/// one iteration to the next, so re-padding it and re-compressing the two
/// key blocks every time would be half the work done for nothing: an
/// iteration is two compressions, not four.
pub fn pbkdf2_sha256(password: &[u8], salt: &[u8], iterations: u32, out_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(out_len);
    let (inner, outer) = hmac_sha256_key_schedule(password);
    let mut block_index: u32 = 1;
    while out.len() < out_len {
        let mut h = hmac_with(&inner, &outer, &[salt, &block_index.to_be_bytes()]);
        let mut acc = h;
        for _ in 1..iterations {
            h = hmac_with(&inner, &outer, &[&h]);
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

/// The two halves of an HMAC-SHA256 key schedule, each already compressed
/// over its padded key block.
fn hmac_sha256_key_schedule(password: &[u8]) -> (Sha256, Sha256) {
    let mut block = [0u8; 64];
    if password.len() > 64 {
        block[..32].copy_from_slice(&sha256(password));
    } else {
        block[..password.len()].copy_from_slice(password);
    }
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    for b in block.iter() {
        inner.update(&[b ^ 0x36]);
        outer.update(&[b ^ 0x5c]);
    }
    (inner, outer)
}

/// HMAC-SHA256 over a key schedule that has already been prepared.
fn hmac_with(inner: &Sha256, outer: &Sha256, parts: &[&[u8]]) -> [u8; 32] {
    let mut i = inner.clone();
    for part in parts {
        i.update(part);
    }
    let mut o = outer.clone();
    o.update(&i.finalize());
    o.finalize()
}

// ---------------------------------------------------------------- scrypt ---

/// scrypt (RFC 7914).
///
/// This is what Better Auth hashes passwords with, and the output has to
/// be byte-for-byte the same as Node's: the two stacks share a database,
/// so a hash written by one has to verify under the other.
///
/// `n` must be a power of two greater than one, and `r * p` must be
/// small enough that the working set (`128 * r * n` bytes) fits in memory.
pub fn scrypt(
    password: &[u8],
    salt: &[u8],
    n: usize,
    r: usize,
    p: usize,
    dk_len: usize,
) -> Vec<u8> {
    assert!(
        n.is_power_of_two() && n > 1,
        "scrypt needs n to be a power of two"
    );
    let words = 32 * r;
    let chunk_bytes = 128 * r;

    // Step 1: stretch the password into p blocks of 128 * r bytes.
    let mut blocks = pbkdf2_sha256(password, salt, 1, p * chunk_bytes);
    for chunk in blocks.chunks_mut(chunk_bytes) {
        let mut x = vec![0u32; words];
        for (i, word) in x.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        scrypt_romix(&mut x, n, r);
        for (i, word) in x.iter().enumerate() {
            chunk[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
    }

    // Step 3: the final stretch over the whole thing.
    pbkdf2_sha256(password, &blocks, 1, dk_len)
}

/// scryptROMix (RFC 7914 §6).
fn scrypt_romix(block: &mut [u32], n: usize, r: usize) {
    let width = 32 * r;
    let mut v = vec![0u32; width * n];
    let mut x = vec![0u32; width];
    let mut y = vec![0u32; width];
    x.copy_from_slice(block);

    // Fill the table with the chain.
    for i in 0..n {
        v[i * width..(i + 1) * width].copy_from_slice(&x);
        scrypt_block_mix(&x, &mut y, r);
        std::mem::swap(&mut x, &mut y);
    }

    // Walk the chain again, mixing in the entry the last block points at.
    for _ in 0..n {
        // Integerify: the last 64-byte block read as a little-endian
        // integer. Only the low bits matter, because n is a power of two.
        // The block is little-endian words, so its first eight bytes are
        // its first two words.
        let j = ((x[width - 16] as u64) | ((x[width - 15] as u64) << 32)) as usize & (n - 1);
        for k in 0..width {
            x[k] ^= v[j * width + k];
        }
        scrypt_block_mix(&x, &mut y, r);
        std::mem::swap(&mut x, &mut y);
    }
    block.copy_from_slice(&x);
}

/// scryptBlockMix (RFC 7914 §6).
fn scrypt_block_mix(input: &[u32], output: &mut [u32], r: usize) {
    let mut x = [0u32; 16];
    x.copy_from_slice(&input[(2 * r - 1) * 16..][..16]);
    let mut y = vec![0u32; 32 * r];
    for i in 0..2 * r {
        let block = &input[i * 16..i * 16 + 16];
        for j in 0..16 {
            x[j] ^= block[j];
        }
        salsa20_8(&mut x);
        y[i * 16..i * 16 + 16].copy_from_slice(&x);
    }
    // The even blocks come first, then the odd ones.
    for k in 0..r {
        output[k * 16..k * 16 + 16].copy_from_slice(&y[2 * k * 16..2 * k * 16 + 16]);
        output[(r + k) * 16..(r + k) * 16 + 16]
            .copy_from_slice(&y[(2 * k + 1) * 16..(2 * k + 1) * 16 + 16]);
    }
}

/// The Salsa20/8 core: eight rounds rather than twenty.
///
/// Every one of the rotations and constants is load-bearing. A wrong one
/// changes every byte of the output, so the vectors in the tests are the
/// only real check on this function.
fn salsa20_8(block: &mut [u32; 16]) {
    let mut x = *block;
    for _ in 0..4 {
        // Column round: four quarter-rounds, one per column of the
        // 4x4 state.
        x[4] ^= x[0].wrapping_add(x[12]).rotate_left(7);
        x[8] ^= x[4].wrapping_add(x[0]).rotate_left(9);
        x[12] ^= x[8].wrapping_add(x[4]).rotate_left(13);
        x[0] ^= x[12].wrapping_add(x[8]).rotate_left(18);

        x[9] ^= x[5].wrapping_add(x[1]).rotate_left(7);
        x[13] ^= x[9].wrapping_add(x[5]).rotate_left(9);
        x[1] ^= x[13].wrapping_add(x[9]).rotate_left(13);
        x[5] ^= x[1].wrapping_add(x[13]).rotate_left(18);

        x[14] ^= x[10].wrapping_add(x[6]).rotate_left(7);
        x[2] ^= x[14].wrapping_add(x[10]).rotate_left(9);
        x[6] ^= x[2].wrapping_add(x[14]).rotate_left(13);
        x[10] ^= x[6].wrapping_add(x[2]).rotate_left(18);

        x[3] ^= x[15].wrapping_add(x[11]).rotate_left(7);
        x[7] ^= x[3].wrapping_add(x[15]).rotate_left(9);
        x[11] ^= x[7].wrapping_add(x[3]).rotate_left(13);
        x[15] ^= x[11].wrapping_add(x[7]).rotate_left(18);

        // Row round.
        x[1] ^= x[0].wrapping_add(x[3]).rotate_left(7);
        x[2] ^= x[1].wrapping_add(x[0]).rotate_left(9);
        x[3] ^= x[2].wrapping_add(x[1]).rotate_left(13);
        x[0] ^= x[3].wrapping_add(x[2]).rotate_left(18);

        x[6] ^= x[5].wrapping_add(x[4]).rotate_left(7);
        x[7] ^= x[6].wrapping_add(x[5]).rotate_left(9);
        x[4] ^= x[7].wrapping_add(x[6]).rotate_left(13);
        x[5] ^= x[4].wrapping_add(x[7]).rotate_left(18);

        x[11] ^= x[10].wrapping_add(x[9]).rotate_left(7);
        x[8] ^= x[11].wrapping_add(x[10]).rotate_left(9);
        x[9] ^= x[8].wrapping_add(x[11]).rotate_left(13);
        x[10] ^= x[9].wrapping_add(x[8]).rotate_left(18);

        x[12] ^= x[15].wrapping_add(x[14]).rotate_left(7);
        x[13] ^= x[12].wrapping_add(x[15]).rotate_left(9);
        x[14] ^= x[13].wrapping_add(x[12]).rotate_left(13);
        x[15] ^= x[14].wrapping_add(x[13]).rotate_left(18);
    }
    for i in 0..16 {
        block[i] = x[i].wrapping_add(block[i]);
    }
}

/// The parameters Better Auth uses: `N=16384, r=16, p=1`, 64 bytes out.
pub const SCRYPT_N: usize = 16_384;
pub const SCRYPT_R: usize = 16;
pub const SCRYPT_P: usize = 1;
pub const SCRYPT_DK_LEN: usize = 64;

/// scrypt with the parameters Better Auth uses.
pub fn scrypt_better_auth(password: &[u8], salt: &[u8]) -> Vec<u8> {
    scrypt(password, salt, SCRYPT_N, SCRYPT_R, SCRYPT_P, SCRYPT_DK_LEN)
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
    // A host that has no random device of its own — a Worker, where the
    // only entropy is the one the platform hands over — installs a source
    // with [`set_random_source`] before anything asks for randomness.
    if let Some(source) = RANDOM.get() {
        let mut buf = vec![0u8; n];
        source(&mut buf);
        return buf;
    }
    #[cfg(feature = "native")]
    {
        os_random(n)
    }
    // Without an operating system there is no entropy to read, and a
    // program that has not installed a source has a bug rather than a slow
    // request. Predictable identifiers are worse than a loud failure.
    #[cfg(not(feature = "native"))]
    {
        panic!("no random source installed: call nanobase::crypto::set_random_source first")
    }
}

/// A source of random bytes, given a buffer to fill.
pub type RandomSource = fn(&mut [u8]);

static RANDOM: std::sync::OnceLock<RandomSource> = std::sync::OnceLock::new();

/// Install the source the platform draws randomness from.
///
/// The first call wins, so two halves of a program cannot disagree about
/// where the entropy comes from.
pub fn set_random_source(source: RandomSource) {
    let _ = RANDOM.set(source);
}

/// Whether a random source has been installed.
pub fn has_random_source() -> bool {
    RANDOM.get().is_some()
}

/// The operating system's random bytes.
#[cfg(feature = "native")]
fn os_random(n: usize) -> Vec<u8> {
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

/// The last resort when there is no random device: the standard library's
/// per-process random hash keys, mixed with a counter, the clock and the
/// process id.
#[cfg(feature = "native")]
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

    #[test]
    fn pbkdf2_known_answer() {
        // RFC 7914 §11's PBKDF2-HMAC-SHA256 vector.
        let out = pbkdf2_sha256(b"passwd", b"salt", 1, 64);
        let hex: String = out.iter().map(|x| format!("{x:02x}")).collect();
        assert_eq!(
            hex,
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc\
             49ca9cccf179b645991664b39d77ef317c71b845b1e30bd509112041d3a19783"
        );
        // And the same, with more than one iteration.
        let out = pbkdf2_sha256(b"Password", b"NaCl", 80000, 64);
        let hex: String = out.iter().map(|x| format!("{x:02x}")).collect();
        assert_eq!(
            hex,
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56\
             a1d425a1225833549adb841b51c9b3176a272bdebba1d078478f62b397f33c8d"
        );
    }

    /// The vectors Node's `crypto.scrypt` produces, which is what Better
    /// Auth writes. If these drift, the two stacks stop sharing a
    /// database.
    #[test]
    fn scrypt_matches_node() {
        let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
        let salt = |s: &str| -> Vec<u8> {
            (0..s.len() / 2)
                .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
                .collect()
        };

        assert_eq!(
            hex(&scrypt(
                b"",
                &salt("00000000000000000000000000000000"),
                16,
                16,
                1,
                64
            )),
            "c036e168676b6a8cf5ed2befc24876d475be8e54881ed75c1a4acf19c3db2344\
             7c433573aa9a303328370227d2d4826752a16592c8185f00b167af1a0bad0c5c"
        );
        assert_eq!(
            hex(&scrypt(
                b"password",
                &salt("00000000000000000000000000000000"),
                16,
                16,
                1,
                64
            )),
            "921ff013407d09c34f5db2226ca143a5bc31786bd30610e089694070022266c1\
             55bde1bb4db1019f4936e11fecd5a65e599ccad56d753fa7942d58994fe1b7c1"
        );
        assert_eq!(
            hex(&scrypt(
                b"a good long password",
                &salt("0123456789abcdef0123456789abcdef"),
                16,
                16,
                1,
                64
            )),
            "ee7dd3cace1bf9928979670a18f63b1de9792746e7f16aced26445fea53b5f8\
             0174455f5ca5bf99b8e0d1a977c86ef584402114de3df26a0194e093c53924a64"
        );
        assert_eq!(
            hex(&scrypt(
                b"pleaseletmein",
                &salt("536f6469756d43686c6f7269646500"),
                16,
                8,
                1,
                64
            )),
            "f2cbf7e7afc42a4aaa826c18df468e363f415161bf75ab033c97b78dbb5e635e9\
             6c74d7d654650afe02fe7b579f78fd9852559a7d0ba6653d8ca4d293a2252d7"
        );
    }

    /// How long a real hash takes. Run with `--release -- --ignored`.
    #[test]
    #[ignore]
    fn scrypt_speed() {
        let t0 = std::time::Instant::now();
        let out = scrypt_better_auth(b"a good long password", b"0123456789abcdef");
        let took = t0.elapsed();
        println!("scrypt N=16384 r=16 p=1: {:?} ({} bytes)", took, out.len());
        assert_eq!(out.len(), 64);
    }

    /// RFC 7914 §12's vector, for the parameters the RFC uses.
    #[test]
    fn scrypt_matches_rfc() {
        let hex = |b: &[u8]| -> String { b.iter().map(|x| format!("{x:02x}")).collect() };
        let out = scrypt(b"", b"", 16, 1, 1, 64);
        assert_eq!(
            hex(&out),
            "77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442\
             fcd0069ded0948f8326a753a0fc81f17e8d3e0fb2e0d3628cf35e20c38d18906"
        );
    }
}
