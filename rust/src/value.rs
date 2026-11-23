//! A tiny, dependency-free JSON value model.
//!
//! The JavaScript implementation works on plain JS values, which gives it two
//! properties that matter for SQL parity:
//!
//! * object keys keep **insertion order** (`INSERT INTO t (a, b)` must not become `(b, a)`),
//! * numbers are IEEE-754 doubles whose `String()` form is used verbatim in SQL.
//!
//! `Value` reproduces both. It also carries a `Date` variant so the Rust API can
//! express the `new Date()` values the JS builder accepts from user code.

use crate::error::{Error, Result};
use alloc::string::{String, ToString};
use alloc::vec::Vec;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    pub entries: Vec<(String, Value)>,
}

impl Object {
    pub fn new() -> Self {
        Object {
            entries: Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Remove a key, preserving the position of the others.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let idx = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(idx).1)
    }

    /// Insert or replace, keeping the original position (JS object semantics).
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn iter(&self) -> core::slice::Iter<'_, (String, Value)> {
        self.entries.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn first(&self) -> Option<&(String, Value)> {
        self.entries.first()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Object),
    /// ISO-8601 instant, e.g. `"2024-01-02T03:04:05.000Z"`.
    Date(String),
}

impl Value {
    pub fn object() -> Value {
        Value::Object(Object::new())
    }

    pub fn str(s: impl Into<String>) -> Value {
        Value::Str(s.into())
    }

