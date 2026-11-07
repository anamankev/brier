//! Identity (SPEC §6, §13). People sign in with a password, stored only as an Argon2id hash,
//! and get a session. Bots sign every request with an Ed25519 key.
//!
//! A signed request carries three headers:
//! - `Brier-Key`: the API key's ID;
//! - `Brier-Timestamp`: milliseconds since the Unix epoch;
//! - `Brier-Signature`: base64 Ed25519 signature of
//!   `"{timestamp}\n{METHOD}\n{path and query}\n{hex SHA-256 of the body}"`.
//!
//! The server keeps only the public key, refuses timestamps more than [`MAX_CLOCK_SKEW_MS`] from
//! its clock, and refuses any signature already seen inside that window (a replay).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use axum::http::{HeaderMap, Method};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;

use crate::error::ApiError;

/// How far a signed request's timestamp may be from the server clock.
pub const MAX_CLOCK_SKEW_MS: u64 = 5_000;
/// How long a session token is valid.
pub const SESSION_HOURS: i32 = 24;
/// Shortest password accepted (NIST SP 800-63B recommends at least 8; 12 here).
pub const MIN_PASSWORD_LEN: usize = 12;
/// Longest password accepted, bounding hashing cost per request.
pub const MAX_PASSWORD_LEN: usize = 256;

/// Who a request comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct User {
    /// User ID, which is also the engine account ID.
    pub id: i64,
    /// What the user may do.
    pub role: Role,
}

/// A user's role (the `users.role` column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Trades its own account.
    Trader,
    /// Trades, and runs administrative commands.
    Admin,
    /// Proposes market outcomes, and nothing else: it cannot trade, confirm, or settle.
    Resolver,
}

impl Role {
    /// The role stored as `name`; an unknown name gets the fewest rights.
    #[must_use]
    pub fn from_name(name: &str) -> Self {
        match name {
            "admin" => Self::Admin,
            "resolver" => Self::Resolver,
            _ => Self::Trader,
        }
    }
}

impl User {
    /// The user's engine account.
    ///
    /// # Errors
    /// [`ApiError`] if the ID is negative (never true for rows from `users`).
    pub fn account(&self) -> Result<brier_types::AccountId, ApiError> {
        u64::try_from(self.id)
            .map(brier_types::AccountId::new)
            .map_err(|_| ApiError::internal(&"negative user id"))
    }
}

/// Argon2id hash of a password, as a PHC string (with its own random salt).
///
/// # Errors
/// [`ApiError`] if the password is outside the allowed length, or hashing fails.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    check_password_length(password)?;
    Argon2::default()
        .hash_password(password.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal(&e))
}

/// Whether `password` matches the stored PHC hash.
#[must_use]
pub fn verify_password(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    password.len() <= MAX_PASSWORD_LEN
        && Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
}

fn check_password_length(password: &str) -> Result<(), ApiError> {
    if (MIN_PASSWORD_LEN..=MAX_PASSWORD_LEN).contains(&password.chars().count()) {
        Ok(())
    } else {
        Err(ApiError::bad_request(format!(
            "passwords must be {MIN_PASSWORD_LEN} to {MAX_PASSWORD_LEN} characters"
        )))
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(
        String::with_capacity(bytes.len().saturating_mul(2)),
        |mut s, b| {
            let _ = write!(s, "{b:02x}"); // writing to a String cannot fail
            s
        },
    )
}

/// A fresh session token for `user_id`; only its SHA-256 is stored.
///
/// # Errors
/// [`ApiError`] if randomness or the database fails.
pub async fn new_session(db: &Client, user_id: i64) -> Result<String, ApiError> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| ApiError::internal(&e))?;
    let token = URL_SAFE_NO_PAD.encode(raw);
    db.execute(
        "INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, now() + make_interval(hours => $3))",
        &[&sha256(token.as_bytes()).as_slice(), &user_id, &SESSION_HOURS],
    )
    .await?;
    Ok(token)
}

/// End a session.
///
/// # Errors
/// [`ApiError`] if the database fails.
pub async fn end_session(db: &Client, token: &str) -> Result<(), ApiError> {
    db.execute(
        "DELETE FROM sessions WHERE token_hash = $1",
        &[&sha256(token.as_bytes()).as_slice()],
    )
    .await?;
    Ok(())
}

