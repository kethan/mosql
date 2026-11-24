//! The primitives, checked against their published test vectors rather than
//! against themselves. A hash that agrees with its own output proves nothing.

use nanobase::crypto::*;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// -------------------------------------------------------------- SHA-256 ---

#[test]
fn sha256_fips_180_4_vectors() {
    assert_eq!(
        hex(&sha256(b"")),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        hex(&sha256(b"abc")),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&sha256(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        )),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    // One million 'a' characters: the vector that catches a length-encoding
    // bug, because it is the only common one that crosses many blocks.
    let mut h = Sha256::new();
    let chunk = [b'a'; 1000];
    for _ in 0..1000 {
        h.update(&chunk);
    }
    assert_eq!(
        hex(&h.finalize()),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn sha256_streaming_matches_one_shot_at_every_boundary() {
    let data: Vec<u8> = (0u8..=200).collect();
    for split in 0..=data.len() {
        let mut h = Sha256::new();
        h.update(&data[..split]);
        h.update(&data[split..]);
        assert_eq!(h.finalize(), sha256(&data), "split at {split}");
    }
}

#[test]
fn sha256_takes_many_small_updates() {
    let data: Vec<u8> = (0u8..=150).collect();
    let mut h = Sha256::new();
    for b in &data {
        h.update(&[*b]);
    }
    assert_eq!(h.finalize(), sha256(&data));
}

// ----------------------------------------------------------------- HMAC ---

#[test]
fn hmac_rfc_4231_vectors() {
    let cases: &[(&[u8], &[u8], &str)] = &[
        (
            &[0x0b; 20],
            b"Hi There",
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
        ),
        (
            b"Jefe",
            b"what do ya want for nothing?",
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
        ),
        (
            &[0xaa; 20],
            &[0xdd; 50],
            "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
        ),
    ];
    for (key, data, want) in cases {
        assert_eq!(hex(&hmac_sha256(key, data)), *want, "key {key:?}");
    }
}

#[test]
fn hmac_hashes_a_key_longer_than_its_block() {
    // RFC 4231 test case 6: a 131-byte key must be hashed before use, or the
    // result is wrong in a way that only shows up for long keys.
    let key = [0xaa; 131];
    let want = "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54";
    assert_eq!(
        hex(&hmac_sha256(
            &key,
            b"Test Using Larger Than Block-Size Key - Hash Key First"
        )),
        want
    );
}

#[test]
fn hmac_parts_matches_the_single_slice_form() {
    let key = b"a different key";
    let data = b"hello world, in pieces";
    let (a, b) = data.split_at(9);
    assert_eq!(hmac_sha256_parts(key, &[a, b]), hmac_sha256(key, data));
}

// --------------------------------------------------------------- PBKDF2 ---

#[test]
fn pbkdf2_rfc_7914_vectors() {
    let cases: &[(u32, &str)] = &[
        (
            1,
            "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b",
        ),
        (
            2,
            "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43",
        ),
        (
            4096,
            "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a",
        ),
    ];
    for (iterations, want) in cases {
        assert_eq!(
            hex(&pbkdf2_sha256(b"password", b"salt", *iterations, 32)),
            *want,
            "{iterations} iterations"
        );
    }
}

#[test]
fn pbkdf2_produces_the_requested_length_across_blocks() {
    // A 64-byte output needs two blocks; the second must not repeat the first.
    let out = pbkdf2_sha256(b"pw", b"salt", 3, 64);
    assert_eq!(out.len(), 64);
    assert_ne!(&out[..32], &out[32..]);
    // And the first block is what the single-block call returns.
    assert_eq!(&out[..32], &pbkdf2_sha256(b"pw", b"salt", 3, 32));
}

// ------------------------------------------------------ constant-time eq ---

#[test]
fn ct_eq_compares_contents_and_lengths() {
    assert!(ct_eq(b"same", b"same"));
    assert!(!ct_eq(b"same", b"dame"));
    assert!(!ct_eq(b"short", b"shorter"));
    assert!(ct_eq(b"", b""));
    assert!(!ct_eq(b"", b"a"));
}

// ------------------------------------------------------------ base64url ---

#[test]
fn base64url_known_values() {
    let cases = [
        ("", ""),
        ("f", "Zg"),
        ("fo", "Zm8"),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg"),
        ("fooba", "Zm9vYmE"),
        ("foobar", "Zm9vYmFy"),
    ];
    for (plain, encoded) in cases {
        assert_eq!(b64url_encode(plain.as_bytes()), encoded, "{plain:?}");
        assert_eq!(b64url_decode(encoded).as_deref(), Some(plain.as_bytes()));
    }
}

#[test]
fn base64url_uses_the_url_safe_alphabet() {
    // 0xfb and 0xff would be '+' and '/' in standard base64.
    let all = (0u8..=255).collect::<Vec<u8>>();
    let encoded = b64url_encode(&all);
    assert!(!encoded.contains('+'), "{encoded}");
    assert!(!encoded.contains('/'), "{encoded}");
    assert!(!encoded.contains('='), "no padding: {encoded}");
    assert_eq!(b64url_decode(&encoded).as_deref(), Some(all.as_slice()));
}

#[test]
fn base64url_decode_accepts_padding_and_rejects_junk() {
    assert_eq!(b64url_decode("Zm8=").as_deref(), Some(b"fo".as_slice()));
    assert_eq!(
        b64url_decode("Zm9vYmE=").as_deref(),
        Some(b"fooba".as_slice())
    );
    // Two padding characters mean one byte in the last quantum, so this is
    // four bytes and not five.
    assert_eq!(
        b64url_decode("Zm9vYQ==").as_deref(),
        Some(b"fooa".as_slice())
    );
    assert!(b64url_decode("Zm9v!").is_none(), "invalid character");
    assert!(b64url_decode("Zm9vY").is_none(), "1 leftover character");
}

#[test]
fn base64url_round_trips_random_bytes() {
    for len in 0..80 {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 7 % 251) as u8).collect();
        let encoded = b64url_encode(&bytes);
        assert_eq!(b64url_decode(&encoded).as_deref(), Some(bytes.as_slice()));
    }
}

// ------------------------------------------------------------ randomness ---

#[test]
fn random_bytes_returns_the_requested_length() {
    assert!(random_bytes(0).is_empty());
    for n in [1, 2, 15, 16, 31, 32, 64, 1000] {
        assert_eq!(random_bytes(n).len(), n, "n = {n}");
    }
}

#[test]
fn random_bytes_do_not_repeat() {
    let a = random_bytes(32);
    let b = random_bytes(32);
    assert_ne!(a, b, "two calls produced the same bytes");
    // And they are not all one value, which is what a broken source would give.
    assert!(a.iter().any(|x| *x != a[0]));
}

#[test]
fn random_ids_are_unique_and_shaped() {
    let ids: Vec<String> = (0..50).map(|_| random_id()).collect();
    for id in &ids {
        assert!(id.starts_with('u'), "{id}");
        assert_eq!(id.len(), 33, "u + 32 hex characters");
        assert!(id[1..].chars().all(|c| c.is_ascii_hexdigit()), "{id}");
    }
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "ids collided");
}
