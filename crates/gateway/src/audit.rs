//! The audit log: every administrative action, every resolution step, and every command the
//! gateway sends on its own (the scheduler and the settler, as actor `system`), with who, why,
//! and what the engine answered.

use brier_core::Committed;
use brier_engine::Command;
use brier_wire::event_json;
use serde_json::{Value, json};

use crate::AppState;
use crate::error::ApiError;

/// Who acted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// A signed-in user.
    User(i64),
    /// The gateway itself.
    System,
}

impl Actor {
    const fn columns(self) -> (Option<i64>, &'static str) {
        match self {
            Self::User(id) => (Some(id), "user"),
            Self::System => (None, "system"),
        }
    }
}

/// A trimmed reason of 3 to 500 characters.
///
/// # Errors
/// [`ApiError::bad_request`] otherwise.
pub fn reason(raw: &str) -> Result<&str, ApiError> {
    let r = raw.trim();
    if (3..=500).contains(&r.chars().count()) {
        Ok(r)
    } else {
        Err(ApiError::bad_request("reason must be 3 to 500 characters"))
    }
}

/// Write one audit row for an action that sends no command; returns its ID.
///
/// # Errors
/// [`ApiError`] if the database fails.
pub async fn record(
    s: &AppState,
    actor: Actor,
    action: &str,
    detail: &Value,
    reason: &str,
) -> Result<i64, ApiError> {
    let (user, kind) = actor.columns();
    let row = s
        .db
        .query_one(
            "INSERT INTO audit_log (user_id, actor, action, detail, reason) VALUES ($1, $2, $3, $4::text::jsonb, $5) RETURNING id",
            &[&user, &kind, &action, &detail.to_string(), &reason],
        )
        .await?;
    Ok(row.get(0))
}

/// A command sent with an audit row.
pub struct Audited {
    /// The audit row.
    pub audit_id: i64,
    /// The command as the core committed it.
    pub committed: Committed,
    /// Its events, in the wire format.
    pub events: Vec<Value>,
}

/// Submit `command` with an audit row written first (so even a crash mid-command leaves a
/// trace), then completed with the sequence number and the engine's answer.
///
/// # Errors
/// [`ApiError`] if the database or the core fails; the audit row then records the error.
pub async fn submit(
    s: &AppState,
    actor: Actor,
    action: &str,
    command: Command,
    detail: Value,
    reason: &str,
) -> Result<Audited, ApiError> {
    let audit_id = record(s, actor, action, &json!({ "request": detail }), reason).await?;
    let committed = match s.core.submit(command).await {
        Ok(c) => c,
        Err(e) => {
            let note = json!({ "error": e.to_string() }).to_string();
            s.db.execute(
                "UPDATE audit_log SET detail = detail || $2::text::jsonb WHERE id = $1",
                &[&audit_id, &note],
            )
            .await?;
            return Err(e.into());
        }
    };
    let seq = committed.seq.get();
    let events: Vec<Value> = committed
        .events
        .iter()
        .map(|e| event_json(seq, e))
        .collect();
    let seq_col = i64::try_from(seq).map_err(|e| ApiError::internal(&e))?;
    let note = json!({ "events": events }).to_string();
    s.db.execute(
        "UPDATE audit_log SET seq = $2, detail = detail || $3::text::jsonb WHERE id = $1",
        &[&audit_id, &seq_col, &note],
    )
    .await?;
    Ok(Audited {
        audit_id,
        committed,
        events,
    })
}