    /// JS `obj[key]` — only plain objects are indexable.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(o) => o.get(key),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Value::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `isObject` from the JS source: a plain object (not an array, not null).
    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Value::Array(_))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// `typeof v === 'object'` — true for objects, arrays and null.
    pub fn is_object_like(&self) -> bool {
        matches!(
            self,
            Value::Object(_) | Value::Array(_) | Value::Null | Value::Date(_)
        )
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    /// JS truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0 && !f.is_nan(),
            Value::Str(s) => !s.is_empty(),
            Value::Array(_) | Value::Object(_) | Value::Date(_) => true,
        }
    }

    /// JS `String(value)` — used by the builder for raw interpolation.
    pub fn to_js_string(&self) -> String {
        match self {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => fmt_number(*f),
            Value::Str(s) => s.clone(),
            Value::Date(d) => d.clone(),
            Value::Array(a) => a
                .iter()
                .map(|v| match v {
                    Value::Null => String::new(),
                    other => other.to_js_string(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Value::Object(_) => "[object Object]".to_string(),
        }
    }

    /// `Number(value)` — only the coercions the builder relies on.
    pub fn to_number(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::Null => Some(0.0),
            Value::Str(s) => Some(parse_js_number(s)),
            Value::Array(a) => {
                if a.is_empty() {
                    Some(0.0)
                } else if a.len() == 1 {
                    a[0].to_number()
                } else {
                    Some(f64::NAN)
                }
            }
            Value::Object(_) | Value::Date(_) => Some(f64::NAN),
        }
    }
}

/// `Number(string)` — enough of the spec for `$limit`, `$skip`, `$mod`, ...
fn parse_js_number(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty() {
        return 0.0;
    }
    let lower = t.to_ascii_lowercase();
    match lower.as_str() {
        "infinity" | "+infinity" => return f64::INFINITY,
        "-infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    if let Some(hex) = lower.strip_prefix("0x") {
        return i64::from_str_radix(hex, 16)
            .map(|v| v as f64)
            .unwrap_or(f64::NAN);
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// Format a float exactly like JavaScript's `String(number)`.
///
/// JS switches to exponential notation at `1e21` and below `1e-6`, always keeps a
/// sign on the exponent and never emits a trailing `.0`.
pub fn fmt_number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    let a = x.abs();
    if a != 0.0 && !(1e-6..1e21).contains(&a) {
        let s = format!("{:e}", x); // "1e21", "1.5e-7", "-2.5e20"
        let (mantissa, exp) = match s.split_once('e') {
            Some((m, e)) => (m.to_string(), e.parse::<i32>().unwrap_or(0)),
            None => (s, 0),
        };
        let mantissa = match mantissa.strip_suffix(".0") {
            Some(m) => m.to_string(),
            None => mantissa,
        };
        format!("{}e{}{}", mantissa, if exp >= 0 { "+" } else { "" }, exp)
    } else {
        format!("{}", x)
    }
}

// ---------------------------------------------------------------------------
// JSON serialization (JSON.stringify compatible, compact)
// ---------------------------------------------------------------------------

pub fn to_json(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, &mut out);
    out
}

fn write_json(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Int(i) => out.push_str(&i.to_string()),
        Value::Float(f) => {
            if f.is_finite() {
                out.push_str(&fmt_number(*f));
            } else {
                out.push_str("null");
            }
        }
        Value::Str(s) => write_json_string(s, out),
        Value::Date(d) => write_json_string(d, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        Value::Object(obj) => {
            out.push('{');
            for (i, (k, v)) in obj.entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json_string(k, out);
                out.push(':');
                write_json(v, out);
            }
            out.push('}');
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ---------------------------------------------------------------------------
// JSON parsing
// ---------------------------------------------------------------------------

/// Canonical representation of a JavaScript number: integral values inside the
/// safe range become [`Value::Int`], everything else [`Value::Float`]. This
/// matches what `parse_json` produces, so a value read directly from JS
/// behaves identically to the same value parsed from JSON.
///
/// Only the Node-API converter needs it; the wasm build reads JSON instead. It
/// is gated exactly like `node_api`, its only caller, so that a default build
/// neither compiles it nor — on Windows — tries to link a bridge that is not
/// there.
#[cfg(all(not(target_arch = "wasm32"), feature = "napi"))]
pub fn from_f64(x: f64) -> Value {
    if x.is_finite() && crate::float::fract(x) == 0.0 && x.abs() < 9.007_199_254_740_992e15 {
        Value::Int(x as i64)
    } else {
        Value::Float(x)
    }
}

pub fn parse_json(input: &str) -> Result<Value> {
    let mut p = Parser {
        b: input.as_bytes(),
        i: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.i != p.b.len() {
        return Err(Error::new("Unexpected trailing characters in JSON"));
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.b.get(self.i).copied()
    }

    fn value(&mut self) -> Result<Value> {
        match self.peek() {
            None => Err(Error::new("Unexpected end of JSON input")),
            Some(b'n') => self.literal("null", Value::Null),
            Some(b't') => self.literal("true", Value::Bool(true)),
            Some(b'f') => self.literal("false", Value::Bool(false)),
            Some(b'"') => self.string().map(Value::Str),
            Some(b'[') => self.array(),
            Some(b'{') => self.object(),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(Error::new(format!("Unexpected token `{}`", c as char))),
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Result<Value> {
        if self.b[self.i..].starts_with(word.as_bytes()) {
            self.i += word.len();
            Ok(value)
        } else {
            Err(Error::new(format!("Unexpected token, expected `{}`", word)))
        }
    }

    fn string(&mut self) -> Result<String> {
        // caller guarantees current byte is '"'
        self.i += 1;
        let mut out = String::new();
        loop {
            let c = *self
                .b
                .get(self.i)
                .ok_or_else(|| Error::new("Unterminated string"))?;
            self.i += 1;
            match c {
                b'"' => return Ok(out),
                b'\\' => {
                    let e = *self
                        .b
                        .get(self.i)
                        .ok_or_else(|| Error::new("Unterminated escape"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) {
                                // surrogate pair
                                if self.b.get(self.i) == Some(&b'\\')
                                    && self.b.get(self.i + 1) == Some(&b'u')
                                {
                                    self.i += 2;
                                    let lo = self.hex4()?;
                                    if (0xDC00..0xE000).contains(&lo) {
                                        let cp = 0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00);
                                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                                        continue;
                                    }
                                }
                                out.push('\u{FFFD}');
                            } else {
                                out.push(char::from_u32(hi).unwrap_or('\u{FFFD}'));
                            }
                        }
                        other => {
                            return Err(Error::new(format!(
                                "Bad escape character `\\{}`",
                                other as char
                            )))
                        }
                    }
                }
                c if c < 0x20 => return Err(Error::new("Unescaped control character in string")),
                c if c < 0x80 => out.push(c as char),
                _ => {
                    // Multi-byte UTF-8: decode by collecting continuation bytes.
                    let start = self.i - 1;
                    let mut end = start + 1;
                    while end < self.b.len() && (self.b[end] & 0xC0) == 0x80 {
                        end += 1;
                    }
                    self.i = end;
                    match core::str::from_utf8(&self.b[start..end]) {
                        Ok(s) => out.push_str(s),
                        Err(_) => out.push('\u{FFFD}'),
                    }
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u32> {
        if self.i + 4 > self.b.len() {
            return Err(Error::new("Bad unicode escape"));
        }
        let s = core::str::from_utf8(&self.b[self.i..self.i + 4])
            .map_err(|_| Error::new("Bad unicode escape"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| Error::new("Bad unicode escape"))?;
        self.i += 4;
        Ok(v)
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.i;
        if self.peek() == Some(b'-') {
            self.i += 1;
        }
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.i += 1;
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.i += 1;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.i += 1;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.i += 1;
            }
            while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                self.i += 1;
            }
        }
        let raw =
            core::str::from_utf8(&self.b[start..self.i]).map_err(|_| Error::new("Bad number"))?;
        if !is_float {
            if let Ok(i) = raw.parse::<i64>() {
                return Ok(Value::Int(i));
            }
        }
        raw.parse::<f64>()
            .map(Value::Float)
            .map_err(|_| Error::new(format!("Bad number `{}`", raw)))
    }

    fn array(&mut self) -> Result<Value> {
        self.i += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Value::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Value::Array(items));
                }
                _ => return Err(Error::new("Expected `,` or `]` in array")),
            }
        }
    }

    fn object(&mut self) -> Result<Value> {
        self.i += 1; // '{'
        let mut obj = Object::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Value::Object(obj));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(Error::new("Expected `\"` in object"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(Error::new("Expected `:` in object"));
            }
            self.i += 1;
            self.skip_ws();
            let value = self.value()?;
            obj.set(key, value);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Value::Object(obj));
                }
                _ => return Err(Error::new("Expected `,` or `}` in object")),
            }
        }
    }
}

