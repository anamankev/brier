//! Forecasts and forecaster scores (SPEC §7, docs/gates/G6-plan.md).
//!
//! Any user may record a probability (1 to 99 percent) on an open market and change it until
//! close; forecasts are append-only and timestamped by the server. Once a minute the scheduler
//! samples each open market's price ([`sample`]). When a market settles YES or NO, every
//! forecaster is scored over those samples ([`score`]):
//! - Brier score, in units of 1/10,000: (p − 100·outcome)², with p in percent;
//! - skill per sample: the market's score minus the user's, and 0 before the user's first
//!   forecast, so forecasting late earns nothing extra;
//! - skill: the average over every sample, rounded half away from zero.
//!
//! Void markets are not scored. Leaderboards rank average skill over at least
//! [`MIN_SCORED_MARKETS`] markets, and settled trading profit.
#![allow(clippy::missing_errors_doc)] // handler errors are the documented API responses

use axum::Json;
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use brier_core::{Answer, Query};
use brier_engine::MarketState;
use brier_types::MarketId;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::AppState;
use crate::clock::Unix;
use crate::error::ApiError;
use crate::markets::{iso, listing, market_state};
use crate::routes::{ApiResult, Req, Shared, parse_id};

/// Scored markets a user needs before appearing on the skill leaderboard (SPEC §7).
pub const MIN_SCORED_MARKETS: i64 = 10;
/// Seconds between price samples.
pub const SAMPLE_EVERY: Unix = 60;
/// Rows on a leaderboard.
pub const LEADERBOARD_ROWS: i64 = 100;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForecastBody {
    probability: u8,
}

/// `PUT /v1/markets/{id}/forecast`: record a probability, 1 to 99 percent.
pub async fn put(State(s): Shared, Path(raw): Path<String>, req: Req) -> ApiResult {
    let market = MarketId::new(parse_id(&raw, "market")?);
    let user = req.user(&s, true).await?;
    if !user.role.trades() {
        return Err(ApiError::forbidden_because(
            "resolver and surveillance accounts do not forecast",
        ));
    }
    if !s.order_limits.allow(user.id) {
        return Err(ApiError::rate_limited());
    }
    let ForecastBody { probability } = req.json()?;
    if !(1..=99).contains(&probability) {
        return Err(ApiError::bad_request("probability must be 1 to 99 percent"));
    }
    let l = listing(&s, market)
        .await?
        .ok_or_else(|| ApiError::not_found("no rulebook for this market"))?;
    let now = s.clock.now();
    let open = matches!(
        market_state(&s, market).await?,
        Some(MarketState::Open | MarketState::Halted)
    );
    if !open || now >= l.close {
        return Err(ApiError::conflict(
            "forecasts are taken only while the market is open, until its close time",
        ));
    }
    s.db.execute(
        "INSERT INTO forecasts (user_id, market, probability, at) VALUES ($1, $2, $3, to_timestamp($4::bigint))",
        &[&user.id, &i64::from(market.get()), &i16::from(probability), &now],
    )
    .await?;
    Ok(
        Json(json!({ "market": market.get(), "probability": probability, "at": iso(now) }))
            .into_response(),
    )
}

/// `GET /v1/forecasts`: the caller's forecasts, newest first.
pub async fn mine(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let rows = s
        .db
        .query(
            "SELECT market, probability, extract(epoch FROM at)::bigint FROM forecasts WHERE user_id = $1 ORDER BY at DESC, id DESC LIMIT 1000",
            &[&user.id],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| json!({ "market": r.get::<_, i64>(0), "probability": r.get::<_, i16>(1), "at": iso(r.get(2)) }))
        .collect();
    Ok(Json(json!({ "forecasts": items })).into_response())
}

/// The market's price for scoring, in percent: the last trade; before the first trade, the
/// book midpoint (rounded half up); one side alone if the other is empty; 50 if both are.
async fn price(s: &AppState, market: MarketId) -> Result<i16, ApiError> {
    let last =
        s.db.query_opt(
            "SELECT price FROM fills WHERE market = $1 ORDER BY seq DESC, ordinal DESC LIMIT 1",
            &[&i64::from(market.get())],
        )
        .await?;
    if let Some(r) = last {
        return Ok(r.get(0));
    }
    let Answer::Book { bids, asks, .. } = s.core.query(Query::Book(market)).await? else {
        return Err(ApiError::internal(&"book query answered another shape"));
    };
    let best =
        |levels: &[(brier_types::Price, u64)]| levels.first().map(|(p, _)| i16::from(p.ticks()));
    Ok(match (best(&bids), best(&asks)) {
        // bid + ask is at most 197 and at least 3, so the arithmetic cannot overflow.
        (Some(b), Some(a)) => b.saturating_add(a).saturating_add(1) / 2,
        (Some(p), None) | (None, Some(p)) => p,
        (None, None) => 50,
    })
}

