//! Base64, hex and percent-decoding — written here, because a connection
//! URL has all three in it and the crate has no dependencies.
//!
//! * **base64** (RFC 4648, the standard alphabet) is how a Turso blob
//!   travels and how a MySQL `Basic` credential is built.
//! * **hex** is how a Postgres `bytea` in its `\x…` form is read.
//! * **percent-decoding** is how a password with an `@` in it survives
//!   the trip through a URL.

use alloc::string::String;
use alloc::vec::Vec;

use crate::driver::DbError;

/// The standard base64 alphabet.
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as base64, with padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    // Three bytes in, four characters out.
    let mut index = 0usize;
    while index + 3 <= data.len() {
        let n = (u32::from(data[index]) << 16)
            | (u32::from(data[index + 1]) << 8)
            | u32::from(data[index + 2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        out.push(ALPHABET[n as usize & 63] as char);
        index += 3;
    }
    let rest = &data[index..];
    match rest.len() {
        1 => {
            let n = u32::from(rest[0]) << 16;
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let n = (u32::from(rest[0]) << 16) | (u32::from(rest[1]) << 8);
            out.push(ALPHABET[(n >> 18) as usize & 63] as char);
            out.push(ALPHABET[(n >> 12) as usize & 63] as char);
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

/// Decode base64. Whitespace is skipped; anything else that is not in
/// the alphabet is an error.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, DbError> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte.is_ascii_whitespace() || byte == b'=' {
            continue;
        }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => {
                return Err(DbError::new(format!(
                    "the text is not base64: {:?} is not in the alphabet",
                    byte as char
                )))
            }
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    Ok(out)
}

/// Encode bytes as lowercase hex.
pub fn hex_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len() * 2);
    for byte in data {
        out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    out
}

/// Decode hex. An odd number of digits is an error.
pub fn hex_decode(input: &str) -> Result<Vec<u8>, DbError> {
    let bytes = input.as_bytes();
    if !bytes.len().is_multiple_of(2) {
        return Err(DbError::new(
            "the text is not hex: it has an odd number of digits",
        ));
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    // Two digits in, one byte out.
    let mut index = 0usize;
    while index + 1 < bytes.len() {
        let high = digit(bytes[index])?;
        let low = digit(bytes[index + 1])?;
        out.push((high << 4) | low);
        index += 2;
    }
    Ok(out)
}

/// One hex digit.
fn digit(byte: u8) -> Result<u8, DbError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(DbError::new(format!(
            "the text is not hex: {:?} is not a hex digit",
            byte as char
        ))),
    }
}

/// Percent-decode a URL component.
///
/// A `+` is **not** a space here: this decodes a *path or userinfo*
/// component, where `+` means a plus sign. A query string is decoded by
/// the caller, which knows about `+`.
pub fn percent_decode(input: &str) -> Result<String, DbError> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return Err(DbError::new(
                        "the URL ends in the middle of an escape sequence",
                    ));
                }
                let high = digit(bytes[i + 1])?;
                let low = digit(bytes[i + 2])?;
                out.push((high << 4) | low);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| DbError::new("the URL holds text that is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_known_answers() {
        // RFC 4648's own examples.
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_round_trips() {
        for size in 0..40usize {
            let data: Vec<u8> = (0..size).map(|i| (i * 7 + 3) as u8).collect();
            let encoded = base64_encode(&data);
            assert_eq!(base64_decode(&encoded).unwrap(), data, "size {size}");
        }
    }

    #[test]
    fn base64_refuses_what_is_not_base64() {
        assert!(base64_decode("not base64!").is_err());
    }

    #[test]
    fn hex_matches_the_known_answers() {
        assert_eq!(hex_encode(b""), "");
        assert_eq!(hex_encode(b"f"), "66");
        assert_eq!(hex_encode(b"fo"), "666f");
        assert_eq!(hex_encode(b"foo"), "666f6f");
    }

    #[test]
    fn hex_round_trips() {
        let data = vec![0u8, 1, 15, 16, 255, 128];
        assert_eq!(hex_decode(&hex_encode(&data)).unwrap(), data);
    }

    #[test]
    fn hex_refuses_an_odd_number_of_digits() {
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
    }

    #[test]
    fn percent_decoding_unescapes() {
        assert_eq!(percent_decode("hello").unwrap(), "hello");
        assert_eq!(percent_decode("a%40b").unwrap(), "a@b");
        assert_eq!(percent_decode("a%2Fb").unwrap(), "a/b");
        assert_eq!(percent_decode("a%3Ab").unwrap(), "a:b");
        assert_eq!(percent_decode("a+b").unwrap(), "a+b", "a plus is a plus");
    }

    #[test]
    fn percent_decoding_refuses_a_broken_escape() {
        assert!(percent_decode("a%").is_err());
        assert!(percent_decode("a%4").is_err());
        assert!(percent_decode("a%zz").is_err());
    }
}
