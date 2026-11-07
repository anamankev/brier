//! The wire format (docs/protocol.md): commands as JSON objects in, events as JSON objects
//! out. Shared by `brier-replay`, `brier-core`, and, through the protocol document, the Python
//! reference engine.

use std::fmt;

use brier_engine::{
    CancelReason, Command, Engine, EngineError, EntryKind, Event, LedgerAccount, MarketState,
    NewOrder, RejectReason, TimeInForce,
};
use brier_types::{AccountId, Credits, MarketId, OrderId, Outcome, Side};
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
    Ok((parsed.seq, to_command(parsed.command)))
}

#[derive(Debug, Deserialize)]
struct Line {
    seq: Option<u64>,
    #[serde(flatten)]
    command: WireCommand,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum WireCommand {
    CreateMarket {
        market: u32,
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

fn to_command(w: WireCommand) -> Command {
    match w {
        WireCommand::CreateMarket { market } => Command::CreateMarket {
            market: MarketId::new(market),
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
        WireCommand::Settle { market, outcome } => Command::Settle {
            market: MarketId::new(market),
            outcome: match outcome {
                WireOutcome::Yes => Outcome::Yes,
                WireOutcome::No => Outcome::No,
                WireOutcome::Void => Outcome::Void,
            },
        },
        WireCommand::CancelAll { account, market } => Command::CancelAll {
            account: account.map(AccountId::new),
            market: market.map(MarketId::new),
        },
        WireCommand::HaltAll => Command::HaltAll,
    }
}

const fn outcome_name(o: Outcome) -> &'static str {
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

const fn side_name(s: Side) -> &'static str {
    match s {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

const fn state_name(s: MarketState) -> &'static str {
    match s {
        MarketState::PreOpen => "pre_open",
        MarketState::Open => "open",
        MarketState::Halted => "halted",
        MarketState::Closed => "closed",
        MarketState::Resolved => "resolved",
        MarketState::Voided => "voided",
    }
}

const fn cancel_name(r: CancelReason) -> &'static str {
    match r {
        CancelReason::User => "user",
        CancelReason::IocRemainder => "ioc_remainder",
        CancelReason::SelfTrade => "self_trade",
        CancelReason::MarketClosed => "market_closed",
        CancelReason::KillSwitch => "kill_switch",
    }
}

const fn reject_name(r: RejectReason) -> &'static str {
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
        Event::MarketCreated { market } => {
            json!({"seq": seq, "ev": "market_created", "market": market.get()})
        }
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
        Event::Settled { market, outcome } => {
            json!({"seq": seq, "ev": "settled", "market": market.get(), "outcome": outcome_name(outcome)})
        }
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
