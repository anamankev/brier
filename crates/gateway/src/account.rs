//! Public accounts (G10 U1): what sign-up asks for, recovery codes for a lost password, the
//! user's data export, and account deletion.
//!
//! Brier has no email, so a lost password is recovered with one of ten one-time codes shown
//! once at sign-up (and on regeneration). Deleting an account removes everything that
//! identifies the person; the ledger's entries stay under a `deleted_N` name, because
//! conservation and settlement need them.
#![allow(clippy::missing_errors_doc)]

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use brier_core::Answer;
use brier_engine::Command;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_postgres::Client;

use crate::audit::{self, Actor};
use crate::auth;
use crate::error::ApiError;
use crate::routes::{ApiResult, Req, Shared};
use crate::{AppState, totp};

/// The terms of use a new user accepts (`web/app/terms`); a later version is accepted at its
/// own date. Clients send this exact string.
pub const TERMS_VERSION: &str = "2026-11-12";
/// Recovery codes issued at once. Ten covers years of lost passwords; more invites careless
/// storage.
pub const RECOVERY_CODES: usize = 10;
/// Random bytes in a code: 80 bits, 16 base32 characters, beyond guessing at the login rate
/// limit.
const CODE_BYTES: usize = 10;
/// Accounts one client address may open in a day. A household or a shared office opens a few;
/// a script farming sign-up credits opens hundreds.
pub const SIGNUPS_PER_ADDRESS_PER_DAY: i64 = 5;
/// Accounts the whole exchange opens in a day before sign-up pauses. Well above a hobby
/// exchange's launch day; reaching it means a botnet, and pausing beats diluting every
/// leaderboard.
pub const SIGNUPS_PER_DAY: i64 = 5_000;

fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// A code as typed: case, spaces and dashes do not matter.
fn normalize(code: &str) -> String {
    code.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// `XXXX-XXXX-XXXX-XXXX`, easier to copy by hand.
fn display(code: &str) -> String {
    code.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// Replace the user's recovery codes with fresh ones; the codes are returned once, and only
/// their hashes are kept.
pub(crate) async fn new_codes(db: &Client, user: i64) -> Result<Vec<String>, ApiError> {
    let mut codes = Vec::with_capacity(RECOVERY_CODES);
    let mut hashes = Vec::with_capacity(RECOVERY_CODES);
    for _ in 0..RECOVERY_CODES {
        let mut raw = [0u8; CODE_BYTES];
        getrandom::fill(&mut raw).map_err(|e| ApiError::internal(&e))?;
        let code = totp::base32(&raw);
        hashes.push(sha256(code.as_bytes()).to_vec());
        codes.push(display(&code));
    }
    db.execute("DELETE FROM recovery_codes WHERE user_id = $1", &[&user])
        .await?;
    db.execute(
        "INSERT INTO recovery_codes (user_id, code_hash) SELECT $1, unnest($2::bytea[])",
        &[&user, &hashes],
    )
    .await?;
    Ok(codes)
}

/// What sign-up must confirm, and whether this address or the exchange has hit its daily
/// limit. `address` is the HMAC of a trusted client address, when the gateway has one.
pub(crate) async fn check_signup(
    db: &Client,
    adult: bool,
    terms: &str,
    address: Option<&[u8; 32]>,
) -> Result<(), ApiError> {
    if !adult {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "adult_required",
            "Brier is for people 18 or older",
        ));
    }
    if terms != TERMS_VERSION {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "terms_required",
            format!("accept the terms of use (version {TERMS_VERSION})"),
        ));
    }
    let today: i64 = db
        .query_one(
            "SELECT count(*) FROM users WHERE created_at > now() - interval '1 day'",
            &[],
        )
        .await?
        .get(0);
    if today >= SIGNUPS_PER_DAY {
        return Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "signups_paused",
            "sign-up is paused for today; please try again tomorrow",
        ));
    }
    if let Some(hash) = address {
        let mine: i64 = db
            .query_one(
                "SELECT count(*) FROM users WHERE signup_address = $1 AND created_at > now() - interval '1 day'",
                &[&hash.as_slice()],
            )
            .await?
            .get(0);
        if mine >= SIGNUPS_PER_ADDRESS_PER_DAY {
            return Err(ApiError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_signups",
                "too many accounts opened from this network today",
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverBody {
    username: String,
    code: String,
    new_password: String,
    /// The current TOTP code, when TOTP is on: a recovery code replaces the password, not the
    /// second factor.
    #[serde(default)]
    totp: Option<String>,
}

/// `POST /v1/recover`: set a new password with a recovery code. The code is used up, and every
/// session ends.
pub async fn recover(State(s): Shared, req: Req) -> ApiResult {
    if !s.auth_limits.allow(req.ip) {
        return Err(ApiError::rate_limited());
    }
    let b: RecoverBody = req.json()?;
    let hash = auth::hash_password_blocking(b.new_password).await?;
    let wrong = || ApiError::unauthorized("wrong username or recovery code");
    let row = s
        .db
        .query_opt(
            "SELECT id, totp_enabled, totp_secret FROM users WHERE username = $1 AND deleted_at IS NULL",
            &[&b.username],
        )
        .await?
        .ok_or_else(wrong)?;
    let user: i64 = row.get(0);
    let code_hash = sha256(normalize(&b.code).as_bytes());
    let unused: bool = s
        .db
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM recovery_codes WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL)",
            &[&user, &code_hash.as_slice()],
        )
        .await?
        .get(0);
    if !unused {
        return Err(wrong());
    }
    if row.get::<_, bool>(1) {
        let stored: Vec<u8> = row.get(2);
        let code = b.totp.ok_or_else(|| {
            ApiError::new(
                StatusCode::UNAUTHORIZED,
                "totp_required",
                "this account needs its TOTP code",
            )
        })?;
        crate::routes::check_totp(&s, user, &stored, &code).await?;
    }
    // Using the code and changing the password happen together, so a code is never spent
    // without the password changing, or the reverse.
    let changed = s
        .db
        .execute(
            "WITH spent AS (UPDATE recovery_codes SET used_at = now() WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL RETURNING user_id) \
             UPDATE users SET password_hash = $3 FROM spent WHERE users.id = spent.user_id",
            &[&user, &code_hash.as_slice(), &hash],
        )
        .await?;
    if changed != 1 {
        return Err(wrong());
    }
    s.db.execute("DELETE FROM sessions WHERE user_id = $1", &[&user])
        .await?;
    let left: i64 =
        s.db.query_one(
            "SELECT count(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL",
            &[&user],
        )
        .await?
        .get(0);
    audit::record(
        &s,
        Actor::User(user),
        "recover_password",
        &json!({}),
        "password reset with a recovery code",
    )
    .await?;
    Ok(Json(json!({ "codes_left": left })).into_response())
}

/// `POST /v1/recovery-codes`: replace the caller's recovery codes (session only). The new codes
/// are in the answer and nowhere else.
pub async fn regenerate(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, false).await?;
    let codes = new_codes(&s.db, user.id).await?;
    Ok(Json(json!({ "recovery_codes": codes })).into_response())
}

