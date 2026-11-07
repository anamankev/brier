//! The resolution workflow (docs/resolution.md): propose, confirm, dispute, rule, settle.
//!
//! 1. A resolver (or an admin) proposes an outcome for a closed market after its release time,
//!    with the evidence record that decided it.
//! 2. An admin other than the proposer confirms it, which opens the dispute window.
//! 3. Any position holder may file one dispute per resolution while the window is open. An
//!    admin other than the proposer rules on it: upholding withdraws the resolution, rejecting
//!    lets it stand.
//! 4. When the window has closed with no open dispute, the settler sends `settle` carrying the
//!    evidence hash ([`crate::scheduler`]).
//!
//! Each step is one conditional SQL statement, so two requests racing on the same resolution
//! cannot both succeed: the loser finds the state already changed and gets `conflict`.
#![allow(clippy::missing_errors_doc)] // handler errors are the documented API responses

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use brier_core::{Answer, Query};
use brier_engine::MarketState;
use brier_rules::canonical::canonical;
use brier_rules::hex;
use brier_rules::time::instant;
use brier_types::MarketId;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

use crate::AppState;
use crate::audit::{self, Actor};
use crate::auth::Role;
use crate::clock::Unix;
use crate::error::ApiError;
use crate::markets::{iso, listing, market_state};
use crate::routes::{ApiResult, Req, Shared, admin_user, parse_id};

/// Largest canonical evidence record accepted. A record names the raw response by its hash, so
/// it stays small.
pub const MAX_EVIDENCE_BYTES: usize = 4 * 1_024;
/// How far an evidence record's retrieval time may be ahead of the gateway's clock.
pub const MAX_CLOCK_SKEW: Unix = 300;

const VOID_CONDITIONS: [&str; 2] = ["no_value_by_deadline", "source_discontinued"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeBody {
    outcome: String,
    #[serde(default)]
    void_condition: Option<String>,
    evidence: Value,
    reason: String,
}

/// Check the evidence record against the market and the proposal, and return its canonical
/// JSON and SHA-256. The gateway checks only what binds the record to this decision; the rest
/// of the record is the resolver's (docs/resolution.md).
fn check_evidence(
    evidence: &Value,
    market: MarketId,
    rules_hash: &str,
    outcome: &str,
    earliest: Unix,
    now: Unix,
) -> Result<(String, [u8; 32]), ApiError> {
    let bad = |m: &str| ApiError::bad_request(format!("evidence: {m}"));
    let text =
        canonical(evidence).map_err(|_| bad("numbers must be integers within ±(2^53 − 1)"))?;
    if text.len() > MAX_EVIDENCE_BYTES {
        return Err(bad("the record is larger than 4 KiB"));
    }
    if evidence.get("market").and_then(Value::as_u64) != Some(u64::from(market.get())) {
        return Err(bad("market does not match"));
    }
    if evidence.get("rules_hash").and_then(Value::as_str) != Some(rules_hash) {
        return Err(bad("rules_hash does not match the market's rulebook"));
    }
    if evidence.get("outcome").and_then(Value::as_str) != Some(outcome) {
        return Err(bad("outcome does not match the proposal"));
    }
    let retrieved = evidence
        .get("retrieved_at")
        .and_then(Value::as_str)
        .and_then(instant)
        .ok_or_else(|| bad("retrieved_at must be YYYY-MM-DDTHH:MM:SSZ"))?;
    if retrieved < earliest {
        return Err(bad(
            "retrieved_at is before the earliest time this outcome can be known",
        ));
    }
    if retrieved > now.saturating_add(MAX_CLOCK_SKEW) {
        return Err(bad("retrieved_at is in the future"));
    }
    Ok((text.clone(), Sha256::digest(text.as_bytes()).into()))
}

/// `POST /v1/markets/{id}/resolution`: propose an outcome (resolver or admin).
pub async fn propose(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let market = MarketId::new(parse_id(&raw, "market")?);
    let user = req.user(&s, true).await?;
    if !matches!(user.role, Role::Resolver | Role::Admin) {
        return Err(ApiError::forbidden_because(
            "proposing an outcome needs the resolver or admin role",
        ));
    }
    let b: ProposeBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let l = listing(&s, market)
        .await?
        .ok_or_else(|| ApiError::not_found("no rulebook for this market"))?;
    if market_state(&s, market).await? != Some(MarketState::Closed) {
        return Err(ApiError::conflict("only a closed market can be resolved"));
    }
    let now = s.clock.now();
    if now < l.release {
        return Err(ApiError::too_early("the release time has not come"));
    }
    if !["yes", "no", "void"].contains(&b.outcome.as_str()) {
        return Err(ApiError::bad_request("outcome must be yes, no, or void"));
    }
    let earliest = match (b.outcome.as_str(), b.void_condition.as_deref()) {
        ("void", Some(c)) => {
            if !VOID_CONDITIONS.contains(&c) || !l.void_conditions().iter().any(|v| v == c) {
                return Err(ApiError::bad_request(
                    "void_condition must be one of the rulebook's void conditions",
                ));
            }
            if c == "no_value_by_deadline" {
                if now < l.deadline {
                    return Err(ApiError::too_early(
                        "the resolution deadline has not passed",
                    ));
                }
                l.deadline
            } else {
                l.release
            }
        }
        ("void", None) => {
            return Err(ApiError::bad_request(
                "a void proposal needs its void_condition",
            ));
        }
        (_, Some(_)) => {
            return Err(ApiError::bad_request(
                "only a void proposal has a void_condition",
            ));
        }
        (_, None) => l.release,
    };
    let rules_hex = l.rules_hash.to_hex();
    let (record, hash) =
        check_evidence(&b.evidence, market, &rules_hex, &b.outcome, earliest, now)?;
    let market_col = i64::from(market.get());
    s.db.execute(
        "INSERT INTO evidence (evidence_hash, market, canonical, submitted_by) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
        &[&hash.as_slice(), &market_col, &record, &user.id],
    )
    .await?;
    let row = s
        .db
        .query_opt(
            "INSERT INTO resolutions (market, outcome, void_condition, evidence_hash, state, proposed_by, proposed_at, proposal_reason) \
             SELECT $1, $2, $3, $4, 'proposed', $5, to_timestamp($6::bigint), $7 \
             WHERE NOT EXISTS (SELECT 1 FROM resolutions WHERE market = $1 AND state <> 'withdrawn') \
             ON CONFLICT DO NOTHING RETURNING id",
            &[
                &market_col,
                &b.outcome,
                &b.void_condition,
                &hash.as_slice(),
                &user.id,
                &now,
                &reason,
            ],
        )
        .await?
        .ok_or_else(|| ApiError::conflict("this market already has a live resolution"))?;
    let id: i64 = row.get(0);
    let evidence_hash = hex(&hash);
    audit::record(
        &s,
        Actor::User(user.id),
        "propose_resolution",
        &json!({ "resolution": id, "market": market.get(), "outcome": b.outcome, "evidence_hash": evidence_hash }),
        reason,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "resolution": id, "market": market.get(), "state": "proposed", "evidence_hash": evidence_hash })),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonBody {
    reason: String,
}

