//! Markets listed from rulebooks (docs/rulebook.md) and kept to their schedules.
//!
//! An admin lists a market by sending its rulebook. The gateway validates it with
//! `brier-rules`, stores its canonical JSON, and sends `create_market` carrying the rules hash,
//! so the journal pins the exact rules. The rulebook's times then govern the market: the
//! scheduler opens and closes it ([`crate::scheduler`]), and an admin cannot open it outside
//! its trading window.
#![allow(clippy::missing_errors_doc)] // handler errors are the documented API responses

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use brier_core::{Answer, Query};
use brier_engine::{Command, Event, MarketState};
use brier_rules::time::format_instant;
use brier_rules::{Validated, hex, validate};
use brier_types::{Digest, MarketId};
use brier_wire::reject_name;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::audit::{self, Actor};
use crate::clock::Unix;
use crate::error::ApiError;
use crate::routes::{ApiResult, Req, Shared, admin_user, parse_id};

/// A listed market's stored rulebook.
#[derive(Debug, Clone)]
pub struct Listing {
    /// The market.
    pub market: MarketId,
    /// SHA-256 of the canonical rulebook.
    pub rules_hash: Digest,
    /// Trading opens.
    pub open: Unix,
    /// Trading closes.
    pub close: Unix,
    /// The source publishes the value.
    pub release: Unix,
    /// The resolution deadline.
    pub deadline: Unix,
    /// The canonical rulebook.
    pub canonical: String,
}

impl Listing {
    /// The rulebook's void conditions, by name.
    #[must_use]
    pub fn void_conditions(&self) -> Vec<String> {
        serde_json::from_str::<Value>(&self.canonical)
            .ok()
            .and_then(|v| {
                v.get("void").and_then(Value::as_array).map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_owned))
                        .collect()
                })
            })
            .unwrap_or_default()
    }
}

/// An instant as `YYYY-MM-DDTHH:MM:SSZ` (stored times are always inside the rulebook range).
pub(crate) fn iso(t: Unix) -> Value {
    format_instant(t).map_or(Value::Null, Value::String)
}

/// The stored rulebook for `market`, if it was listed from one.
pub async fn listing(s: &AppState, market: MarketId) -> Result<Option<Listing>, ApiError> {
    let row = s
        .db
        .query_opt(
            "SELECT rules_hash, extract(epoch FROM open_at)::bigint, extract(epoch FROM close_at)::bigint, \
             extract(epoch FROM release_at)::bigint, extract(epoch FROM deadline_at)::bigint, canonical \
             FROM rulebooks WHERE market = $1",
            &[&i64::from(market.get())],
        )
        .await?;
    row.map(|r| {
        let bytes: [u8; 32] = r
            .get::<_, &[u8]>(0)
            .try_into()
            .map_err(|_| ApiError::internal(&"stored rules hash is not 32 bytes"))?;
        Ok(Listing {
            market,
            rules_hash: Digest::new(bytes),
            open: r.get(1),
            close: r.get(2),
            release: r.get(3),
            deadline: r.get(4),
            canonical: r.get(5),
        })
    })
    .transpose()
}

/// The market's state in the engine, or `None` if it does not exist.
pub async fn market_state(s: &AppState, market: MarketId) -> Result<Option<MarketState>, ApiError> {
    match s.core.query(Query::Book(market)).await? {
        Answer::Book { state, .. } => Ok(state),
        _ => Err(ApiError::internal(&"book query answered another shape")),
    }
}

