//! The daily allowance (G10 U2): any account may claim `ALLOWANCE` once per UTC day, by the
//! gateway's clock. One bad trade must not end the game for good, and the same rule applies
//! to everyone (SPEC §1.2). The grant is a journaled command and a ledger entry, so
//! conservation holds; the profit leaderboard counts trading only, so grants never rank.
#![allow(clippy::missing_errors_doc)]

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use brier_engine::{ALLOWANCE, Command, Event};
use serde_json::json;

use crate::clock::Unix;
use crate::error::ApiError;
use crate::routes::{ApiResult, Req, Shared};

const DAY: Unix = 86_400;

/// The UTC day number of `t` (days since 1970-01-01) and the start of the next day.
fn day_of(t: Unix) -> (i64, Unix) {
    let day = t.div_euclid(DAY);
    (day, day.saturating_add(1).saturating_mul(DAY))
}

fn iso(t: Unix) -> serde_json::Value {
    crate::markets::iso(t)
}

/// `GET /v1/allowance`: whether the caller has claimed today, how much a claim gives, and when
/// the next day starts.
pub async fn status(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let (day, next) = day_of(s.clock.now());
    let claimed: bool = s
        .db
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM allowance_claims WHERE user_id = $1 AND day = DATE '1970-01-01' + $2::integer)",
            &[&user.id, &i32::try_from(day).map_err(|e| ApiError::internal(&e))?],
        )
        .await?
        .get(0);
    Ok(Json(json!({
        "amount": ALLOWANCE.cents(), "claimed_today": claimed, "next_at": iso(next),
    }))
    .into_response())
}

/// `POST /v1/allowance`: claim today's allowance. 409 if already claimed today.
pub async fn claim(State(s): Shared, req: Req) -> ApiResult {
    let user = req.user(&s, true).await?;
    let account = user.account()?;
    let (day, next) = day_of(s.clock.now());
    let day = i32::try_from(day).map_err(|e| ApiError::internal(&e))?;
    let fresh = s
        .db
        .execute(
            "INSERT INTO allowance_claims (user_id, day) VALUES ($1, DATE '1970-01-01' + $2::integer) ON CONFLICT DO NOTHING",
            &[&user.id, &day],
        )
        .await?;
    if fresh == 0 {
        return Err(ApiError::conflict(
            "today's allowance is already claimed; the next one is available tomorrow (UTC)",
        ));
    }
    let committed = match s.core.submit(Command::Allowance { account }).await {
        Ok(c) => c,
        Err(e) => {
            release(&s, user.id, day).await?;
            return Err(e.into());
        }
    };
    if !matches!(committed.events.as_slice(), [Event::Entry(_)]) {
        release(&s, user.id, day).await?;
        return Err(ApiError::internal(&format!(
            "allowance answered {:?}",
            committed.events
        )));
    }
    let seq = i64::try_from(committed.seq.get()).map_err(|e| ApiError::internal(&e))?;
    s.db.execute(
        "UPDATE allowance_claims SET seq = $3 WHERE user_id = $1 AND day = DATE '1970-01-01' + $2::integer",
        &[&user.id, &day, &seq],
    )
    .await?;
    Ok(granted(seq, &iso(next)))
}

/// Undo a claim whose grant was not journaled, so the user can try again.
async fn release(s: &crate::AppState, user: i64, day: i32) -> Result<(), ApiError> {
    s.db.execute(
        "DELETE FROM allowance_claims WHERE user_id = $1 AND day = DATE '1970-01-01' + $2::integer AND seq IS NULL",
        &[&user, &day],
    )
    .await?;
    Ok(())
}

fn granted(seq: i64, next: &serde_json::Value) -> Response {
    (
        StatusCode::CREATED,
        Json(json!({ "granted": ALLOWANCE.cents(), "seq": seq, "next_at": next })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_dates() {
        assert_eq!(day_of(0), (0, 86_400));
        assert_eq!(day_of(86_399), (0, 86_400));
        assert_eq!(day_of(1_794_234_600).0, 20_766);
    }
}
