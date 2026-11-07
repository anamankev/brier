//! The wire format (docs/protocol.md): commands as JSON objects in, events as JSON objects
//! out. Shared by `brier-replay`, `brier-core`, and, through the protocol document, the Python
//! reference engine.

use std::fmt;

use brier_engine::{
    CancelReason, Command, Engine, EngineError, EntryKind, Event, LedgerAccount, MarketState,
    NewOrder, RejectReason, TimeInForce,
};
use brier_types::{AccountId, Credits, Digest, MarketId, OrderId, Outcome, Side};
use serde::Deserialize;
use serde_json::{Value, json};

/// A line that is not a well-formed wire command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub String);

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "malformed command: {}", self.0)
    }
}

impl std::error::Error for WireError {}

/// Parse one command line. Returns its `seq` if the line has one (the replay tool requires
/// it; the core assigns its own and ignores it), and the command.
///
/// # Errors
/// [`WireError`] if the line is not a command in the wire format, or a value is outside its
/// wire type (docs/protocol.md §1).
pub fn parse_command(line: &str) -> Result<(Option<u64>, Command), WireError> {
    let parsed: Line = serde_json::from_str(line).map_err(|e| WireError(e.to_string()))?;
    Ok((parsed.seq, to_command(parsed.command)?))
}

#[derive(Debug, Deserialize)]
struct Line {
    seq: Option<u64>,
    #[serde(flatten)]
    command: WireCommand,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum WireCommand {
    CreateMarket {
        market: u32,
        #[serde(default)]
        rules_hash: Option<String>,
    },
    SetState {
        market: u32,
        state: WireState,
    },
    Place {
        account: u64,
        market: u32,
        side: WireSide,
        price: u8,
        qty: u32,
        tif: WireTif,
        post_only: bool,
        #[serde(default)]
        client_order_id: Option<u64>,
    },
    Cancel {
        account: u64,
        order: u64,
    },
    Amend {
        account: u64,
        order: u64,
        qty: u32,
    },
    OpenAccount {
        account: u64,
    },
    Settle {
        market: u32,
        outcome: WireOutcome,
        #[serde(default)]
        evidence_hash: Option<String>,
    },
    CancelAll {
        #[serde(default)]
        account: Option<u64>,
        #[serde(default)]
        market: Option<u32>,
    },
    HaltAll,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireOutcome {
    Yes,
    No,
    Void,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireTif {
    Gtc,
    Ioc,
    Fok,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireState {
    PreOpen,
    Open,
    Halted,
    Closed,
    Resolved,
    Voided,
}

const fn side(s: WireSide) -> Side {
    match s {
        WireSide::Buy => Side::Buy,
        WireSide::Sell => Side::Sell,
    }
}

/// An optional digest field: 64 lowercase hex digits (docs/protocol.md §1).
fn digest(field: &str, value: Option<String>) -> Result<Option<Digest>, WireError> {
    value
        .map(|s| {
            Digest::from_hex(&s)
                .ok_or_else(|| WireError(format!("{field} is not 64 lowercase hex digits")))
        })
        .transpose()
}

fn to_command(w: WireCommand) -> Result<Command, WireError> {
    Ok(match w {
        WireCommand::CreateMarket { market, rules_hash } => Command::CreateMarket {
            market: MarketId::new(market),
            rules_hash: digest("rules_hash", rules_hash)?,
        },
        WireCommand::SetState { market, state } => Command::SetState {
            market: MarketId::new(market),
            state: match state {
                WireState::PreOpen => MarketState::PreOpen,
                WireState::Open => MarketState::Open,
                WireState::Halted => MarketState::Halted,
                WireState::Closed => MarketState::Closed,
                WireState::Resolved => MarketState::Resolved,
                WireState::Voided => MarketState::Voided,
            },
        },
        WireCommand::Place {
            account,
            market,
            side: s,
            price,
            qty,
            tif,
            post_only,
            client_order_id,
        } => Command::Place(NewOrder {
            account: AccountId::new(account),
            market: MarketId::new(market),
            side: side(s),
            price,
            qty,
            tif: match tif {
                WireTif::Gtc => TimeInForce::Gtc,
                WireTif::Ioc => TimeInForce::Ioc,
                WireTif::Fok => TimeInForce::Fok,
            },
            post_only,
            client_order_id,
        }),
        WireCommand::Cancel { account, order } => Command::Cancel {
            account: AccountId::new(account),
            order: OrderId::new(order),
        },
        WireCommand::Amend {
            account,
            order,
            qty,
        } => Command::Amend {
            account: AccountId::new(account),
            order: OrderId::new(order),
            qty,
        },
        WireCommand::OpenAccount { account } => Command::OpenAccount {
            account: AccountId::new(account),
        },
        WireCommand::Settle {
            market,
            outcome,
            evidence_hash,
        } => Command::Settle {
            market: MarketId::new(market),
            outcome: match outcome {
                WireOutcome::Yes => Outcome::Yes,
                WireOutcome::No => Outcome::No,
                WireOutcome::Void => Outcome::Void,
            },
            evidence_hash: digest("evidence_hash", evidence_hash)?,
        },
        WireCommand::CancelAll { account, market } => Command::CancelAll {
            account: account.map(AccountId::new),
            market: market.map(MarketId::new),
        },
        WireCommand::HaltAll => Command::HaltAll,
    })
}

/// Add `key: hex` to an event object when the digest is present (docs/protocol.md §6).
fn with_digest(mut v: Value, key: &str, d: Option<Digest>) -> Value {
    if let (Some(d), Some(map)) = (d, v.as_object_mut()) {
        map.insert(key.to_owned(), Value::String(d.to_hex()));
    }
    v
}

/// Wire name of an outcome.
#[must_use]
pub const fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Yes => "yes",
        Outcome::No => "no",
        Outcome::Void => "void",
    }
}

const fn entry_kind_name(k: EntryKind) -> &'static str {
    match k {
        EntryKind::Grant => "grant",
        EntryKind::Trade => "trade",
        EntryKind::Settle => "settle",
    }
}

fn ledger_account_name(a: LedgerAccount) -> String {
    match a {
        LedgerAccount::Cash(id) => format!("cash:{}", id.get()),
        LedgerAccount::Escrow(m) => format!("escrow:{}", m.get()),
        LedgerAccount::Issuance => "issuance".to_owned(),
    }
}

/// Wire name of a side.
#[must_use]
pub const fn side_name(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// Wire name of a market state.
#[must_use]
pub const fn state_name(s: MarketState) -> &'static str {
    match s {
        MarketState::PreOpen => "pre_open",
        MarketState::Open => "open",
        MarketState::Halted => "halted",
        MarketState::Closed => "closed",
        MarketState::Resolved => "resolved",
        MarketState::Voided => "voided",
    }
}

/// Wire name of a cancel reason.
#[must_use]
pub const fn cancel_name(r: CancelReason) -> &'static str {
    match r {
        CancelReason::User => "user",
        CancelReason::IocRemainder => "ioc_remainder",
        CancelReason::SelfTrade => "self_trade",
        CancelReason::MarketClosed => "market_closed",
        CancelReason::KillSwitch => "kill_switch",
    }
}

