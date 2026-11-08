//! The social layer (G10 U3): public profiles, who holds each market, recent trades with who
//! made them, and each user's notifications. Everything here is read from the read models, so
//! it reaches as far as the projector (`as_of`). Usernames and trades are public, as on any
//! exchange (the privacy notice says so); a deleted account shows as `deleted_N`.
#![allow(clippy::missing_errors_doc)]

use std::sync::atomic::Ordering;

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::ApiError;
use crate::routes::{ApiResult, Req, Shared, parse_id};

/// Holders listed per side of a market.
const HOLDERS: i64 = 10;
/// Trades on a profile and in a market's activity feed when the client does not say.
const RECENT: i64 = 30;
/// Most trades a client may ask for at once.
const MAX_RECENT: i64 = 200;
/// Notifications returned at once: a page of recent ones is all a bell shows.
const NOTIFICATIONS: i64 = 40;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limit {
    #[serde(default)]
    limit: Option<i64>,
}

fn limit(req: &Req) -> Result<i64, ApiError> {
    let Limit { limit } = req.query()?;
    let n = limit.unwrap_or(RECENT);
    if (1..=MAX_RECENT).contains(&n) {
        Ok(n)
    } else {
        Err(ApiError::bad_request(format!(
            "limit must be 1 to {MAX_RECENT}"
        )))
    }
}

/// Signed contract legs per fill: + is YES bought, − is YES sold (NO bought), with the cash
/// each leg paid (−) or received (+), for both sides of every trade.
const LEGS: &str = "SELECT market, taker_account AS account, \
       CASE WHEN taker_side = 'buy' THEN qty::bigint ELSE -qty::bigint END AS x, \
       CASE WHEN taker_side = 'buy' THEN -price::bigint * qty ELSE price::bigint * qty END AS cash FROM fills \
     UNION ALL \
     SELECT market, maker_account, \
       CASE WHEN taker_side = 'buy' THEN -qty::bigint ELSE qty::bigint END, \
       CASE WHEN taker_side = 'buy' THEN price::bigint * qty ELSE -price::bigint * qty END FROM fills";

const ISO: &str = r#"'YYYY-MM-DD"T"HH24:MI:SS"Z"'"#;

/// `GET /v1/users/{name}`: a public profile: when the user joined, their trading and
/// forecasting record, open positions, and recent trades.
pub async fn profile(State(s): Shared, Path(name): Path<String>, req: Req) -> ApiResult {
    let n = limit(&req)?;
    let user = s
        .db
        .query_opt(
            &format!("SELECT id, to_char(created_at AT TIME ZONE 'UTC', {ISO}), role FROM users WHERE username = $1 AND deleted_at IS NULL"),
            &[&name],
        )
        .await?
        .ok_or_else(|| ApiError::not_found("no such user"))?;
    let id: i64 = user.get(0);
    let as_of = s.projected.load(Ordering::Acquire);

    let record = s
        .db
        .query_one(
            &format!(
                "WITH legs AS ({LEGS}), per AS ( \
                   SELECT l.market, sum(l.x) AS x, sum(l.cash) AS cash, j.outcome \
                   FROM legs l LEFT JOIN market_journal j ON j.market = l.market \
                   WHERE l.account = $1 GROUP BY l.market, j.outcome) \
                 SELECT count(*) FILTER (WHERE outcome IS NOT NULL)::bigint, \
                        coalesce(sum(cash + x * CASE outcome WHEN 'yes' THEN 100 WHEN 'no' THEN 0 ELSE 50 END) FILTER (WHERE outcome IS NOT NULL), 0)::bigint, \
                        count(*)::bigint, \
                        (SELECT coalesce(sum(qty), 0)::bigint FROM fills WHERE maker_account = $1 OR taker_account = $1), \
                        (SELECT count(*)::bigint FROM scores WHERE user_id = $1), \
                        (SELECT CASE WHEN count(*) = 0 THEN NULL ELSE round(avg(skill))::bigint END FROM scores WHERE user_id = $1) \
                 FROM per"
            ),
            &[&id],
        )
        .await?;

    let positions = s
        .db
        .query(
            &format!(
                "WITH legs AS ({LEGS}) \
                 SELECT l.market, sum(l.x)::bigint AS x, sum(l.cash)::bigint, r.event_slug, r.outcome, r.canonical::jsonb->>'title' \
                 FROM legs l JOIN rulebooks r ON r.market = l.market \
                 LEFT JOIN market_journal j ON j.market = l.market \
                 WHERE l.account = $1 AND j.settled_seq IS NULL \
                 GROUP BY l.market, r.event_slug, r.outcome, r.canonical HAVING sum(l.x) <> 0 \
                 ORDER BY abs(sum(l.x)) DESC LIMIT 100"
            ),
            &[&id],
        )
        .await?
        .iter()
        .map(|r| {
            json!({
                "market": r.get::<_, i64>(0), "position": r.get::<_, i64>(1), "cash": r.get::<_, i64>(2),
                "event": r.get::<_, Option<&str>>(3), "outcome": r.get::<_, Option<&str>>(4),
                "title": r.get::<_, Option<&str>>(5),
            })
        })
        .collect::<Vec<_>>();

    let trades = s
        .db
        .query(
            &format!(
                "SELECT f.seq, f.market, CASE WHEN f.taker_account = $1 THEN f.taker_side \
                        ELSE CASE f.taker_side WHEN 'buy' THEN 'sell' ELSE 'buy' END END, \
                        f.price, f.qty, to_char(ft.at AT TIME ZONE 'UTC', {ISO}), r.event_slug, r.outcome, r.canonical::jsonb->>'title' \
                 FROM fills f LEFT JOIN rulebooks r ON r.market = f.market LEFT JOIN fill_times ft ON ft.seq = f.seq \
                 WHERE f.maker_account = $1 OR f.taker_account = $1 \
                 ORDER BY f.seq DESC, f.ordinal DESC LIMIT $2"
            ),
            &[&id, &n],
        )
        .await?
        .iter()
        .map(trade_json)
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "as_of": as_of,
        "username": name,
        "joined": user.get::<_, Option<String>>(1),
        "role": user.get::<_, &str>(2),
        "settled_markets": record.get::<_, i64>(0),
        "profit": record.get::<_, i64>(1),
        "markets_traded": record.get::<_, i64>(2),
        "volume": record.get::<_, i64>(3),
        "scored_markets": record.get::<_, i64>(4),
        "average_skill": record.get::<_, Option<i64>>(5),
        "positions": positions,
        "trades": trades,
    }))
    .into_response())
}

