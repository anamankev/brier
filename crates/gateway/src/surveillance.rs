//! Surveillance (SPEC §9; docs/surveillance.md): the login signals that link accounts, and the
//! cases the surveillance service opens and admins work.
//!
//! - At each login the gateway records a keyed hash of the client address. Two accounts that
//!   share one are linked: weak evidence (households share addresses), but the only evidence
//!   that two accounts are one person.
//! - The surveillance service (`python -m brier_services.surveillance`, role `surveillance`)
//!   reads the links, runs the detectors over the exported journal, and opens one case per
//!   alert. It cannot trade, forecast, or close a case.
//! - An admin signed in with TOTP lists cases, reads the evidence, and closes each one as
//!   dismissed or actioned with a reason, which is audited.
#![allow(clippy::missing_errors_doc)] // handler errors are the documented API responses

use std::net::IpAddr;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::AppState;
use crate::audit::{self, Actor};
use crate::auth::{Role, User};
use crate::error::ApiError;
use crate::markets::iso;
use crate::routes::{ApiResult, Req, Shared, admin_user, parse_id};

/// Domain label for the login-signal key, derived from `BRIER_TOTP_KEY`.
const SIGNAL_LABEL: &[u8] = b"brier login-signal key v1";
/// The typologies a case can have (SPEC §9).
pub const TYPOLOGIES: [&str; 4] = ["wash", "spoof", "close", "insider"];
/// Largest number of cases one list returns.
pub const CASE_PAGE: i64 = 200;

fn hmac(key: &[u8], data: &[u8]) -> Result<[u8; 32], ApiError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|e| ApiError::internal(&e))?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().into())
}

/// The keyed hash recorded for a client address. The address's canonical text form is
/// hashed, so an IPv4 client seen as an IPv4-mapped IPv6 address hashes the same.
///
/// # Errors
/// [`ApiError`] only if HMAC rejects the key, which it never does for 32 bytes.
pub fn address_hash(totp_key: &[u8; 32], ip: IpAddr) -> Result<[u8; 32], ApiError> {
    let key = hmac(totp_key, SIGNAL_LABEL)?;
    hmac(&key, ip.to_canonical().to_string().as_bytes())
}

/// Record that `user` logged in from `ip`. Nothing is recorded without an address the gateway
/// trusts ([`crate::LoginAddress`]) or without `BRIER_TOTP_KEY`.
pub async fn record_login(s: &AppState, user: i64, ip: Option<IpAddr>) -> Result<(), ApiError> {
    let (Some(key), Some(ip)) = (s.totp_key, ip) else {
        return Ok(());
    };
    let hash = address_hash(&key, ip)?;
    s.db.execute(
        "INSERT INTO login_signals (user_id, address_hash) VALUES ($1, $2) \
         ON CONFLICT (user_id, address_hash) DO UPDATE SET last_seen = now(), logins = login_signals.logins + 1",
        &[&user, &hash.as_slice()],
    )
    .await?;
    Ok(())
}

/// The surveillance service, or an admin signed in with TOTP.
async fn watcher(s: &AppState, req: &Req) -> Result<User, ApiError> {
    let user = req.user(s, true).await?;
    match user.role {
        Role::Surveillance => Ok(user),
        Role::Admin => admin_user(s, req).await,
        _ => Err(ApiError::forbidden_because(
            "this needs the surveillance or admin role",
        )),
    }
}