/// Record this minute's price for every market inside its trading window. Returns how many
/// samples were written (a minute already sampled is skipped).
pub async fn sample(s: &AppState) -> Result<usize, ApiError> {
    let now = s.clock.now();
    let at = now.saturating_sub(now.rem_euclid(SAMPLE_EVERY));
    let rows = s
        .db
        .query(
            "SELECT market FROM rulebooks WHERE open_at <= to_timestamp($1::bigint) AND close_at > to_timestamp($1::bigint) ORDER BY market",
            &[&now],
        )
        .await?;
    let mut written = 0usize;
    for r in rows {
        let id: i64 = r.get(0);
        let market = MarketId::new(u32::try_from(id).map_err(|e| ApiError::internal(&e))?);
        if !matches!(
            market_state(s, market).await?,
            Some(MarketState::Open | MarketState::Halted)
        ) {
            continue;
        }
        let p = price(s, market).await?;
        written = written.saturating_add(usize::try_from(
            s.db.execute(
                "INSERT INTO score_samples (market, at, price, volume) \
                 VALUES ($1, to_timestamp($2::bigint), $3, (SELECT coalesce(sum(qty), 0)::bigint FROM fills WHERE market = $1)) \
                 ON CONFLICT DO NOTHING",
                &[&id, &at, &p],
            )
            .await?,
        ).unwrap_or(0));
    }
    Ok(written)
}

/// `round(sum / n)`, halves away from zero, in SQL on bigints.
fn round(sum: &str, n: &str) -> String {
    format!("(sign({sum}) * ((abs({sum}) * 2 + {n}) / (2 * {n})))::bigint")
}

/// Score every market that has settled YES or NO and is not yet scored. Returns how many
/// markets were scored.
pub async fn score(s: &AppState) -> Result<usize, ApiError> {
    let due = s
        .db
        .query(
            "SELECT j.market, j.outcome FROM market_journal j JOIN rulebooks b ON b.market = j.market \
             WHERE j.outcome IN ('yes', 'no') AND NOT EXISTS (SELECT 1 FROM scored_markets m WHERE m.market = j.market) \
             ORDER BY j.market",
            &[],
        )
        .await?;
    let now = s.clock.now();
    for r in &due {
        let market: i64 = r.get(0);
        let outcome: &str = r.get(1);
        let hundred: i32 = if outcome == "yes" { 100 } else { 0 };
        let sql = format!(
            "WITH smp AS (SELECT at, price::int AS price FROM score_samples WHERE market = $1), \
                  n AS (SELECT count(*)::bigint AS n FROM smp), \
                  who AS (SELECT user_id, count(*)::int AS forecasts FROM forecasts WHERE market = $1 GROUP BY user_id), \
                  standing AS ( \
                    SELECT who.user_id, smp.price, \
                           (SELECT f.probability::int FROM forecasts f WHERE f.market = $1 AND f.user_id = who.user_id AND f.at <= smp.at \
                            ORDER BY f.at DESC, f.id DESC LIMIT 1) AS p \
                    FROM who CROSS JOIN smp), \
                  per AS ( \
                    SELECT user_id, \
                           sum(CASE WHEN p IS NULL THEN 0 ELSE (price - $2) * (price - $2) - (p - $2) * (p - $2) END)::bigint AS skill_sum, \
                           sum(CASE WHEN p IS NULL THEN 0 ELSE (p - $2) * (p - $2) END)::bigint AS user_sum, \
                           count(p)::bigint AS covered \
                    FROM standing GROUP BY user_id), \
                  mkt AS (SELECT sum((price - $2) * (price - $2))::bigint AS market_sum FROM smp), \
                  ins AS ( \
                    INSERT INTO scores (market, user_id, outcome, skill, user_brier, market_brier, samples, forecasts) \
                    SELECT $1, per.user_id, $3, {skill}, {user}, {mkt}, n.n, who.forecasts \
                    FROM per JOIN who USING (user_id) CROSS JOIN n CROSS JOIN mkt WHERE per.covered > 0 AND n.n > 0 \
                    RETURNING 1) \
             INSERT INTO scored_markets (market, outcome, samples, scored_at) \
             SELECT $1, $3, n.n, to_timestamp($4::bigint) FROM n",
            skill = round("per.skill_sum", "n.n"),
            user = round("per.user_sum", "per.covered"),
            mkt = round("mkt.market_sum", "n.n"),
        );
        s.db.execute(&sql, &[&market, &hundred, &outcome, &now])
            .await?;
    }
    Ok(due.len())
}