/// The bearer token in `Authorization`, if any.
#[must_use]
pub fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}

/// Signatures seen recently, for replay refusal. Entries older than the skew window can never
/// be accepted again anyway (their timestamp would be stale), so they are pruned.
#[derive(Default)]
pub struct ReplayCache {
    seen: Mutex<HashMap<[u8; 64], u64>>,
}

impl ReplayCache {
    /// Record a signature at `now_ms`; false if it was already seen inside the window.
    fn first_use(&self, signature: [u8; 64], now_ms: u64) -> bool {
        let Ok(mut seen) = self.seen.lock() else {
            return false;
        };
        seen.retain(|_, at| now_ms.saturating_sub(*at) <= MAX_CLOCK_SKEW_MS.saturating_mul(2));
        seen.insert(signature, now_ms).is_none()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// The exact bytes a client signs.
#[must_use]
pub fn signing_payload(
    timestamp_ms: u64,
    method: &Method,
    path_and_query: &str,
    body: &[u8],
) -> Vec<u8> {
    format!(
        "{timestamp_ms}\n{method}\n{path_and_query}\n{}",
        hex(&sha256(body))
    )
    .into_bytes()
}

/// The three signed-request headers, parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignedHeaders {
    /// `Brier-Key`: the API key's ID.
    pub key_id: i64,
    /// `Brier-Timestamp`: milliseconds since the Unix epoch.
    pub timestamp: u64,
    /// `Brier-Signature`, decoded.
    pub signature: [u8; 64],
}

/// The signed-request headers, if the request has any.
///
/// # Errors
/// [`ApiError::unauthorized`] if some are present but missing or malformed.
pub fn signed_headers(headers: &HeaderMap) -> Result<Option<SignedHeaders>, ApiError> {
    let header = |name: &str| headers.get(name).map(|v| v.to_str().ok());
    let (key_id, timestamp, signature) = match (
        header("brier-key"),
        header("brier-timestamp"),
        header("brier-signature"),
    ) {
        (None, None, None) => return Ok(None),
        (Some(Some(k)), Some(Some(t)), Some(Some(s))) => (k, t, s),
        _ => {
            return Err(ApiError::unauthorized(
                "a signed request needs Brier-Key, Brier-Timestamp, and Brier-Signature",
            ));
        }
    };
    Ok(Some(SignedHeaders {
        key_id: key_id
            .parse()
            .map_err(|_| ApiError::unauthorized("bad Brier-Key"))?,
        timestamp: timestamp
            .parse()
            .map_err(|_| ApiError::unauthorized("bad Brier-Timestamp"))?,
        signature: STANDARD
            .decode(signature)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| ApiError::unauthorized("bad Brier-Signature"))?,
    }))
}

/// Refuse a timestamp more than [`MAX_CLOCK_SKEW_MS`] from `now_ms`.
///
/// # Errors
/// [`ApiError::unauthorized`] for a stale or future timestamp.
pub fn check_timestamp(timestamp: u64, now_ms: u64) -> Result<(), ApiError> {
    if now_ms.abs_diff(timestamp) > MAX_CLOCK_SKEW_MS {
        return Err(ApiError::unauthorized(
            "the request timestamp is too far from the server clock",
        ));
    }
    Ok(())
}

/// Verify a signed request against a stored public key, strictly (RFC 8032 with the
/// small-order and non-canonical encodings refused).
///
/// # Errors
/// [`ApiError::unauthorized`] if the key is invalid or the signature does not verify.
pub fn verify_signature(
    public_key: &[u8],
    signed: &SignedHeaders,
    method: &Method,
    path_and_query: &str,
    body: &[u8],
) -> Result<(), ApiError> {
    let public: [u8; 32] = public_key
        .try_into()
        .map_err(|_| ApiError::unauthorized("the API key is not 32 bytes"))?;
    let key = VerifyingKey::from_bytes(&public)
        .map_err(|_| ApiError::unauthorized("the API key is not a valid Ed25519 key"))?;
    key.verify_strict(
        &signing_payload(signed.timestamp, method, path_and_query, body),
        &Signature::from_bytes(&signed.signature),
    )
    .map_err(|_| ApiError::unauthorized("the signature does not verify"))
}