/// `GET /v1/surveillance/links`: pairs of accounts that have logged in from a shared address.
pub async fn links(State(s): Shared, req: Req) -> ApiResult {
    watcher(&s, &req).await?;
    let rows =
        s.db.query(
            "SELECT a.user_id, b.user_id, count(*) FROM login_signals a \
             JOIN login_signals b ON a.address_hash = b.address_hash AND a.user_id < b.user_id \
             GROUP BY 1, 2 ORDER BY 1, 2",
            &[],
        )
        .await?;
    let links: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "accounts": [r.get::<_, i64>(0), r.get::<_, i64>(1)],
                "shared_addresses": r.get::<_, i64>(2),
            })
        })
        .collect();
    Ok(Json(json!({ "links": links })).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseBody {
    typology: String,
    market: u32,
    account: u64,
    score: f64,
    detector: String,
    evidence: Value,
}

/// `POST /v1/surveillance/cases`: open a case for an alert (surveillance role). The same
/// (typology, market, account) opens one case only: a repeat returns the existing case.
pub async fn open_case(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    if user.role != Role::Surveillance {
        return Err(ApiError::forbidden_because(
            "only the surveillance service opens cases",
        ));
    }
    let b: CaseBody = req.json()?;
    if !TYPOLOGIES.contains(&b.typology.as_str()) {
        return Err(ApiError::bad_request(
            "typology must be wash, spoof, close, or insider",
        ));
    }
    if !(b.score.is_finite() && b.score > 0.0) {
        return Err(ApiError::bad_request("score must be a positive number"));
    }
    if !(1..=100).contains(&b.detector.chars().count()) {
        return Err(ApiError::bad_request(
            "detector must be 1 to 100 characters",
        ));
    }
    if !b.evidence.is_object() {
        return Err(ApiError::bad_request("evidence must be a JSON object"));
    }
    let market = i64::from(b.market);
    let account =
        i64::try_from(b.account).map_err(|_| ApiError::bad_request("account is out of range"))?;
    let inserted =
        s.db.query_opt(
            "INSERT INTO cases (typology, market, account, score, detector, evidence, opened_by) \
             VALUES ($1, $2, $3, $4, $5, $6::text::jsonb, $7) \
             ON CONFLICT (typology, market, account) DO NOTHING RETURNING id",
            &[
                &b.typology,
                &market,
                &account,
                &b.score,
                &b.detector,
                &b.evidence.to_string(),
                &user.id,
            ],
        )
        .await?;
    let Some(row) = inserted else {
        let id: i64 =
            s.db.query_one(
                "SELECT id FROM cases WHERE typology = $1 AND market = $2 AND account = $3",
                &[&b.typology, &market, &account],
            )
            .await?
            .get(0);
        return Ok(Json(json!({ "case": id, "opened": false })).into_response());
    };
    let id: i64 = row.get(0);
    let detail = json!({ "case": id, "typology": b.typology, "market": market, "account": account, "score": b.score });
    audit::record(
        &s,
        Actor::User(user.id),
        "open_case",
        &detail,
        "detector alert",
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "case": id, "opened": true })),
    )
        .into_response())
}

#[derive(Deserialize)]
struct CaseFilter {
    status: Option<String>,
}

const CASE_COLUMNS: &str = "c.id, c.typology, c.market, c.account, u.username, c.score, c.detector, c.status, \
     extract(epoch FROM c.opened_at)::bigint, extract(epoch FROM c.closed_at)::bigint, cu.username, c.close_reason, \
     b.canonical::jsonb ->> 'title'";
const CASE_JOINS: &str = "FROM cases c LEFT JOIN users u ON u.id = c.account \
     LEFT JOIN users cu ON cu.id = c.closed_by LEFT JOIN rulebooks b ON b.market = c.market";

fn case_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "case": r.get::<_, i64>(0), "typology": r.get::<_, &str>(1), "market": r.get::<_, i64>(2),
        "account": r.get::<_, i64>(3), "username": r.get::<_, Option<&str>>(4),
        "score": r.get::<_, f64>(5), "detector": r.get::<_, &str>(6), "status": r.get::<_, &str>(7),
        "opened_at": iso(r.get(8)), "closed_at": r.get::<_, Option<i64>>(9).map_or(Value::Null, iso),
        "closed_by": r.get::<_, Option<&str>>(10), "close_reason": r.get::<_, Option<&str>>(11),
        "market_title": r.get::<_, Option<&str>>(12),
    })
}

