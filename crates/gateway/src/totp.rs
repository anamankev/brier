//! Time-based one-time passwords (RFC 6238): HMAC-SHA-1, 6 digits, 30-second steps, the
//! parameters every authenticator app supports.
//!
//! - A code is accepted for the current step and one step either side (clock drift), and each
//!   step at most once per user, so a code seen over someone's shoulder cannot be reused.
//! - Secrets are 20 random bytes, stored encrypted: a fresh 16-byte nonce `n`, and the secret
//!   XOR the first 20 bytes of HMAC-SHA-256(key, "brier-totp-v1" ‖ n), with the key from
//!   `BRIER_TOTP_KEY`. HMAC-SHA-256 is a PRF and each nonce is used once, so this is a one-block
//!   stream cipher. A database dump without the key reveals no secret. Tampering yields a
//!   different secret, whose codes simply fail.

use hmac::{Hmac, KeyInit, Mac};
use sha1::Sha1;
use sha2::Sha256;

use crate::error::ApiError;

/// Seconds per step.
pub const STEP_SECONDS: u64 = 30;
/// Code length.
pub const DIGITS: u32 = 6;
/// Secret length in bytes (160 bits, RFC 4226's recommendation).
pub const SECRET_BYTES: usize = 20;
const NONCE_BYTES: usize = 16;
const DOMAIN: &[u8] = b"brier-totp-v1";

/// The HOTP value of `secret` at `counter` (RFC 4226 §5.3), as a zero-padded string.
#[must_use]
pub fn hotp(secret: &[u8], counter: u64) -> String {
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(secret) else {
        return String::new(); // HMAC accepts keys of any length; unreachable
    };
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = usize::from(digest.last().copied().unwrap_or(0) & 0x0f);
    let word = digest
        .get(offset..offset.saturating_add(4))
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_be_bytes)
        & 0x7fff_ffff;
    let modulus = 10u32.pow(DIGITS);
    format!(
        "{:0width$}",
        word.checked_rem(modulus).unwrap_or(0),
        width = DIGITS as usize
    )
}

/// The step that `unix_seconds` falls in.
#[must_use]
pub const fn step(unix_seconds: u64) -> u64 {
    unix_seconds / STEP_SECONDS
}

/// The step in `[now − 1, now + 1]` whose code is `code`, if any.
#[must_use]
pub fn matching_step(secret: &[u8], code: &str, now_step: u64) -> Option<u64> {
    if code.len() != DIGITS as usize || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    [
        now_step.checked_sub(1),
        Some(now_step),
        now_step.checked_add(1),
    ]
    .into_iter()
    .flatten()
    .find(|&s| constant_time_eq(hotp(secret, s).as_bytes(), code.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// RFC 4648 base32 without padding, as authenticator apps expect.
#[must_use]
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for &b in bytes {
        buffer = (buffer << 8) | u32::from(b);
        bits = bits.saturating_add(8);
        while bits >= 5 {
            bits = bits.saturating_sub(5);
            let index = usize::try_from((buffer >> bits) & 31).unwrap_or(0);
            out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
        }
        buffer &= (1u32 << bits).saturating_sub(1);
    }
    if bits > 0 {
        let index = usize::try_from((buffer << (5u32.saturating_sub(bits))) & 31).unwrap_or(0);
        out.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
    }
    out
}

fn pad(key: &[u8; 32], nonce: &[u8]) -> Result<[u8; SECRET_BYTES], ApiError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|e| ApiError::internal(&e))?;
    mac.update(DOMAIN);
    mac.update(nonce);
    let block = mac.finalize().into_bytes();
    block
        .get(..SECRET_BYTES)
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| ApiError::internal(&"HMAC-SHA-256 output is shorter than a secret"))
}

/// A new random secret and its stored form (nonce ‖ encrypted secret).
///
/// # Errors
/// [`ApiError`] if the system random source fails.
pub fn new_secret(key: &[u8; 32]) -> Result<([u8; SECRET_BYTES], Vec<u8>), ApiError> {
    let mut secret = [0u8; SECRET_BYTES];
    let mut nonce = [0u8; NONCE_BYTES];
    getrandom::fill(&mut secret).map_err(|e| ApiError::internal(&e))?;
    getrandom::fill(&mut nonce).map_err(|e| ApiError::internal(&e))?;
    let mut stored = nonce.to_vec();
    stored.extend(secret.iter().zip(pad(key, &nonce)?).map(|(s, p)| s ^ p));
    Ok((secret, stored))
}

/// The secret in a stored form made by [`new_secret`].
///
/// # Errors
/// [`ApiError`] if the stored bytes have the wrong length.
pub fn open_secret(key: &[u8; 32], stored: &[u8]) -> Result<[u8; SECRET_BYTES], ApiError> {
    let (nonce, sealed) = stored
        .split_at_checked(NONCE_BYTES)
        .filter(|(_, s)| s.len() == SECRET_BYTES)
        .ok_or_else(|| ApiError::internal(&"stored TOTP secret has the wrong length"))?;
    let mut secret = [0u8; SECRET_BYTES];
    for ((out, s), p) in secret.iter_mut().zip(sealed).zip(pad(key, nonce)?) {
        *out = s ^ p;
    }
    Ok(secret)
}

/// Parse `BRIER_TOTP_KEY`: 64 lowercase hex digits.
#[must_use]
pub fn parse_key(hex: &str) -> Option<[u8; 32]> {
    brier_types::Digest::from_hex(hex.trim()).map(|d| *d.bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_6238_sha1_vectors() {
        // RFC 6238 appendix B: the SHA-1 secret "12345678901234567890", 8-digit codes; the
        // 6-digit code is the last six digits.
        let secret = b"12345678901234567890";
        for (t, eight) in [
            (59u64, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(hotp(secret, step(t)), eight[2..], "t = {t}");
        }
    }

    #[test]
    fn codes_match_one_step_either_side_only() {
        let secret = [7u8; SECRET_BYTES];
        let now: u64 = 1_000_000;
        for (offset, expected) in [
            (-2i64, None),
            (-1, Some(now - 1)),
            (0, Some(now)),
            (1, Some(now + 1)),
            (2, None),
        ] {
            let s = now.checked_add_signed(offset).unwrap();
            assert_eq!(
                matching_step(&secret, &hotp(&secret, s), now),
                expected,
                "{offset}"
            );
        }
        for bad in ["", "12345", "1234567", "12345a", "١٢٣٤٥٦"] {
            assert_eq!(matching_step(&secret, bad, now), None);
        }
    }

    #[test]
    fn base32_matches_rfc_4648() {
        for (input, expected) in [
            (&b""[..], ""),
            (b"f", "MY"),
            (b"fo", "MZXQ"),
            (b"foo", "MZXW6"),
            (b"foob", "MZXW6YQ"),
            (b"fooba", "MZXW6YTB"),
            (b"foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(input), expected);
        }
    }

    #[test]
    fn secrets_round_trip_and_differ_by_nonce() {
        let key = [9u8; 32];
        let (a, stored_a) = new_secret(&key).unwrap();
        let (_, stored_b) = new_secret(&key).unwrap();
        assert_eq!(open_secret(&key, &stored_a).unwrap(), a);
        assert_ne!(stored_a, stored_b);
        assert_ne!(
            open_secret(&[8u8; 32], &stored_a).unwrap(),
            a,
            "another key gives another secret"
        );
        assert!(open_secret(&key, &stored_a[..10]).is_err());
        assert_eq!(parse_key(&"ab".repeat(32)), Some([0xab; 32]));
        assert_eq!(parse_key("short"), None);
    }
}