/// `GET /v1/me/export`: everything Brier holds about the caller, as JSON (session only).
pub async fn export(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, false).await?;
    let account = user.account()?;
    let profile = s
        .db
        .query_one(
            r#"SELECT username, role, totp_enabled, to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'),
                      to_char(adult_confirmed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'), terms_version,
                      to_char(terms_accepted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"')
               FROM users WHERE id = $1"#,
            &[&user.id],
        )
        .await?;
    let id = i64::try_from(account.get()).map_err(|e| ApiError::internal(&e))?;
    let rows = |sql: &'static str| {
        let db = &s.db;
        async move {
            let rows = db.query(sql, &[&id]).await?;
            rows.iter()
                .map(|r| serde_json::from_str::<Value>(r.get::<_, &str>(0)))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| ApiError::internal(&e))
        }
    };
    let fills = rows("SELECT to_jsonb(f)::text FROM fills f WHERE maker_account = $1 OR taker_account = $1 ORDER BY seq, ordinal").await?;
    let orders = rows(
        "SELECT to_jsonb(o)::text FROM order_events o WHERE account = $1 ORDER BY seq, ordinal",
    )
    .await?;
    let forecasts = rows(r#"SELECT jsonb_build_object('market', market, 'probability', probability, 'at', to_char(at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'))::text FROM forecasts WHERE user_id = $1 ORDER BY at"#).await?;
    let keys = rows(r#"SELECT jsonb_build_object('key_id', id, 'created_at', to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS"Z"'), 'revoked', revoked_at IS NOT NULL)::text FROM api_keys WHERE user_id = $1 ORDER BY id"#).await?;
    let balance = match s.core.query(brier_core::Query::Account(account)).await? {
        Answer::Account { view: Some(v), .. } => {
            json!({ "cash": v.cash.cents(), "reserved": v.reserved.cents() })
        }
        _ => Value::Null,
    };
    Ok(Json(json!({
        "exported_at": auth::now_ms(),
        "profile": {
            "user_id": user.id, "username": profile.get::<_, &str>(0), "role": profile.get::<_, &str>(1),
            "totp": profile.get::<_, bool>(2), "created_at": profile.get::<_, Option<String>>(3),
            "adult_confirmed_at": profile.get::<_, Option<String>>(4),
            "terms_version": profile.get::<_, Option<String>>(5),
            "terms_accepted_at": profile.get::<_, Option<String>>(6),
        },
        "balance_cents": balance,
        "orders": orders,
        "fills": fills,
        "forecasts": forecasts,
        "api_keys": keys,
    }))
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteBody {
    password: String,
}

/// `DELETE /v1/me`: delete the caller's account (session only, password required). Open orders
/// are cancelled; positions stay in the ledger under `deleted_N` and still settle, but nothing
/// that identifies the person is kept, and no one can sign in again.
pub async fn delete(State(s): Shared, req: Req) -> ApiResult {
    if !s.auth_limits.allow(req.ip) {
        return Err(ApiError::rate_limited());
    }
    let user = req.user(&s, false).await?;
    let DeleteBody { password } = req.json()?;
    let stored: String =
        s.db.query_one("SELECT password_hash FROM users WHERE id = $1", &[&user.id])
            .await?
            .get(0);
    if !auth::verify_password_blocking(password, stored).await? {
        return Err(ApiError::unauthorized("wrong password"));
    }
    erase(&s, user.id).await?;
    audit::record(
        &s,
        Actor::User(user.id),
        "delete_account",
        &json!({}),
        "user deleted their account",
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Cancel the user's open orders, then remove everything that identifies them and every way
/// to sign in. The row stays, renamed `deleted_N`, because the ledger's entries need it.
async fn erase(s: &AppState, user: i64) -> Result<(), ApiError> {
    let account = brier_types::AccountId::new(
        u64::try_from(user).map_err(|_| ApiError::internal(&"negative user id"))?,
    );
    s.core
        .submit(Command::CancelAll {
            account: Some(account),
            market: None,
        })
        .await?;
    let name = format!("deleted_{user}");
    s.db.execute(
        "UPDATE users SET username = $2, password_hash = '!', totp_enabled = false, totp_secret = NULL, totp_last_step = NULL, \
         signup_address = NULL, deleted_at = now() WHERE id = $1",
        &[&user, &name],
    )
    .await?;
    for sql in [
        "DELETE FROM sessions WHERE user_id = $1",
        "DELETE FROM recovery_codes WHERE user_id = $1",
        "DELETE FROM login_signals WHERE user_id = $1",
        "UPDATE api_keys SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    ] {
        s.db.execute(sql, &[&user]).await?;
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseBody {
    reason: String,
}

/// `POST /v1/admin/users/{id}/close`: close an account that broke the house rules (admin with
/// TOTP, reason required, audited). The same as the user deleting it: orders cancelled, sign-in
/// gone, personal data removed. Admin and service accounts cannot be closed this way.
pub async fn close(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let admin = crate::routes::admin_user(&s, &req).await?;
    let id: i64 = crate::routes::parse_id(&raw, "user id")?;
    let CloseBody { reason } = req.json()?;
    let reason = audit::reason(&reason)?;
    let role: String =
        s.db.query_opt(
            "SELECT role FROM users WHERE id = $1 AND deleted_at IS NULL",
            &[&id],
        )
        .await?
        .ok_or_else(|| ApiError::not_found("no such open account"))?
        .get(0);
    if role != "trader" {
        return Err(ApiError::conflict("only trader accounts can be closed"));
    }
    erase(&s, id).await?;
    audit::record(
        &s,
        Actor::User(admin.id),
        "close_account",
        &json!({ "user_id": id }),
        reason,
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}

/// Record what the user confirmed at sign-up.
pub(crate) async fn record_signup(
    s: &AppState,
    user: i64,
    address: Option<[u8; 32]>,
) -> Result<(), ApiError> {
    let address = address.map(|a| a.to_vec());
    s.db.execute(
        "UPDATE users SET adult_confirmed_at = now(), terms_version = $2, terms_accepted_at = now(), signup_address = $3 WHERE id = $1",
        &[&user, &TERMS_VERSION, &address],
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_typed_loosely() {
        assert_eq!(normalize("abcd-EFGH ijkl-mnop"), "ABCDEFGHIJKLMNOP");
        assert_eq!(display("ABCDEFGHIJKLMNOP"), "ABCD-EFGH-IJKL-MNOP");
        assert_eq!(normalize(&display("ABCDEFGHIJKLMNOP")), "ABCDEFGHIJKLMNOP");
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TermsBody {
    terms: String,
}

/// `POST /v1/terms`: accept the current terms of use, after they changed since sign-up.
pub async fn accept_terms(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let TermsBody { terms } = req.json()?;
    if terms != TERMS_VERSION {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "terms_required",
            format!("accept the terms of use (version {TERMS_VERSION})"),
        ));
    }
    s.db.execute(
        "UPDATE users SET terms_version = $2, terms_accepted_at = now() WHERE id = $1",
        &[&user.id, &TERMS_VERSION],
    )
    .await?;
    if let Ok(mut ok) = s.terms_ok.lock() {
        ok.insert(user.id);
    }
    Ok(Json(json!({ "terms_version": TERMS_VERSION })).into_response())
}

/// Whether `user` has accepted the current terms. Accounts that never accepted any version
/// (the exchange's own service accounts, created by the operator) are not asked; people who
/// accepted an older version must accept the new one before placing orders. A yes is cached:
/// it cannot become a no until the next version, which ships with a restart.
pub(crate) async fn terms_current(s: &AppState, user: i64) -> Result<bool, ApiError> {
    if s.terms_ok.lock().is_ok_and(|ok| ok.contains(&user)) {
        return Ok(true);
    }
    let row =
        s.db.query_one("SELECT terms_version FROM users WHERE id = $1", &[&user])
            .await?;
    let current = row
        .get::<_, Option<String>>(0)
        .is_none_or(|v| v == TERMS_VERSION);
    if current && let Ok(mut ok) = s.terms_ok.lock() {
        ok.insert(user);
    }
    Ok(current)
}