/// Parse an ISO-8601 instant into UTC fields.
pub fn parse_iso(s: &str) -> Option<(i32, u32, u32, u32, u32, u32, u32)> {
    let b = s.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i32> { s.get(from..to)?.parse::<i32>().ok() };
    let mut y = num(0, 4)?;
    let mut mo = num(5, 7)? as u32;
    let mut d = num(8, 10)? as u32;
    let (mut h, mut mi, mut sec, mut ms) = (0u32, 0u32, 0u32, 0u32);
    if b.len() >= 19 && (b[10] == b'T' || b[10] == b' ') {
        h = num(11, 13)? as u32;
        mi = num(14, 16)? as u32;
        sec = num(17, 19)? as u32;
        let mut i = 19;
        if b.get(i) == Some(&b'.') {
            i += 1;
            let start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            let frac = &s[start..i];
            let mut v: u32 = frac.get(..3).unwrap_or("0").parse().unwrap_or(0);
            if frac.len() < 3 {
                for _ in frac.len()..3 {
                    v *= 10;
                }
            }
            ms = v;
        }
        // timezone
        if i < b.len() {
            match b[i] {
                b'Z' | b'z' => {}
                b'+' | b'-' => {
                    let sign = if b[i] == b'-' { -1 } else { 1 };
                    let oh = s
                        .get(i + 1..i + 3)
                        .and_then(|x| x.parse::<i32>().ok())
                        .unwrap_or(0);
                    let om = if b.len() >= i + 6 {
                        s.get(i + 4..i + 6)
                            .and_then(|x| x.parse::<i32>().ok())
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    let offset_min = sign * (oh * 60 + om);
                    let total =
                        h as i64 * 3600 + mi as i64 * 60 + sec as i64 - offset_min as i64 * 60;
                    let days = total.div_euclid(86400);
                    let rem = total.rem_euclid(86400);
                    h = (rem / 3600) as u32;
                    mi = ((rem % 3600) / 60) as u32;
                    sec = (rem % 60) as u32;
                    if days != 0 {
                        let (yy, mm, dd) = shift_days(y, mo, d, days as i32);
                        y = yy;
                        mo = mm;
                        d = dd;
                    }
                }
                _ => {}
            }
        }
    }
    Some((y, mo, d, h, mi, sec, ms))
}

fn shift_days(y: i32, m: u32, d: u32, days: i32) -> (i32, u32, u32) {
    // civil_from_days / days_from_civil (Howard Hinnant's algorithm)
    fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
        let y = if m <= 2 { y - 1 } else { y } as i64;
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m as i64 + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }
    fn civil_from_days(z: i64) -> (i32, u32, u32) {
        let z = z + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if m <= 2 { y + 1 } else { y } as i32, m, d)
    }
    civil_from_days(days_from_civil(y, m, d) + days as i64)
}

/// `new Date(...).toISOString()` — normalised to UTC with millisecond precision.
pub fn to_iso_string(date: &str) -> String {
    match parse_iso(date) {
        Some((y, mo, d, h, mi, s, ms)) => format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
            y, mo, d, h, mi, s, ms
        ),
        None => date.to_string(),
    }
}

/// MySQL needs `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn to_mysql_datetime(date: &str) -> String {
    match parse_iso(date) {
        Some((y, mo, d, h, mi, s, _)) => {
            format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s)
        }
        None => date.to_string(),
    }
}
