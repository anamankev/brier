//! The gateway's own commands, sent once a second ([`run`]) and audited as actor `system`:
//! - **Schedule.** Open a listed market at its rulebook's open time and close it at its close
//!   time. After downtime it catches up: a market past its close time is closed at once,
//!   straight from `pre_open` if it never opened, so nobody trades after the close time.
//! - **Settle.** A confirmed resolution whose dispute window has ended with no open dispute is
//!   claimed (`settling`), then settled with its evidence hash. A claimed resolution is
//!   finished after a crash: if the journal already holds the settlement, the row is completed
//!   from the `market_journal` read model; otherwise `settle` is sent.
//!
//! [`tick`] does one pass, so tests can drive it with a manual clock.

use std::sync::Arc;
use std::time::Duration;

use brier_core::{Answer, Query};
use brier_engine::{Command, Event, MarketState};
use brier_types::{Digest, MarketId, Outcome};
use brier_wire::outcome_from_name;
use serde_json::json;

use crate::AppState;
use crate::audit::{self, Actor};
use crate::error::ApiError;
use crate::markets::iso;

/// How often [`run`] calls [`tick`].
pub const TICK: Duration = Duration::from_secs(1);

/// What one [`tick`] did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickReport {
    /// Markets opened.
    pub opened: usize,
    /// Markets closed.
    pub closed: usize,
    /// Resolutions settled.
    pub settled: usize,
}

/// One pass: open and close markets by their schedules, then settle what is due.
///
/// # Errors
/// [`ApiError`] if the database or the core fails; the next tick retries.
pub async fn tick(s: &AppState) -> Result<TickReport, ApiError> {
    let mut report = schedule(s).await?;
    report.settled = settle(s).await?;
    Ok(report)
}

/// Call [`tick`] every [`TICK`] until shutdown. Failures are logged and retried.
pub async fn run(state: Arc<AppState>) {
    let mut shutdown = state.shutdown.subscribe();
    loop {
        if let Err(e) = tick(&state).await {
            crate::log("error", "scheduler tick failed; retrying", &e.body());
        }
        tokio::select! {
            () = tokio::time::sleep(TICK) => {}
            _ = shutdown.wait_for(|stop| *stop) => return,
        }
    }
}

async fn schedule(s: &AppState) -> Result<TickReport, ApiError> {
    let now = s.clock.now();
    let Answer::Markets { markets, .. } = s.core.query(Query::Markets).await? else {
        return Err(ApiError::internal(&"markets query answered another shape"));
    };
    let rows = s
        .db
        .query(
            "SELECT market, extract(epoch FROM open_at)::bigint, extract(epoch FROM close_at)::bigint FROM rulebooks ORDER BY market",
            &[],
        )
        .await?;
    let mut report = TickReport::default();
    for r in rows {
        let market =
            MarketId::new(u32::try_from(r.get::<_, i64>(0)).map_err(|e| ApiError::internal(&e))?);
        let (open, close): (i64, i64) = (r.get(1), r.get(2));
        let Some(&(_, state)) = markets.iter().find(|(m, _)| *m == market) else {
            continue; // stored, but the listing command has not landed (a retry will send it)
        };
        let (target, action) = match state {
            MarketState::PreOpen | MarketState::Open | MarketState::Halted if now >= close => {
                (MarketState::Closed, "schedule_close")
            }
            MarketState::PreOpen if now >= open => (MarketState::Open, "schedule_open"),
            _ => continue,
        };
        let detail = json!({ "market": market.get(), "state": brier_wire::state_name(target), "at": iso(now) });
        let done = audit::submit(
            s,
            Actor::System,
            action,
            Command::SetState {
                market,
                state: target,
            },
            detail,
            "rulebook schedule",
        )
        .await?;
        if matches!(
            done.committed.events.first(),
            Some(Event::MarketStateChanged { .. })
        ) {
            if target == MarketState::Open {
                report.opened = report.opened.saturating_add(1);
            } else {
                report.closed = report.closed.saturating_add(1);
            }
        }
    }
    Ok(report)
}