/// `GET /v1/admin/cases?status=open|dismissed|actioned|all`: cases, newest first (default
/// open).
pub async fn cases(State(s): Shared, req: Req) -> ApiResult {
    admin_user(&s, &req).await?;
    let status = req
        .query::<CaseFilter>()?
        .status
        .unwrap_or_else(|| "open".to_owned());
    if !["open", "dismissed", "actioned", "all"].contains(&status.as_str()) {
        return Err(ApiError::bad_request(
            "status must be open, dismissed, actioned, or all",
        ));
    }
    let rows = s
        .db
        .query(
            &format!(
                "SELECT {CASE_COLUMNS} {CASE_JOINS} WHERE $1 = 'all' OR c.status = $1 ORDER BY c.id DESC LIMIT $2"
            ),
            &[&status, &CASE_PAGE],
        )
        .await?;
    let items: Vec<Value> = rows.iter().map(case_json).collect();
    Ok(Json(json!({ "cases": items })).into_response())
}

/// `GET /v1/admin/cases/{id}`: one case with its evidence, and the accounts linked to its
/// account by a shared login address.
pub async fn case(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    admin_user(&s, &req).await?;
    let id: i64 = parse_id(&raw, "case")?;
    let row =
        s.db.query_opt(
            &format!("SELECT {CASE_COLUMNS}, c.evidence::text {CASE_JOINS} WHERE c.id = $1"),
            &[&id],
        )
        .await?
        .ok_or_else(|| ApiError::not_found("no such case"))?;
    let account: i64 = row.get(3);
    let linked =
        s.db.query(
            "SELECT DISTINCT u.id, u.username FROM login_signals a \
             JOIN login_signals b ON a.address_hash = b.address_hash AND b.user_id <> a.user_id \
             JOIN users u ON u.id = b.user_id WHERE a.user_id = $1 ORDER BY u.id",
            &[&account],
        )
        .await?;
    let mut out = case_json(&row);
    if let Some(map) = out.as_object_mut() {
        map.insert(
            "evidence".to_owned(),
            serde_json::from_str(row.get(13)).unwrap_or(Value::Null),
        );
        map.insert(
            "linked_accounts".to_owned(),
            linked
                .iter()
                .map(|r| json!({ "account": r.get::<_, i64>(0), "username": r.get::<_, &str>(1) }))
                .collect(),
        );
    }
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CloseBody {
    outcome: String,
    reason: String,
}

/// `POST /v1/admin/cases/{id}/close`: close an open case as `dismissed` or `actioned`, with a
/// reason; audited.
pub async fn close_case(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let user = admin_user(&s, &req).await?;
    let id: i64 = parse_id(&raw, "case")?;
    let b: CloseBody = req.json()?;
    if !["dismissed", "actioned"].contains(&b.outcome.as_str()) {
        return Err(ApiError::bad_request(
            "outcome must be dismissed or actioned",
        ));
    }
    let reason = audit::reason(&b.reason)?;
    let updated =
        s.db.execute(
            "UPDATE cases SET status = $2, closed_by = $3, closed_at = now(), close_reason = $4 \
             WHERE id = $1 AND status = 'open'",
            &[&id, &b.outcome, &user.id, &reason],
        )
        .await?;
    if updated == 0 {
        let exists =
            s.db.query_opt("SELECT 1 FROM cases WHERE id = $1", &[&id])
                .await?
                .is_some();
        return Err(if exists {
            ApiError::conflict("this case is already closed")
        } else {
            ApiError::not_found("no such case")
        });
    }
    let audit_id = audit::record(
        &s,
        Actor::User(user.id),
        "close_case",
        &json!({ "case": id, "outcome": b.outcome }),
        reason,
    )
    .await?;
    Ok(Json(json!({ "case": id, "status": b.outcome, "audit_id": audit_id })).into_response())
}