/// Refuse a manual `set_state` to `open` outside the rulebook's trading window. Halting and
/// closing are always allowed: they are how an admin stops trading.
pub async fn check_manual_state(
    s: &AppState,
    market: MarketId,
    state: MarketState,
) -> Result<(), ApiError> {
    if state != MarketState::Open {
        return Ok(());
    }
    let listing = listing(s, market)
        .await?
        .ok_or_else(|| ApiError::conflict("this market has no rulebook, so it cannot open"))?;
    let now = s.clock.now();
    if now < listing.open {
        return Err(ApiError::too_early("the market's open time has not come"));
    }
    if now >= listing.close {
        return Err(ApiError::conflict("the market is past its close time"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, "invalid_rulebook", message)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateMarketBody {
    rulebook: Value,
    reason: String,
}

/// Store the rulebook, or accept a retry of the same one; refuse a different one.
async fn store(s: &AppState, v: &Validated, user: i64) -> Result<(), ApiError> {
    let market = i64::from(v.rulebook.market);
    let t = &v.times;
    let inserted = s
        .db
        .execute(
            "INSERT INTO rulebooks (market, canonical, rules_hash, open_at, close_at, release_at, deadline_at, listed_by) \
             VALUES ($1, $2, $3, to_timestamp($4::bigint), to_timestamp($5::bigint), to_timestamp($6::bigint), to_timestamp($7::bigint), $8) \
             ON CONFLICT DO NOTHING",
            &[
                &market,
                &v.canonical,
                &v.hash.as_slice(),
                &t.open,
                &t.close,
                &t.release,
                &t.deadline,
                &user,
            ],
        )
        .await?;
    if inserted == 1 {
        return Ok(());
    }
    let same =
        s.db.query_opt(
            "SELECT 1 FROM rulebooks WHERE market = $1 AND rules_hash = $2",
            &[&market, &v.hash.as_slice()],
        )
        .await?
        .is_some();
    if same {
        Ok(())
    } else {
        Err(ApiError::conflict(
            "a different rulebook is already stored for this market ID",
        ))
    }
}

/// `POST /v1/admin/markets`: list a market from its rulebook (it starts in `pre_open`).
pub async fn create_market(State(s): Shared, req: Req) -> ApiResult {
    let user = admin_user(&s, &req).await?;
    let b: CreateMarketBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let v = validate(&b.rulebook).map_err(|e| invalid(format!("{}: {e}", e.code())))?;
    if v.times.close <= s.clock.now() {
        return Err(invalid("schedule_past: the close time has already passed"));
    }
    let market = MarketId::new(v.rulebook.market);
    if market_state(&s, market).await?.is_some() {
        return Err(ApiError::conflict("a market with this ID already exists"));
    }
    store(&s, &v, user.id).await?;
    let rules_hash = Digest::new(v.hash);
    let command = Command::CreateMarket {
        market,
        rules_hash: Some(rules_hash),
    };
    let detail = json!({ "market": market.get(), "rules_hash": rules_hash.to_hex() });
    let done = audit::submit(
        &s,
        Actor::User(user.id),
        "create_market",
        command,
        detail,
        reason,
    )
    .await?;
    let seq = done.committed.seq.get();
    if let [Event::Rejected { reason }] = done.committed.events.as_slice() {
        return Err(ApiError::rejected(reject_name(*reason), seq));
    }
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "seq": seq, "audit_id": done.audit_id, "market": market.get(),
            "rules_hash": rules_hash.to_hex(), "events": done.events,
        })),
    )
        .into_response())
}

/// `GET /v1/markets/{id}/rules`: the market's rulebook, its rules hash, the rule text, and
/// what the journal recorded, so anyone can check the two agree.
pub async fn rules(State(s): Shared, Path(raw): Path<String>) -> ApiResult {
    let market = MarketId::new(parse_id(&raw, "market")?);
    let l = listing(&s, market)
        .await?
        .ok_or_else(|| ApiError::not_found("no rulebook for this market"))?;
    let rulebook: Value = serde_json::from_str(&l.canonical).map_err(|e| ApiError::internal(&e))?;
    let text = validate(&rulebook)
        .map(|v| v.describe())
        .map_err(|e| ApiError::internal(&e))?;
    let journal =
        s.db.query_opt(
            "SELECT listed_seq, rules_hash FROM market_journal WHERE market = $1",
            &[&i64::from(market.get())],
        )
        .await?
        .map(|r| {
            let hash: Option<&[u8]> = r.get(1);
            json!({
                "listed_seq": r.get::<_, i64>(0),
                "rules_hash": hash.map(hex),
                "matches": hash == Some(l.rules_hash.bytes().as_slice()),
            })
        });
    Ok(Json(json!({
        "market": market.get(),
        "rules_hash": l.rules_hash.to_hex(),
        "rulebook": rulebook,
        "canonical": l.canonical,
        "text": text,
        "schedule": {
            "open": iso(l.open), "close": iso(l.close),
            "release": iso(l.release), "deadline": iso(l.deadline),
        },
        "journal": journal,
    }))
    .into_response())
}
