//! The framing of the newline-delimited JSON protocols.
//!
//! A request is a JSON line, optionally preceded by header lines:
//!
//! ```text
//! authorization: Bearer s3cret
//! {"op":"findMany","model":"user"}
//! ```
//!
//! A reply is one JSON line. A header line is a name and a value, the way
//! HTTP writes them; the only one that means anything here is
//! `authorization`, which carries a bearer token so the service can tell
//! one caller from another.
//!
//! The rule for telling a header from a request: a header's name is
//! letters, digits and dashes, and it is followed by a colon. JSON starts
//! with a brace, so the two can never be confused.

use std::io::BufRead;

/// One request as it comes off the wire.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frame {
    /// The bearer token, when the request carried one.
    pub token: Option<String>,
    /// The request's JSON.
    pub json: String,
}

/// Whether a line is a header, and its name and value if it is.
fn header(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let name = &line[..colon];
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    Some((name, line[colon + 1..].trim()))
}

/// Read one frame: header lines, then the JSON line.
///
/// `None` means the connection ended.
pub fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<Frame>> {
    let mut token: Option<String> = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Ok(None);
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }
        match header(line) {
            Some((name, value)) => {
                if name.eq_ignore_ascii_case("authorization") {
                    token = Some(bearer(value));
                }
            }
            None => {
                return Ok(Some(Frame {
                    token,
                    json: line.to_string(),
                }))
            }
        }
    }
}

/// The token in an `authorization` header value.
fn bearer(value: &str) -> String {
    let value = value.trim();
    match value.split_once(' ') {
        Some((scheme, rest)) if scheme.eq_ignore_ascii_case("bearer") => rest.trim().to_string(),
        _ => value.to_string(),
    }
}

/// Write a frame: the token's header line, then the JSON line.
pub fn write_frame(
    writer: &mut impl std::io::Write,
    token: Option<&str>,
    json: &str,
) -> std::io::Result<()> {
    if let Some(token) = token {
        writer.write_all(format!("authorization: Bearer {token}\n").as_bytes())?;
    }
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Whether a token may in: `None` means the service wants no token, and
/// the caller gave one.
pub fn token_ok(wanted: Option<&str>, offered: Option<&str>) -> bool {
    match wanted {
        None => true,
        Some(wanted) => offered == Some(wanted),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    fn frame_of(text: &str) -> Option<Frame> {
        let mut reader = BufReader::new(text.as_bytes());
        read_frame(&mut reader).expect("read")
    }

    #[test]
    fn a_bare_request_is_one_line() {
        let frame = frame_of("{\"op\":\"count\"}\n").expect("a frame");
        assert_eq!(frame.json, "{\"op\":\"count\"}");
        assert_eq!(frame.token, None);
    }

    #[test]
    fn a_token_comes_in_a_header_line() {
        let frame =
            frame_of("authorization: Bearer s3cret\n{\"op\":\"count\"}\n").expect("a frame");
        assert_eq!(frame.json, "{\"op\":\"count\"}");
        assert_eq!(frame.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn the_scheme_is_matched_without_care() {
        let frame = frame_of("AUTHORIZATION: bearer s3cret\n{}\n").expect("a frame");
        assert_eq!(frame.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn a_value_without_a_scheme_is_the_token() {
        let frame = frame_of("authorization: s3cret\n{}\n").expect("a frame");
        assert_eq!(frame.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn other_headers_are_read_and_ignored() {
        let frame =
            frame_of("x-tenant: acme\nauthorization: Bearer s3cret\n{}\n").expect("a frame");
        assert_eq!(frame.json, "{}");
        assert_eq!(frame.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn a_json_line_is_not_mistaken_for_a_header() {
        let frame = frame_of("{\"op\":\"count\",\"model\":\"user\"}\n").expect("a frame");
        assert_eq!(frame.json, "{\"op\":\"count\",\"model\":\"user\"}");
        assert_eq!(frame.token, None);
    }

    #[test]
    fn the_end_of_the_connection_is_the_end_of_the_frames() {
        assert_eq!(frame_of(""), None);
        assert_eq!(frame_of("authorization: Bearer s3cret\n"), None);
    }

    #[test]
    fn a_frame_round_trips() {
        let mut out: Vec<u8> = Vec::new();
        write_frame(&mut out, Some("s3cret"), "{\"op\":\"count\"}").expect("written");
        let mut reader = BufReader::new(out.as_slice());
        let frame = read_frame(&mut reader).expect("read").expect("a frame");
        assert_eq!(frame.json, "{\"op\":\"count\"}");
        assert_eq!(frame.token.as_deref(), Some("s3cret"));
    }

    #[test]
    fn a_token_is_accepted_only_when_it_matches() {
        assert!(token_ok(None, None));
        assert!(token_ok(None, Some("s3cret")));
        assert!(token_ok(Some("s3cret"), Some("s3cret")));
        assert!(!token_ok(Some("s3cret"), Some("other")));
        assert!(!token_ok(Some("s3cret"), None));
    }
}
