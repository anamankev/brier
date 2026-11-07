//! Canonical JSON after RFC 8785 (JSON Canonicalization Scheme), for the values rulebooks use.
//!
//! Object keys are sorted by their UTF-16 code units, there is no whitespace, and strings are
//! escaped as ECMAScript's `JSON.stringify` does: `"` and `\` with a backslash, U+0008, U+0009,
//! U+000A, U+000C, and U+000D as `\b \t \n \f \r`, other characters below U+0020 as `\u00xx`
//! with lowercase hex, and everything else literally. Numbers are limited to integers of at
//! most 2^53 − 1 in magnitude, which every JSON implementation represents exactly; decimals in
//! rulebooks are strings. For these inputs the output is exactly RFC 8785's. The Python twin
//! (`brier_services.rules.canonical`) is held to the same bytes by shared test vectors.

use std::fmt::Write as _;

use serde_json::Value;

/// The largest integer magnitude allowed: 2^53 − 1.
pub const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Why a value has no canonical form here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// A number with a fraction or exponent, or an integer beyond ±(2^53 − 1).
    Number(String),
}

/// The canonical JSON text of `value`.
///
/// # Errors
/// [`CanonicalError::Number`] for a non-integer or out-of-range number.
pub fn canonical(value: &Value) -> Result<String, CanonicalError> {
    let mut out = String::new();
    write_value(&mut out, value)?;
    Ok(out)
}

fn write_value(out: &mut String, value: &Value) -> Result<(), CanonicalError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            let safe = n
                .as_u64()
                .filter(|v| *v <= MAX_SAFE_INTEGER)
                .map(|v| v.to_string())
                .or_else(|| {
                    n.as_i64()
                        .filter(|v| v.unsigned_abs() <= MAX_SAFE_INTEGER)
                        .map(|v| v.to_string())
                });
            out.push_str(&safe.ok_or_else(|| CanonicalError::Number(n.to_string()))?);
        }
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                if let Some(v) = map.get(key) {
                    write_value(out, v)?;
                }
            }
            out.push('}');
        }
    }
    Ok(())
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if u32::from(c) < 0x20 => {
                // Writing to a String cannot fail.
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn rfc_8785_ordering_and_escaping() {
        // Keys sort by UTF-16 code units: U+20AC (one unit) sorts before U+1F600 (a surrogate
        // pair starting 0xD83D), though its scalar value is smaller in UTF-32 too; and "\r"
        // sorts first. Example adapted from RFC 8785 §3.2.3.
        let v = json!({"\u{20ac}": 1, "\r": 2, "\u{1f600}": 3, "1": 4, "\u{80}": 5, "ö": 6});
        assert_eq!(
            canonical(&v).unwrap(),
            "{\"\\r\":2,\"1\":4,\"\u{80}\":5,\"ö\":6,\"\u{20ac}\":1,\"\u{1f600}\":3}"
        );
        let s = json!("a\"b\\c\u{8}\t\n\u{c}\r\u{1}\u{1f}\u{7f}/\u{2028}");
        assert_eq!(
            canonical(&s).unwrap(),
            "\"a\\\"b\\\\c\\b\\t\\n\\f\\r\\u0001\\u001f\u{7f}/\u{2028}\""
        );
    }

    #[test]
    fn numbers_must_be_safe_integers() {
        assert_eq!(
            canonical(&json!([0, -1, 9_007_199_254_740_991_u64])).unwrap(),
            "[0,-1,9007199254740991]"
        );
        assert!(canonical(&json!(9_007_199_254_740_992_u64)).is_err());
        assert!(canonical(&json!(-9_007_199_254_740_992_i64)).is_err());
        assert!(canonical(&json!(1.5)).is_err());
    }
}