fn digest(bytes: &[u8]) -> Result<Digest, ApiError> {
    bytes
        .try_into()
        .map(Digest::new)
        .map_err(|_| ApiError::internal(&"stored hash is not 32 bytes"))
}

async fn settle(s: &AppState) -> Result<usize, ApiError> {
    let now = s.clock.now();
    // Claim every resolution that is due. Disputes need `confirmed` and an open window, so none
    // can be filed against a claimed resolution.
    s.db.execute(
        "UPDATE resolutions SET state = 'settling' WHERE state = 'confirmed' AND dispute_until <= to_timestamp($1::bigint) \
         AND NOT EXISTS (SELECT 1 FROM disputes d WHERE d.resolution = resolutions.id AND d.state = 'open')",
        &[&now],
    )
    .await?;
    let claimed = s
        .db
        .query(
            "SELECT id, market, outcome, evidence_hash FROM resolutions WHERE state = 'settling' ORDER BY id",
            &[],
        )
        .await?;
    let mut settled = 0usize;
    for r in claimed {
        let id: i64 = r.get(0);
        let market_col: i64 = r.get(1);
        let market = MarketId::new(u32::try_from(market_col).map_err(|e| ApiError::internal(&e))?);
        let outcome = outcome_from_name(r.get(2))
            .ok_or_else(|| ApiError::internal(&"stored outcome is not yes, no, or void"))?;
        let evidence = digest(r.get(3))?;
        let seq = match crate::markets::market_state(s, market).await? {
            Some(MarketState::Closed) => send_settle(s, id, market, outcome, evidence).await?,
            Some(MarketState::Resolved | MarketState::Voided) => {
                journaled_settlement(s, id, market_col, outcome, evidence).await?
            }
            other => {
                return Err(ApiError::internal(&format!(
                    "resolution {id} is claimed but market {} is {other:?}",
                    market.get()
                )));
            }
        };
        if let Some(seq) = seq {
            let seq = i64::try_from(seq).map_err(|e| ApiError::internal(&e))?;
            s.db.execute(
                "UPDATE resolutions SET state = 'settled', settled_seq = $2 WHERE id = $1 AND state = 'settling'",
                &[&id, &seq],
            )
            .await?;
            settled = settled.saturating_add(1);
        }
    }
    Ok(settled)
}

async fn send_settle(
    s: &AppState,
    id: i64,
    market: MarketId,
    outcome: Outcome,
    evidence: Digest,
) -> Result<Option<u64>, ApiError> {
    let command = Command::Settle {
        market,
        outcome,
        evidence_hash: Some(evidence),
    };
    let detail = json!({ "resolution": id, "market": market.get(), "outcome": brier_wire::outcome_name(outcome), "evidence_hash": evidence.to_hex() });
    let done = audit::submit(
        s,
        Actor::System,
        "settle",
        command,
        detail,
        "dispute window ended with no open dispute",
    )
    .await?;
    match done.committed.events.first() {
        Some(Event::Settled { .. }) => Ok(Some(done.committed.seq.get())),
        _ => Err(ApiError::internal(&format!(
            "settle for resolution {id} answered {:?}",
            done.committed.events
        ))),
    }
}

/// The market is already settled in the journal (a crash between `settle` and the row update):
/// complete the row from the read model, once it has caught up, if the journal agrees with it.
async fn journaled_settlement(
    s: &AppState,
    id: i64,
    market: i64,
    outcome: Outcome,
    evidence: Digest,
) -> Result<Option<u64>, ApiError> {
    let row = s
        .db
        .query_opt(
            "SELECT settled_seq, outcome, evidence_hash FROM market_journal WHERE market = $1 AND settled_seq IS NOT NULL",
            &[&market],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None); // the read model has not reached the settlement yet
    };
    let same = outcome_from_name(row.get(1)) == Some(outcome)
        && row.get::<_, Option<&[u8]>>(2) == Some(evidence.bytes().as_slice());
    if !same {
        return Err(ApiError::internal(&format!(
            "market {market} settled in the journal differently from resolution {id}"
        )));
    }
    let seq: i64 = row.get(0);
    Ok(Some(
        u64::try_from(seq).map_err(|e| ApiError::internal(&e))?,
    ))
}