/// Identify the caller: a session token, or a signed request. `allow_signed` is false for
/// endpoints only people may use (managing API keys).
///
/// # Errors
/// [`ApiError::unauthorized`] for missing, expired, stale, replayed, or invalid credentials.
pub async fn authenticate(
    db: &Client,
    replays: &ReplayCache,
    headers: &HeaderMap,
    method: &Method,
    path_and_query: &str,
    body: &[u8],
    allow_signed: bool,
) -> Result<User, ApiError> {
    if let Some(token) = bearer(headers) {
        let row = db
            .query_opt(
                "SELECT u.id, u.role FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = $1 AND s.expires_at > now()",
                &[&sha256(token.as_bytes()).as_slice()],
            )
            .await?;
        let row = row.ok_or_else(|| ApiError::unauthorized("the session is invalid or expired"))?;
        return Ok(User {
            id: row.get(0),
            role: Role::from_name(row.get(1)),
        });
    }
    if !allow_signed {
        return Err(ApiError::unauthorized(
            "this endpoint needs a session token",
        ));
    }
    let Some(signed) = signed_headers(headers)? else {
        return Err(ApiError::unauthorized(
            "send a session token or a signed request",
        ));
    };
    let now = now_ms();
    check_timestamp(signed.timestamp, now)?;
    let row = db
        .query_opt(
            "SELECT k.public_key, u.id, u.role FROM api_keys k JOIN users u ON u.id = k.user_id WHERE k.id = $1 AND k.revoked_at IS NULL",
            &[&signed.key_id],
        )
        .await?
        .ok_or_else(|| ApiError::unauthorized("unknown or revoked API key"))?;
    verify_signature(
        row.get::<_, &[u8]>(0),
        &signed,
        method,
        path_and_query,
        body,
    )?;
    let signature = signed.signature;
    if !replays.first_use(signature, now) {
        return Err(ApiError::unauthorized(
            "this signed request was already used",
        ));
    }
    Ok(User {
        id: row.get(1),
        role: Role::from_name(row.get(2)),
    })
}

/// Register a public key for `user_id`; returns its key ID.
///
/// # Errors
/// [`ApiError::bad_request`] if the key is not a valid Ed25519 public key.
pub async fn add_key(db: &Client, user_id: i64, public_key_b64: &str) -> Result<i64, ApiError> {
    let bytes: [u8; 32] = STANDARD
        .decode(public_key_b64)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| ApiError::bad_request("public_key must be 32 bytes, base64"))?;
    VerifyingKey::from_bytes(&bytes)
        .map_err(|_| ApiError::bad_request("public_key is not a valid Ed25519 key"))?;
    let row = db
        .query_one(
            "INSERT INTO api_keys (user_id, public_key) VALUES ($1, $2) RETURNING id",
            &[&user_id, &bytes.as_slice()],
        )
        .await?;
    Ok(row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_with_their_own_salt_and_verify() {
        let a = hash_password("correct horse battery").unwrap();
        let b = hash_password("correct horse battery").unwrap();
        assert!(a.starts_with("$argon2id$"), "{a}");
        assert_ne!(a, b, "each hash has its own salt");
        assert!(verify_password("correct horse battery", &a));
        assert!(!verify_password("correct horse batterz", &a));
        assert!(hash_password("short").is_err());
    }

    #[test]
    fn a_signature_is_refused_the_second_time() {
        let cache = ReplayCache::default();
        assert!(cache.first_use([1; 64], 1_000));
        assert!(!cache.first_use([1; 64], 2_000));
        // Long after the window, the entry is gone (a stale timestamp would be refused anyway).
        assert!(cache.first_use([1; 64], 60_000));
    }

    #[test]
    fn the_signed_payload_binds_time_method_path_and_body() {
        let p = signing_payload(1_700_000_000_000, &Method::POST, "/v1/orders", b"{}");
        let text = String::from_utf8(p).unwrap();
        // SHA-256 of "{}" (two bytes).
        assert_eq!(
            text,
            "1700000000000\nPOST\n/v1/orders\n44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
        );
    }
}