/// The resolution's market, state, and proposer, or 404.
async fn resolution_row(s: &AppState, id: i64) -> Result<(i64, String, i64), ApiError> {
    let r =
        s.db.query_opt(
            "SELECT market, state, proposed_by FROM resolutions WHERE id = $1",
            &[&id],
        )
        .await?
        .ok_or_else(|| ApiError::not_found("no such resolution"))?;
    Ok((r.get(0), r.get(1), r.get(2)))
}

/// `POST /v1/admin/resolutions/{id}/confirm`: confirm a proposal (an admin other than the
/// proposer); opens the dispute window.
pub async fn confirm(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let id: i64 = parse_id(&raw, "resolution")?;
    let user = admin_user(&s, &req).await?;
    let b: ReasonBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let (market, state, proposer) = resolution_row(&s, id).await?;
    if proposer == user.id {
        return Err(ApiError::forbidden_because(
            "the proposer cannot confirm their own proposal (four eyes)",
        ));
    }
    let now = s.clock.now();
    let until = now.saturating_add(s.dispute_window);
    let changed = s
        .db
        .execute(
            "UPDATE resolutions SET state = 'confirmed', confirmed_by = $2, confirmed_at = to_timestamp($3::bigint), dispute_until = to_timestamp($4::bigint) \
             WHERE id = $1 AND state = 'proposed' AND proposed_by <> $2",
            &[&id, &user.id, &now, &until],
        )
        .await?;
    if changed != 1 {
        return Err(ApiError::conflict(format!(
            "only a proposed resolution can be confirmed; this one is {state}"
        )));
    }
    audit::record(
        &s,
        Actor::User(user.id),
        "confirm_resolution",
        &json!({ "resolution": id, "market": market, "dispute_until": iso(until) }),
        reason,
    )
    .await?;
    Ok(
        Json(json!({ "resolution": id, "state": "confirmed", "dispute_until": iso(until) }))
            .into_response(),
    )
}

