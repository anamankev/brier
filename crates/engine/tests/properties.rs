//! Property tests over random command streams, against the rules in docs/protocol.md.
//!
//! Streams mix every command and order type, a few accounts (so self-trade prevention fires),
//! prices clustered around 50 (so orders cross), out-of-range values, lifecycle changes, and
//! cancels or amends of random order IDs (live, dead, other accounts', or never used), and
//! from G2: accounts (one never opened), oversized orders that test collateral, settlements,
//! and kill switches. `Engine::check_invariants` covers I1-I5 after every command.
#![allow(clippy::arithmetic_side_effects)] // test bookkeeping
#![allow(clippy::unwrap_used)] // a failure must stop the test

use std::collections::BTreeMap;

use brier_engine::{
    Command, Engine, EntryKind, Event, LedgerAccount, MarketState, NewOrder, Sequenced, TimeInForce,
};
use brier_types::{AccountId, Credits, MarketId, OrderId, Outcome, Seq, Side};
use proptest::prelude::*;

fn price() -> impl Strategy<Value = u8> {
    prop_oneof![
        20 => 45u8..=55,
        4 => 1u8..=99,
        1 => prop_oneof![Just(0u8), Just(100u8), Just(255u8)],
    ]
}

fn qty() -> impl Strategy<Value = u32> {
    prop_oneof![
        20 => 1u32..=30,
        2 => 1_000u32..=25_000, // large enough to hit insufficient_collateral
        1 => prop_oneof![Just(0u32), Just(25_001u32)],
    ]
}

fn side() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Buy), Just(Side::Sell)]
}

fn tif() -> impl Strategy<Value = TimeInForce> {
    prop_oneof![6 => Just(TimeInForce::Gtc), 2 => Just(TimeInForce::Ioc), 2 => Just(TimeInForce::Fok)]
}

fn state() -> impl Strategy<Value = MarketState> {
    prop_oneof![
        Just(MarketState::PreOpen),
        Just(MarketState::Open),
        Just(MarketState::Halted),
        Just(MarketState::Closed),
        Just(MarketState::Resolved),
        Just(MarketState::Voided),
    ]
}

fn command(max_id: u64) -> impl Strategy<Value = Command> {
    // Account 5 is never opened.
    let account = (1u64..=5).prop_map(AccountId::new);
    let market = (1u32..=3).prop_map(MarketId::new);
    let order = (1u64..=max_id).prop_map(OrderId::new);
    prop_oneof![
        60 => (account.clone(), market.clone(), side(), price(), qty(), tif(), prop::bool::weighted(0.1))
            .prop_map(|(account, market, side, price, qty, tif, post_only)| {
                Command::Place(NewOrder { account, market, side, price, qty, tif, post_only })
            }),
        25 => (account.clone(), order.clone()).prop_map(|(account, order)| Command::Cancel { account, order }),
        8 => (account, order, qty()).prop_map(|(account, order, qty)| Command::Amend { account, order, qty }),
        3 => (market.clone(), state()).prop_map(|(market, state)| Command::SetState { market, state }),
        1 => (1u32..=4).prop_map(|m| Command::CreateMarket { market: MarketId::new(m) }),
        2 => (market.clone(), prop_oneof![Just(Outcome::Yes), Just(Outcome::No), Just(Outcome::Void)])
            .prop_map(|(market, outcome)| Command::Settle { market, outcome }),
        1 => (prop::option::of((1u64..=5).prop_map(AccountId::new)), prop::option::of(market))
            .prop_map(|(account, market)| Command::CancelAll { account, market }),
        1 => Just(Command::HaltAll),
    ]
}

/// Markets 1-3 listed and open, then `len` random commands.
fn stream() -> impl Strategy<Value = Vec<Command>> {
    (1usize..400).prop_flat_map(|len| {
        let max_id = u64::try_from(len).unwrap() + 10;
        prop::collection::vec(command(max_id), len).prop_map(|random| {
            let mut all = Vec::new();
            for a in 1..=4 {
                all.push(Command::OpenAccount {
                    account: AccountId::new(a),
                });
            }
            for m in 1..=3 {
                all.push(Command::CreateMarket {
                    market: MarketId::new(m),
                });
                all.push(Command::SetState {
                    market: MarketId::new(m),
                    state: MarketState::Open,
                });
            }
            all.extend(random);
            all
        })
    })
}