/// Wire name of a reject reason.
#[must_use]
pub const fn reject_name(r: RejectReason) -> &'static str {
    match r {
        RejectReason::UnknownMarket => "unknown_market",
        RejectReason::MarketExists => "market_exists",
        RejectReason::MarketNotOpen => "market_not_open",
        RejectReason::InvalidTransition => "invalid_transition",
        RejectReason::InvalidOrder => "invalid_order",
        RejectReason::InvalidPrice => "invalid_price",
        RejectReason::InvalidQuantity => "invalid_quantity",
        RejectReason::PostOnlyWouldCross => "post_only_would_cross",
        RejectReason::FokUnfillable => "fok_unfillable",
        RejectReason::UnknownOrder => "unknown_order",
        RejectReason::InvalidAmend => "invalid_amend",
        RejectReason::UnknownAccount => "unknown_account",
        RejectReason::AccountExists => "account_exists",
        RejectReason::TooManyOrders => "too_many_orders",
        RejectReason::InsufficientCollateral => "insufficient_collateral",
    }
}

/// One event as its wire JSON object (docs/protocol.md §6).
#[must_use]
pub fn event_json(seq: u64, e: &Event) -> Value {
    match *e {
        Event::MarketCreated { market, rules_hash } => with_digest(
            json!({"seq": seq, "ev": "market_created", "market": market.get()}),
            "rules_hash",
            rules_hash,
        ),
        Event::MarketStateChanged { market, state } => {
            json!({"seq": seq, "ev": "market_state", "market": market.get(), "state": state_name(state)})
        }
        Event::Accepted { order } => json!({"seq": seq, "ev": "accepted", "order": order.get()}),
        Event::Trade(t) => json!({
            "seq": seq, "ev": "trade", "market": t.market.get(),
            "maker": t.maker.get(), "taker": t.taker.get(),
            "maker_account": t.maker_account.get(), "taker_account": t.taker_account.get(),
            "taker_side": side_name(t.taker_side), "price": t.price.ticks(), "qty": t.qty.contracts(),
        }),
        Event::Rested { order, remaining } => {
            json!({"seq": seq, "ev": "rested", "order": order.get(), "remaining": remaining.contracts()})
        }
        Event::Cancelled {
            order,
            remaining,
            reason,
        } => json!({
            "seq": seq, "ev": "cancelled", "order": order.get(),
            "remaining": remaining.contracts(), "reason": cancel_name(reason),
        }),
        Event::Amended { order, remaining } => {
            json!({"seq": seq, "ev": "amended", "order": order.get(), "remaining": remaining.contracts()})
        }
        Event::Duplicate { original } => {
            json!({"seq": seq, "ev": "duplicate", "order": original.get()})
        }
        Event::Rejected { reason } => {
            json!({"seq": seq, "ev": "rejected", "reason": reject_name(reason)})
        }
        Event::AccountOpened { account, cash } => {
            json!({"seq": seq, "ev": "account_opened", "account": account.get(), "cash": cash.cents()})
        }
        Event::Entry(entry) => json!({
            "seq": seq, "ev": "entry", "kind": entry_kind_name(entry.kind()),
            "legs": entry.legs().iter().map(|&(a, x)| json!([ledger_account_name(a), x.cents()])).collect::<Vec<_>>(),
        }),
        Event::Reserved {
            account,
            market,
            amount,
        } => json!({
            "seq": seq, "ev": "reserved", "account": account.get(), "market": market.get(), "amount": amount.cents(),
        }),
        Event::Settled {
            market,
            outcome,
            evidence_hash,
        } => with_digest(
            json!({"seq": seq, "ev": "settled", "market": market.get(), "outcome": outcome_name(outcome)}),
            "evidence_hash",
            evidence_hash,
        ),
    }
}