fn trade_json(r: &tokio_postgres::Row) -> Value {
    json!({
        "seq": r.get::<_, i64>(0), "market": r.get::<_, i64>(1), "side": r.get::<_, &str>(2),
        "price": r.get::<_, i16>(3), "qty": r.get::<_, i32>(4), "at": r.get::<_, Option<String>>(5),
        "event": r.get::<_, Option<&str>>(6), "outcome": r.get::<_, Option<&str>>(7),
        "title": r.get::<_, Option<&str>>(8),
    })
}

/// `GET /v1/markets/{id}/holders`: the largest YES and NO positions in the market.
pub async fn holders(State(s): Shared, Path(raw): Path<String>) -> ApiResult {
    let market = i64::from(parse_id::<u32>(&raw, "market")?);
    let as_of = s.projected.load(Ordering::Acquire);
    let rows = s
        .db
        .query(
            &format!(
                "WITH legs AS ({LEGS}), pos AS ( \
                   SELECT account, sum(x)::bigint AS x FROM legs WHERE market = $1 GROUP BY account HAVING sum(x) <> 0) \
                 (SELECT u.username, p.x FROM pos p JOIN users u ON u.id = p.account WHERE p.x > 0 ORDER BY p.x DESC, u.username LIMIT $2) \
                 UNION ALL \
                 (SELECT u.username, p.x FROM pos p JOIN users u ON u.id = p.account WHERE p.x < 0 ORDER BY p.x, u.username LIMIT $2)"
            ),
            &[&market, &HOLDERS],
        )
        .await?;
    let side = |yes: bool| {
        rows.iter()
            .filter(|r| (r.get::<_, i64>(1) > 0) == yes)
            .map(|r| json!({ "username": r.get::<_, &str>(0), "contracts": r.get::<_, i64>(1).saturating_abs() }))
            .collect::<Vec<_>>()
    };
    Ok(
        Json(json!({ "as_of": as_of, "market": market, "yes": side(true), "no": side(false) }))
            .into_response(),
    )
}