/// Run a stream, checking every invariant after every command. Returns all events per
/// command and the final state hash.
fn run(commands: &[Command]) -> (Vec<Vec<Event>>, [u8; 32], Engine) {
    let mut engine = Engine::new();
    let mut all = Vec::new();
    for (i, &command) in commands.iter().enumerate() {
        let seq = Seq::from_raw(u64::try_from(i).unwrap() + 1);
        let mut out = Vec::new();
        engine.apply(&Sequenced { seq, command }, &mut out).unwrap();
        engine.check_invariants().unwrap();
        all.push(out);
    }
    let hash = engine.state_hash().unwrap();
    (all, hash, engine)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    /// Invariant I5 (never crossed, consistent links and counts) holds after every command;
    /// `run` checks it each step.
    #[test]
    fn invariants_hold_after_every_command(commands in stream()) {
        run(&commands);
    }

    /// Replaying the same commands gives the same events and the same state hash (SPEC §3.2).
    #[test]
    fn replay_is_deterministic(commands in stream()) {
        let (events_a, hash_a, _) = run(&commands);
        let (events_b, hash_b, _) = run(&commands);
        prop_assert_eq!(events_a, events_b);
        prop_assert_eq!(hash_a, hash_b);
    }

    /// Every accepted order's quantity is accounted for exactly:
    /// original = filled + cancelled + amended away + still resting.
    #[test]
    fn every_contract_of_every_order_is_accounted_for(commands in stream()) {
        let (events, _, engine) = run(&commands);
        let mut original: BTreeMap<OrderId, u64> = BTreeMap::new();
        let mut gone: BTreeMap<OrderId, u64> = BTreeMap::new();
        for (cmd, out) in commands.iter().zip(&events) {
            for event in out {
                match *event {
                    Event::Accepted { order } => {
                        let Command::Place(o) = cmd else { panic!("accepted without a place") };
                        original.insert(order, u64::from(o.qty));
                    }
                    Event::Trade(t) => {
                        *gone.entry(t.maker).or_default() += u64::from(t.qty.contracts());
                        *gone.entry(t.taker).or_default() += u64::from(t.qty.contracts());
                    }
                    Event::Cancelled { order, remaining, .. } => {
                        *gone.entry(order).or_default() += u64::from(remaining.contracts());
                    }
                    _ => {}
                }
            }
        }
        // Amendments: the amount amended away is the running quantity just before the amend
        // minus the new quantity.
        let mut resting_now: BTreeMap<OrderId, u64> = BTreeMap::new();
        for market in engine.markets() {
            for side in [Side::Buy, Side::Sell] {
                for o in engine.resting(market, side).unwrap() {
                    resting_now.insert(o.id, u64::from(o.remaining.contracts()));
                }
            }
        }
        let mut amended_away: BTreeMap<OrderId, u64> = BTreeMap::new();
        let mut running: BTreeMap<OrderId, u64> = original.clone();
        for out in &events {
            for event in out {
                match *event {
                    Event::Trade(t) => {
                        for id in [t.maker, t.taker] {
                            *running.get_mut(&id).unwrap() -= u64::from(t.qty.contracts());
                        }
                    }
                    Event::Amended { order, remaining } => {
                        let before = running[&order];
                        let after = u64::from(remaining.contracts());
                        prop_assert!(after < before, "an amend must reduce the quantity");
                        *amended_away.entry(order).or_default() += before - after;
                        *running.get_mut(&order).unwrap() = after;
                    }
                    _ => {}
                }
            }
        }
        for (id, &qty) in &original {
            let accounted = gone.get(id).copied().unwrap_or(0)
                + amended_away.get(id).copied().unwrap_or(0)
                + resting_now.get(id).copied().unwrap_or(0);
            prop_assert_eq!(accounted, qty, "order {}", id.get());
        }
    }
    /// The play-money boundary (SPEC §1.2): every ledger entry is one of three kinds with
    /// exactly the legs the G2 plan allows, so no path moves credits between users except a
    /// matched trade, and credits only enter through grants and leave escrow by settlement.
    #[test]
    fn every_entry_is_a_grant_a_trade_or_a_settlement(commands in stream()) {
        let (events, _, _) = run(&commands);
        for out in &events {
            let trade = out.iter().find_map(|e| match e { Event::Trade(t) => Some(*t), _ => None });
            for event in out {
                let Event::Entry(entry) = event else { continue };
                let sum = entry.legs().iter().map(|&(_, x)| x.cents()).sum::<i64>();
                prop_assert_eq!(sum, 0);
                match entry.kind() {
                    EntryKind::Grant => {
                        prop_assert!(matches!(entry.legs(), [(LedgerAccount::Issuance, _), (LedgerAccount::Cash(_), x)] if x.cents() > 0));
                    }
                    EntryKind::Trade => {
                        prop_assert!(trade.is_some(), "a trade entry without a trade");
                        for &(account, _) in entry.legs() {
                            prop_assert!(matches!(account, LedgerAccount::Cash(_) | LedgerAccount::Escrow(_)));
                        }
                    }
                    EntryKind::Settle => {
                        prop_assert!(matches!(entry.legs(), [(LedgerAccount::Escrow(_), _), (LedgerAccount::Cash(_), x)] if x.cents() > 0));
                    }
                }
            }
        }
    }

    /// Settlement pays out exactly the escrow: afterwards the market's escrow is zero.
    #[test]
    fn settlement_always_zeroes_escrow(commands in stream()) {
        let mut engine = Engine::new();
        for (i, &command) in commands.iter().enumerate() {
            let mut out = Vec::new();
            engine.apply(&Sequenced { seq: Seq::from_raw(u64::try_from(i).unwrap() + 1), command }, &mut out).unwrap();
            for e in &out {
                if let Event::Settled { market, .. } = *e {
                    prop_assert_eq!(engine.escrow(market), Some(Credits::ZERO));
                }
            }
        }
    }
}