/// `POST /v1/admin/resolutions/{id}/withdraw`: withdraw a proposed or confirmed resolution
/// before the settler claims it; open disputes on it become moot.
pub async fn withdraw(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let id: i64 = parse_id(&raw, "resolution")?;
    let user = admin_user(&s, &req).await?;
    let b: ReasonBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let (market, state, _) = resolution_row(&s, id).await?;
    let now = s.clock.now();
    let row = s
        .db
        .query_one(
            "WITH r AS (UPDATE resolutions SET state = 'withdrawn', withdrawn_by = $2, withdrawn_at = to_timestamp($3::bigint) \
                        WHERE id = $1 AND state IN ('proposed', 'confirmed') RETURNING id), \
                  m AS (UPDATE disputes SET state = 'moot', ruled_at = to_timestamp($3::bigint) \
                        WHERE resolution IN (SELECT id FROM r) AND state = 'open' RETURNING id) \
             SELECT (SELECT count(*) FROM r), (SELECT count(*) FROM m)",
            &[&id, &user.id, &now],
        )
        .await?;
    if row.get::<_, i64>(0) != 1 {
        return Err(ApiError::conflict(format!(
            "only a proposed or confirmed resolution can be withdrawn; this one is {state}"
        )));
    }
    let moot: i64 = row.get(1);
    audit::record(
        &s,
        Actor::User(user.id),
        "withdraw_resolution",
        &json!({ "resolution": id, "market": market, "disputes_made_moot": moot }),
        reason,
    )
    .await?;
    Ok(Json(json!({ "resolution": id, "state": "withdrawn" })).into_response())
}

/// `POST /v1/resolutions/{id}/disputes`: dispute a confirmed resolution while its window is
/// open (position holders only, once per resolution).
pub async fn dispute(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let id: i64 = parse_id(&raw, "resolution")?;
    let user = req.user(&s, true).await?;
    if !s.order_limits.allow(user.id) {
        return Err(ApiError::rate_limited());
    }
    let b: ReasonBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let (market, state, _) = resolution_row(&s, id).await?;
    if state != "confirmed" {
        // Checked again, with the window, by the insert below.
        return Err(ApiError::conflict(format!(
            "only a confirmed resolution can be disputed; this one is {state}"
        )));
    }
    let market_id = MarketId::new(u32::try_from(market).map_err(|e| ApiError::internal(&e))?);
    let Answer::Account { view, .. } = s.core.query(Query::Account(user.account()?)).await? else {
        return Err(ApiError::internal(&"account query answered another shape"));
    };
    let holds = view.is_some_and(|v| v.positions.iter().any(|&(m, x)| m == market_id && x != 0));
    if !holds {
        return Err(ApiError::forbidden_because(
            "only accounts holding a position in the market can dispute",
        ));
    }
    let now = s.clock.now();
    let row = s
        .db
        .query_opt(
            "INSERT INTO disputes (resolution, user_id, reason, filed_at, state) \
             SELECT $1, $2, $3, to_timestamp($4::bigint), 'open' \
             WHERE EXISTS (SELECT 1 FROM resolutions WHERE id = $1 AND state = 'confirmed' AND dispute_until > to_timestamp($4::bigint)) \
             ON CONFLICT (resolution, user_id) DO NOTHING RETURNING id",
            &[&id, &user.id, &reason, &now],
        )
        .await?;
    let Some(row) = row else {
        let already =
            s.db.query_opt(
                "SELECT 1 FROM disputes WHERE resolution = $1 AND user_id = $2",
                &[&id, &user.id],
            )
            .await?
            .is_some();
        return Err(ApiError::conflict(if already {
            "you have already disputed this resolution".to_owned()
        } else {
            format!(
                "only a confirmed resolution inside its dispute window can be disputed; this one is {state}"
            )
        }));
    };
    let dispute: i64 = row.get(0);
    audit::record(
        &s,
        Actor::User(user.id),
        "file_dispute",
        &json!({ "dispute": dispute, "resolution": id, "market": market }),
        reason,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "dispute": dispute, "resolution": id, "state": "open" })),
    )
        .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleBody {
    decision: String,
    reason: String,
}

