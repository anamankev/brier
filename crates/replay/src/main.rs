//! `brier-replay`: read wire-format commands as JSON lines on stdin, apply them to a fresh
//! engine in order, and write one JSON line per event to stdout, then a `final_book` record.
//! The format is fixed in docs/gates/G1-plan.md and shared with the Python reference engine.
//!
//! Exit codes: 0 success; 2 malformed input (with the line number on stderr); 3 the engine
//! halted (a sequence gap or a violated invariant).

use std::io::{self, BufRead, BufWriter, Write};
use std::process::ExitCode;

use brier_engine::{
    CancelReason, Command, Engine, EntryKind, Event, LedgerAccount, MarketState, NewOrder,
    RejectReason, Sequenced, TimeInForce,
};
use brier_types::{AccountId, Credits, MarketId, OrderId, Outcome, Seq, Side};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct Line {
    seq: u64,
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

fn event_json(seq: u64, e: &Event) -> Value {
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

fn final_book(engine: &Engine) -> Result<Value, brier_engine::EngineError> {
    let mut markets = Vec::new();
    for market in engine.markets() {
        let side_list = |side| -> Result<Vec<Value>, brier_engine::EngineError> {
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

enum Failure {
    Input(String),
    Engine(String),
    Io(io::Error),
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn run(input: impl BufRead, output: impl Write) -> Result<(), Failure> {
    let mut out = BufWriter::new(output);
    let mut engine = Engine::new();
    let mut events = Vec::new();
    for (index, line) in input.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let number = index.saturating_add(1);
        let parsed: Line = serde_json::from_str(&line)
            .map_err(|e| Failure::Input(format!("line {number}: {e}")))?;
        events.clear();
        let cmd = Sequenced {
            seq: Seq::from_raw(parsed.seq),
            command: to_command(parsed.command),
        };
        engine
            .apply(&cmd, &mut events)
            .map_err(|e| Failure::Engine(format!("line {number}: {e}")))?;
        for e in &events {
            serde_json::to_writer(&mut out, &event_json(parsed.seq, e))
                .map_err(|e| Failure::Io(io::Error::other(e)))?;
            out.write_all(b"\n")?;
        }
    }
    let book = final_book(&engine).map_err(|e| Failure::Engine(e.to_string()))?;
    serde_json::to_writer(&mut out, &book).map_err(|e| Failure::Io(io::Error::other(e)))?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn main() -> ExitCode {
    let stdin = io::stdin();
    match run(stdin.lock(), io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Input(msg)) => {
            eprintln!("brier-replay: malformed input: {msg}");
            ExitCode::from(2)
        }
        Err(Failure::Engine(msg)) => {
            eprintln!("brier-replay: engine halted: {msg}");
            ExitCode::from(3)
        }
        Err(Failure::Io(e)) => {
            eprintln!("brier-replay: i/o error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(input: &str) -> Result<Vec<Value>, Failure> {
        let mut out = Vec::new();
        run(input.as_bytes(), &mut out)?;
        Ok(String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect())
    }

    #[test]
    fn a_short_session_round_trips_through_the_wire_format() {
        let input = r#"{"seq":1,"cmd":"open_account","account":1}
{"seq":2,"cmd":"open_account","account":2}
{"seq":3,"cmd":"create_market","market":1}
{"seq":4,"cmd":"set_state","market":1,"state":"open"}
{"seq":5,"cmd":"place","account":1,"market":1,"side":"sell","price":60,"qty":5,"tif":"gtc","post_only":false}
{"seq":6,"cmd":"place","account":2,"market":1,"side":"buy","price":61,"qty":2,"tif":"ioc","post_only":false}
{"seq":7,"cmd":"place","account":2,"market":1,"side":"buy","price":0,"qty":2,"tif":"gtc","post_only":false}
"#;
        let lines = replay(input).ok().unwrap();
        // Expected values computed by hand from docs/gates/G2-plan.md: account 1 sells YES it
        // does not hold, so each unit opens NO at 100 − 60 = 40 (reserve 5 × 40 = 200); the
        // trade of 2 at 60 moves buyer −120, seller +120 − 200 = −80, escrow +200.
        assert_eq!(
            lines,
            vec![
                json!({"seq":1,"ev":"account_opened","account":1,"cash":1_000_000}),
                json!({"seq":1,"ev":"entry","kind":"grant","legs":[["issuance",-1_000_000],["cash:1",1_000_000]]}),
                json!({"seq":2,"ev":"account_opened","account":2,"cash":1_000_000}),
                json!({"seq":2,"ev":"entry","kind":"grant","legs":[["issuance",-1_000_000],["cash:2",1_000_000]]}),
                json!({"seq":3,"ev":"market_created","market":1}),
                json!({"seq":4,"ev":"market_state","market":1,"state":"open"}),
                json!({"seq":5,"ev":"accepted","order":5}),
                json!({"seq":5,"ev":"rested","order":5,"remaining":5}),
                json!({"seq":5,"ev":"reserved","account":1,"market":1,"amount":200}),
                json!({"seq":6,"ev":"accepted","order":6}),
                json!({"seq":6,"ev":"trade","market":1,"maker":5,"taker":6,"maker_account":1,
                       "taker_account":2,"taker_side":"buy","price":60,"qty":2}),
                json!({"seq":6,"ev":"entry","kind":"trade","legs":[["cash:2",-120],["cash:1",-80],["escrow:1",200]]}),
                json!({"seq":6,"ev":"reserved","account":1,"market":1,"amount":120}),
                json!({"seq":7,"ev":"rejected","reason":"invalid_price"}),
                json!({"ev":"final_book",
                       "markets":[{"market":1,"state":"open","bids":[],"asks":[[5,1,60,3]]}],
                       "accounts":[{"account":1,"cash":999_920,"reserved":120,"positions":[[1,-2]]},
                                   {"account":2,"cash":999_880,"reserved":0,"positions":[[1,2]]}],
                       "escrow":[[1,200]]}),
            ]
        );
    }

    #[test]
    fn malformed_lines_and_sequence_gaps_are_failures_not_events() {
        assert!(matches!(
            replay("{\"seq\":1,\"cmd\":\"nope\"}\n"),
            Err(Failure::Input(_))
        ));
        assert!(matches!(
            replay(
                "{\"seq\":1,\"cmd\":\"place\",\"account\":1,\"market\":1,\"side\":\"buy\",\"price\":300,\"qty\":1,\"tif\":\"gtc\",\"post_only\":false}\n"
            ),
            Err(Failure::Input(_))
        ));
        assert!(matches!(
            replay("{\"seq\":2,\"cmd\":\"create_market\",\"market\":1}\n"),
            Err(Failure::Engine(_))
        ));
    }
}