/// `GET /v1/markets/{id}/activity`: the market's recent trades with the taker's username,
/// newest first.
pub async fn activity(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let market = i64::from(parse_id::<u32>(&raw, "market")?);
    let n = limit(&req)?;
    let as_of = s.projected.load(Ordering::Acquire);
    let rows = s
        .db
        .query(
            &format!(
                "SELECT f.seq, u.username, f.taker_side, f.price, f.qty, to_char(ft.at AT TIME ZONE 'UTC', {ISO}) \
                 FROM fills f JOIN users u ON u.id = f.taker_account LEFT JOIN fill_times ft ON ft.seq = f.seq \
                 WHERE f.market = $1 ORDER BY f.seq DESC, f.ordinal DESC LIMIT $2"
            ),
            &[&market, &n],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "seq": r.get::<_, i64>(0), "username": r.get::<_, &str>(1), "side": r.get::<_, &str>(2),
                "price": r.get::<_, i16>(3), "qty": r.get::<_, i32>(4), "at": r.get::<_, Option<String>>(5),
            })
        })
        .collect();
    Ok(Json(json!({ "as_of": as_of, "market": market, "trades": items })).into_response())
}

/// `GET /v1/notifications`: the caller's recent fills, settlements of markets they traded,
/// and allowance grants, newest first, with how many are unread.
pub async fn notifications(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let as_of = s.projected.load(Ordering::Acquire);
    let rows = s
        .db
        .query(
            &format!(
                "WITH mine AS ( \
                   SELECT f.seq, 'fill' AS kind, f.market, \
                          CASE WHEN f.taker_account = $1 THEN f.taker_side ELSE CASE f.taker_side WHEN 'buy' THEN 'sell' ELSE 'buy' END END AS side, \
                          f.price::bigint AS price, f.qty::bigint AS qty, NULL::text AS outcome, (SELECT to_char(at AT TIME ZONE 'UTC', {ISO}) FROM fill_times WHERE seq = f.seq) AS at \
                   FROM fills f WHERE f.maker_account = $1 OR f.taker_account = $1 \
                   UNION ALL \
                   SELECT j.settled_seq, 'settled', j.market, NULL, NULL, \
                          (SELECT sum(CASE WHEN taker_account = $1 THEN CASE taker_side WHEN 'buy' THEN qty ELSE -qty END \
                                           ELSE CASE taker_side WHEN 'buy' THEN -qty ELSE qty END END)::bigint \
                           FROM fills WHERE market = j.market AND (maker_account = $1 OR taker_account = $1)), \
                          j.outcome, NULL \
                   FROM market_journal j WHERE j.settled_seq IS NOT NULL \
                     AND EXISTS (SELECT 1 FROM fills WHERE market = j.market AND (maker_account = $1 OR taker_account = $1)) \
                   UNION ALL \
                   SELECT a.seq, 'allowance', NULL, NULL, NULL, NULL, NULL, NULL FROM allowance_claims a \
                   WHERE a.user_id = $1 AND a.seq IS NOT NULL) \
                 SELECT m.seq, m.kind, m.market, m.side, m.price, m.qty, m.outcome, m.at, \
                        r.event_slug, r.outcome, r.canonical::jsonb->>'title' \
                 FROM mine m LEFT JOIN rulebooks r ON r.market = m.market \
                 ORDER BY m.seq DESC LIMIT $2"
            ),
            &[&user.id, &NOTIFICATIONS],
        )
        .await?;
    let seen: i64 =
        s.db.query_one("SELECT seen_seq FROM users WHERE id = $1", &[&user.id])
            .await?
            .get(0);
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "seq": r.get::<_, i64>(0), "kind": r.get::<_, &str>(1), "market": r.get::<_, Option<i64>>(2),
                "side": r.get::<_, Option<&str>>(3), "price": r.get::<_, Option<i64>>(4),
                "qty": r.get::<_, Option<i64>>(5), "result": r.get::<_, Option<&str>>(6),
                "at": r.get::<_, Option<String>>(7), "event": r.get::<_, Option<&str>>(8),
                "outcome": r.get::<_, Option<&str>>(9), "title": r.get::<_, Option<&str>>(10),
                "unread": r.get::<_, i64>(0) > seen,
            })
        })
        .collect();
    let unread = items.iter().filter(|v| v["unread"] == true).count();
    Ok(
        Json(json!({ "as_of": as_of, "seen_seq": seen, "unread": unread, "items": items }))
            .into_response(),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seen {
    seq: i64,
}

/// `POST /v1/notifications/seen`: mark everything up to `seq` as read (never moves back).
pub async fn seen(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let Seen { seq } = req.json()?;
    if seq < 0 {
        return Err(ApiError::bad_request("seq must be non-negative"));
    }
    s.db.execute(
        "UPDATE users SET seen_seq = greatest(seen_seq, $2) WHERE id = $1",
        &[&user.id, &seq],
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT.into_response())
}