/// `POST /v1/admin/disputes/{id}/rule`: uphold a dispute (the resolution is withdrawn and a new
/// proposal starts over) or reject it (the resolution stands). An admin other than the proposer.
pub async fn rule(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let id: i64 = parse_id(&raw, "dispute")?;
    let user = admin_user(&s, &req).await?;
    let b: RuleBody = req.json()?;
    let reason = audit::reason(&b.reason)?;
    let r = s
        .db
        .query_opt(
            "SELECT d.resolution, r.proposed_by, r.market FROM disputes d JOIN resolutions r ON r.id = d.resolution WHERE d.id = $1",
            &[&id],
        )
        .await?
        .ok_or_else(|| ApiError::not_found("no such dispute"))?;
    let (resolution, proposer, market): (i64, i64, i64) = (r.get(0), r.get(1), r.get(2));
    if proposer == user.id {
        return Err(ApiError::forbidden_because(
            "the proposer cannot rule on a dispute of their own proposal",
        ));
    }
    let now = s.clock.now();
    let decided = match b.decision.as_str() {
        "reject" => {
            s.db.execute(
                "UPDATE disputes SET state = 'rejected', ruled_by = $2, ruled_at = to_timestamp($3::bigint), ruling_reason = $4 WHERE id = $1 AND state = 'open'",
                &[&id, &user.id, &now, &reason],
            )
            .await?
                == 1
        }
        "uphold" => {
            let row = s
                .db
                .query_one(
                    "WITH d AS (UPDATE disputes SET state = 'upheld', ruled_by = $2, ruled_at = to_timestamp($3::bigint), ruling_reason = $4 \
                                WHERE id = $1 AND state = 'open' RETURNING resolution), \
                          r AS (UPDATE resolutions SET state = 'withdrawn', withdrawn_at = to_timestamp($3::bigint) \
                                WHERE id IN (SELECT resolution FROM d) AND state = 'confirmed' RETURNING id), \
                          m AS (UPDATE disputes SET state = 'moot', ruled_at = to_timestamp($3::bigint) \
                                WHERE resolution IN (SELECT id FROM r) AND state = 'open' AND id <> $1 RETURNING id) \
                     SELECT (SELECT count(*) FROM d), (SELECT count(*) FROM r)",
                    &[&id, &user.id, &now, &reason],
                )
                .await?;
            row.get::<_, i64>(0) == 1
        }
        _ => return Err(ApiError::bad_request("decision must be uphold or reject")),
    };
    if !decided {
        return Err(ApiError::conflict("only an open dispute can be ruled on"));
    }
    audit::record(
        &s,
        Actor::User(user.id),
        "rule_dispute",
        &json!({ "dispute": id, "resolution": resolution, "market": market, "decision": b.decision }),
        reason,
    )
    .await?;
    let state = if b.decision == "uphold" {
        "upheld"
    } else {
        "rejected"
    };
    Ok(Json(json!({ "dispute": id, "resolution": resolution, "state": state })).into_response())
}

/// `GET /v1/markets/{id}/resolution`: every resolution of the market, newest first, with its
/// evidence record and dispute counts. Who proposed, confirmed, or disputed is not shown.
pub async fn get(State(s): Shared, Path(raw): Path<String>) -> ApiResult {
    let market = MarketId::new(parse_id(&raw, "market")?);
    let rows = s
        .db
        .query(
            "SELECT r.id, r.outcome, r.void_condition, r.evidence_hash, r.state, \
                    extract(epoch FROM r.proposed_at)::bigint, extract(epoch FROM r.confirmed_at)::bigint, \
                    extract(epoch FROM r.dispute_until)::bigint, r.settled_seq, e.canonical, \
                    (SELECT count(*) FROM disputes d WHERE d.resolution = r.id AND d.state = 'open'), \
                    (SELECT count(*) FROM disputes d WHERE d.resolution = r.id) \
             FROM resolutions r JOIN evidence e ON e.evidence_hash = r.evidence_hash \
             WHERE r.market = $1 ORDER BY r.id DESC",
            &[&i64::from(market.get())],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            let evidence: Value = serde_json::from_str(r.get(9)).unwrap_or(Value::Null);
            let when = |i: usize| r.get::<_, Option<i64>>(i).map_or(Value::Null, iso);
            json!({
                "resolution": r.get::<_, i64>(0),
                "outcome": r.get::<_, &str>(1),
                "void_condition": r.get::<_, Option<&str>>(2),
                "evidence_hash": hex(r.get::<_, &[u8]>(3)),
                "state": r.get::<_, &str>(4),
                "proposed_at": when(5),
                "confirmed_at": when(6),
                "dispute_until": when(7),
                "settled_seq": r.get::<_, Option<i64>>(8),
                "evidence": evidence,
                "disputes": { "open": r.get::<_, i64>(10), "total": r.get::<_, i64>(11) },
            })
        })
        .collect();
    // What the journal itself recorded (the `market_journal` read model), so anyone can check
    // that the settled outcome and evidence hash are the resolution's.
    let journal = s
        .db
        .query_opt(
            "SELECT settled_seq, outcome, evidence_hash FROM market_journal WHERE market = $1 AND settled_seq IS NOT NULL",
            &[&i64::from(market.get())],
        )
        .await?
        .map(|r| {
            json!({
                "settled_seq": r.get::<_, i64>(0),
                "outcome": r.get::<_, &str>(1),
                "evidence_hash": r.get::<_, Option<&[u8]>>(2).map(hex),
            })
        });
    Ok(
        Json(json!({ "market": market.get(), "resolutions": items, "journal": journal }))
            .into_response(),
    )
}