/// The end-of-input record (docs/protocol.md §7).
///
/// # Errors
/// [`EngineError`] if a book's links are broken.
pub fn final_book(engine: &Engine) -> Result<Value, EngineError> {
    let mut markets = Vec::new();
    for market in engine.markets() {
        let side_list = |side| -> Result<Vec<Value>, EngineError> {
            Ok(engine
                .resting(market, side)?
                .into_iter()
                .map(|o| {
                    json!([
                        o.id.get(),
                        o.account.get(),
                        o.price.ticks(),
                        o.remaining.contracts()
                    ])
                })
                .collect())
        };
        let state = engine.market_state(market).map(state_name);
        markets.push(json!({
            "market": market.get(),
            "state": state,
            "bids": side_list(Side::Buy)?,
            "asks": side_list(Side::Sell)?,
        }));
    }
    let accounts: Vec<Value> = engine
        .accounts()
        .into_iter()
        .map(|v| {
            json!({
                "account": v.account.get(),
                "cash": v.cash.cents(),
                "reserved": v.reserved.cents(),
                "positions": v.positions.iter().map(|&(m, x)| json!([m.get(), x])).collect::<Vec<_>>(),
            })
        })
        .collect();
    let escrow: Vec<Value> = engine
        .markets()
        .map(|m| json!([m.get(), engine.escrow(m).map(Credits::cents)]))
        .collect();
    Ok(json!({"ev": "final_book", "markets": markets, "accounts": accounts, "escrow": escrow}))
}

/// The side with this wire name.
#[must_use]
pub fn side_from_name(name: &str) -> Option<Side> {
    [Side::Buy, Side::Sell]
        .into_iter()
        .find(|&s| side_name(s) == name)
}

/// The market state with this wire name.
#[must_use]
pub fn state_from_name(name: &str) -> Option<MarketState> {
    use MarketState::{Closed, Halted, Open, PreOpen, Resolved, Voided};
    [PreOpen, Open, Halted, Closed, Resolved, Voided]
        .into_iter()
        .find(|&s| state_name(s) == name)
}

/// The outcome with this wire name.
#[must_use]
pub fn outcome_from_name(name: &str) -> Option<Outcome> {
    [Outcome::Yes, Outcome::No, Outcome::Void]
        .into_iter()
        .find(|&o| outcome_name(o) == name)
}

/// The time in force with this wire name (`gtc`, `ioc`, `fok`).
#[must_use]
pub fn tif_from_name(name: &str) -> Option<TimeInForce> {
    match name {
        "gtc" => Some(TimeInForce::Gtc),
        "ioc" => Some(TimeInForce::Ioc),
        "fok" => Some(TimeInForce::Fok),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEX: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    #[test]
    fn hashes_are_optional_lowercase_hex() {
        let (_, plain) = parse_command(r#"{"cmd":"create_market","market":3}"#).unwrap();
        assert_eq!(
            plain,
            Command::CreateMarket {
                market: MarketId::new(3),
                rules_hash: None
            }
        );
        let line =
            format!(r#"{{"cmd":"settle","market":3,"outcome":"no","evidence_hash":"{HEX}"}}"#);
        let (_, settle) = parse_command(&line).unwrap();
        let Command::Settle { evidence_hash, .. } = settle else {
            unreachable!()
        };
        assert_eq!(evidence_hash.map(|d| d.to_hex()), Some(HEX.to_owned()));
        for bad in [HEX.to_uppercase(), HEX[1..].to_owned(), format!("{HEX}0")] {
            let line = format!(r#"{{"cmd":"create_market","market":3,"rules_hash":"{bad}"}}"#);
            assert!(parse_command(&line).is_err(), "{bad}");
        }
        assert!(parse_command(r#"{"cmd":"create_market","market":3,"rules_hash":7}"#).is_err());
    }

    #[test]
    fn events_carry_a_hash_only_when_the_command_had_one() {
        let rules_hash = Digest::from_hex(HEX);
        let market = MarketId::new(3);
        assert_eq!(
            event_json(1, &Event::MarketCreated { market, rules_hash }),
            json!({"seq": 1, "ev": "market_created", "market": 3, "rules_hash": HEX})
        );
        assert_eq!(
            event_json(
                2,
                &Event::Settled {
                    market,
                    outcome: Outcome::Void,
                    evidence_hash: None
                }
            ),
            json!({"seq": 2, "ev": "settled", "market": 3, "outcome": "void"})
        );
    }
}