/// `GET /v1/scorecard`: the caller's score on every scored market, and the average.
pub async fn scorecard(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let rows = s
        .db
        .query(
            "SELECT s.market, b.canonical::jsonb ->> 'title', s.outcome, s.skill, s.user_brier, s.market_brier, s.samples, s.forecasts \
             FROM scores s JOIN rulebooks b ON b.market = s.market WHERE s.user_id = $1 ORDER BY s.market",
            &[&user.id],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "market": r.get::<_, i64>(0), "title": r.get::<_, Option<&str>>(1),
                "outcome": r.get::<_, &str>(2), "skill": r.get::<_, i32>(3),
                "brier": r.get::<_, i32>(4), "market_brier": r.get::<_, i32>(5),
                "samples": r.get::<_, i32>(6), "forecasts": r.get::<_, i32>(7),
            })
        })
        .collect();
    let total: i64 = items.iter().filter_map(|v| v["skill"].as_i64()).sum();
    let n = i64::try_from(items.len()).map_err(|e| ApiError::internal(&e))?;
    let average = (n > 0).then(|| {
        // Halves away from zero, as in the scores themselves.
        total.signum().saturating_mul(
            total
                .saturating_abs()
                .saturating_mul(2)
                .saturating_add(n)
                .checked_div(n.saturating_mul(2))
                .unwrap_or(0),
        )
    });
    Ok(Json(json!({
        "units": "1/10000", "markets": items, "average_skill": average,
        "ranked": n >= MIN_SCORED_MARKETS, "min_scored_markets": MIN_SCORED_MARKETS,
    }))
    .into_response())
}

/// `GET /v1/leaderboards/skill`: average skill, at least [`MIN_SCORED_MARKETS`] markets.
pub async fn skill_leaderboard(State(s): Shared) -> ApiResult {
    let sql = format!(
        "SELECT u.username, count(*)::bigint AS markets, {avg} AS skill \
         FROM scores s JOIN users u ON u.id = s.user_id GROUP BY u.username \
         HAVING count(*) >= $1 ORDER BY skill DESC, markets DESC, u.username LIMIT $2",
        avg = round("sum(s.skill)::bigint", "count(*)::bigint"),
    );
    let rows =
        s.db.query(&sql, &[&MIN_SCORED_MARKETS, &LEADERBOARD_ROWS])
            .await?;
    let items: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| json!({ "rank": i.saturating_add(1), "username": r.get::<_, &str>(0), "markets": r.get::<_, i64>(1), "skill": r.get::<_, i64>(2) }))
        .collect();
    Ok(Json(
        json!({ "units": "1/10000", "min_scored_markets": MIN_SCORED_MARKETS, "leaders": items }),
    )
    .into_response())
}

/// `GET /v1/leaderboards/profit`: settled trading profit in credits (cents). Per market: the
/// signed trade cash (−p·q bought, +p·q sold) plus the final position times 100 (YES), 0 (NO),
/// or 50 (void), from the fills read model and the journaled outcomes.
pub async fn profit_leaderboard(State(s): Shared) -> ApiResult {
    let rows = s
        .db
        .query(
            "WITH legs AS ( \
               SELECT market, taker_account AS account, \
                      CASE WHEN taker_side = 'buy' THEN -price::bigint * qty ELSE price::bigint * qty END AS cash, \
                      CASE WHEN taker_side = 'buy' THEN qty::bigint ELSE -qty::bigint END AS x FROM fills \
               UNION ALL \
               SELECT market, maker_account, \
                      CASE WHEN taker_side = 'buy' THEN price::bigint * qty ELSE -price::bigint * qty END, \
                      CASE WHEN taker_side = 'buy' THEN -qty::bigint ELSE qty::bigint END FROM fills), \
             per AS ( \
               SELECT l.account, l.market, sum(l.cash) AS cash, sum(l.x) AS x, j.outcome \
               FROM legs l JOIN market_journal j ON j.market = l.market AND j.settled_seq IS NOT NULL \
               GROUP BY l.account, l.market, j.outcome) \
             SELECT u.username, count(*)::bigint, \
                    sum(per.cash + per.x * CASE per.outcome WHEN 'yes' THEN 100 WHEN 'no' THEN 0 ELSE 50 END)::bigint AS profit \
             FROM per JOIN users u ON u.id = per.account \
             GROUP BY u.username ORDER BY profit DESC, u.username LIMIT $1",
            &[&LEADERBOARD_ROWS],
        )
        .await?;
    let items: Vec<Value> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| json!({ "rank": i.saturating_add(1), "username": r.get::<_, &str>(0), "markets": r.get::<_, i64>(1), "profit": r.get::<_, i64>(2) }))
        .collect();
    Ok(Json(json!({ "units": "credits", "leaders": items })).into_response())
}

/// `GET /v1/markets/{id}/prices`: the market's price once a minute while it was open (the
/// scoring samples), oldest first, for price charts. At most 10,000 points: the most recent.
pub async fn prices(State(s): Shared, Path(raw): Path<String>) -> ApiResult {
    let market = MarketId::new(parse_id(&raw, "market")?);
    let rows =
        s.db.query(
            "SELECT extract(epoch FROM at)::bigint, price FROM ( \
               SELECT at, price FROM score_samples WHERE market = $1 ORDER BY at DESC LIMIT 10000 \
             ) recent ORDER BY at",
            &[&i64::from(market.get())],
        )
        .await?;
    let points: Vec<Value> = rows
        .iter()
        .map(|r| json!([iso(r.get(0)), r.get::<_, i16>(1)]))
        .collect();
    Ok(
        Json(json!({ "market": market.get(), "every_seconds": SAMPLE_EVERY, "points": points }))
            .into_response(),
    )
}
