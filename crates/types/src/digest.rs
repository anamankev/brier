//! A SHA-256 digest that the exchange records but never computes or interprets (G5 plan).
//!
//! `CreateMarket` carries the digest of the market's rulebook and `Settle` the digest of the
//! evidence that settled it, so the journal alone pins both. The engine passes them through to
//! its events and keeps nothing; who computed them, and from what, is the gateway's business.

use std::fmt;

/// 32 bytes, written on the wire as 64 lowercase hex digits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// The digest with these bytes.
    #[must_use]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The bytes.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Parse exactly 64 lowercase hex digits. Uppercase is refused, so a digest has one
    /// spelling on the wire.
    #[must_use]
    pub fn from_hex(s: &str) -> Option<Self> {
        let raw = s.as_bytes();
        if raw.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        let (pairs, _) = raw.as_chunks::<2>();
        for (byte, &[hi, lo]) in out.iter_mut().zip(pairs) {
            *byte = nibble(hi)?.checked_mul(16)?.checked_add(nibble(lo)?)?;
        }
        Some(Self(out))
    }

    /// The 64 lowercase hex digits.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.to_string()
    }
}

const fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => c.checked_sub(b'0'),
        b'a'..=b'f' => match c.checked_sub(b'a') {
            Some(v) => v.checked_add(10),
            None => None,
        },
        _ => None,
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({self})")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::Digest;

    proptest! {
        #[test]
        fn hex_round_trips(bytes in any::<[u8; 32]>()) {
            let d = Digest::new(bytes);
            let hex = d.to_hex();
            prop_assert_eq!(hex.len(), 64);
            prop_assert_eq!(Digest::from_hex(&hex), Some(d));
        }
    }

    #[test]
    fn only_lowercase_hex_of_exact_length() {
        let ok = "00ff".repeat(16);
        assert_eq!(
            Digest::from_hex(&ok).map(|d| *d.bytes().get(1).unwrap_or(&0)),
            Some(255)
        );
        for bad in [
            "00FF".repeat(16),
            "0".repeat(63),
            "0".repeat(65),
            String::new(),
            format!("{}g", "0".repeat(63)),
            format!("{}é", "0".repeat(62)),
        ] {
            assert_eq!(Digest::from_hex(&bad), None, "{bad}");
        }
    }
}
