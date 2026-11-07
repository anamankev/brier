//! One test per matching rule in docs/gates/G1-plan.md (SPEC §3.1). Every command also
//! re-checks the engine's invariants.
#![allow(clippy::arithmetic_side_effects)] // test bookkeeping (sequence counter)
#![allow(clippy::unwrap_used)] // test helpers: a failure must stop the test

use brier_engine::{
    CancelReason, Command, Engine, EngineError, Event, MarketState, NewOrder, RejectReason,
    Sequenced, TimeInForce, Trade,
};
use brier_types::{AccountId, MarketId, OrderId, Price, Quantity, Seq, Side};

const M: MarketId = MarketId::new(1);
const ALICE: AccountId = AccountId::new(1);
const BOB: AccountId = AccountId::new(2);
const CAROL: AccountId = AccountId::new(3);
/// A book side with no resting orders.
const EMPTY_SIDE: [(u64, u8, u32); 0] = [];

struct Harness {
    engine: Engine,
    seq: u64,
}

impl Harness {
    fn new() -> Self {
        Self {
            engine: Engine::new(),
            seq: 0,
        }
    }

    /// A harness with market M listed and open.
    fn open() -> Self {
        let mut h = Self::new();
        h.cmd(Command::CreateMarket { market: M });
        h.cmd(Command::SetState {
            market: M,
            state: MarketState::Open,
        });
        h
    }

    fn cmd(&mut self, command: Command) -> Vec<Event> {
        self.seq += 1;
        let mut out = Vec::new();
        self.engine
            .apply(
                &Sequenced {
                    seq: Seq::from_raw(self.seq),
                    command,
                },
                &mut out,
            )
            .unwrap();
        self.engine.check_invariants().unwrap();
        out
    }

    /// The ID the next accepted order will get (its command's sequence number).
    fn next_id(&self) -> OrderId {
        OrderId::new(self.seq + 1)
    }

    fn order(
        &mut self,
        account: AccountId,
        side: Side,
        price: u8,
        qty: u32,
        tif: TimeInForce,
    ) -> Vec<Event> {
        self.cmd(Command::Place(NewOrder {
            account,
            market: M,
            side,
            price,
            qty,
            tif,
            post_only: false,
        }))
    }

    fn gtc(&mut self, account: AccountId, side: Side, price: u8, qty: u32) -> OrderId {
        let id = self.next_id();
        self.order(account, side, price, qty, TimeInForce::Gtc);
        id
    }

    fn book(&self, side: Side) -> Vec<(u64, u8, u32)> {
        self.engine
            .resting(M, side)
            .unwrap()
            .into_iter()
            .map(|o| (o.id.get(), o.price.ticks(), o.remaining.contracts()))
            .collect()
    }
}

fn p(t: u8) -> Price {
    Price::new(t).unwrap()
}
fn q(n: u32) -> Quantity {
    Quantity::new(n).unwrap()
}
fn rejected(reason: RejectReason) -> Vec<Event> {
    vec![Event::Rejected { reason }]
}
#[allow(clippy::too_many_arguments)]
fn trade(
    maker: OrderId,
    taker: OrderId,
    ma: AccountId,
    ta: AccountId,
    side: Side,
    price: u8,
    qty: u32,
) -> Event {
    Event::Trade(Trade {
        market: M,
        maker,
        taker,
        maker_account: ma,
        taker_account: ta,
        taker_side: side,
        price: p(price),
        qty: q(qty),
    })
}

#[test]
fn a_sequence_gap_halts_the_engine() {
    let mut engine = Engine::new();
    let mut out = Vec::new();
    let err = engine
        .apply(
            &Sequenced {
                seq: Seq::from_raw(2),
                command: Command::CreateMarket { market: M },
            },
            &mut out,
        )
        .unwrap_err();
    assert_eq!(
        err,
        EngineError::SequenceGap {
            expected: Seq::FIRST,
            got: Seq::from_raw(2)
        }
    );
    assert_eq!(out, [] as [Event; 0]);
}

#[test]
fn markets_are_created_once_and_must_exist() {
    let mut h = Harness::new();
    assert_eq!(
        h.cmd(Command::CreateMarket { market: M }),
        vec![Event::MarketCreated { market: M }]
    );
    assert_eq!(
        h.cmd(Command::CreateMarket { market: M }),
        rejected(RejectReason::MarketExists)
    );
    let other = MarketId::new(9);
    assert_eq!(
        h.cmd(Command::SetState {
            market: other,
            state: MarketState::Open
        }),
        rejected(RejectReason::UnknownMarket)
    );
    let e = h.cmd(Command::Place(NewOrder {
        account: ALICE,
        market: other,
        side: Side::Buy,
        price: 50,
        qty: 1,
        tif: TimeInForce::Gtc,
        post_only: false,
    }));
    assert_eq!(e, rejected(RejectReason::UnknownMarket));
}

#[test]
fn orders_need_an_open_market_and_that_check_comes_first() {
    let mut h = Harness::new();
    h.cmd(Command::CreateMarket { market: M });
    // PreOpen, with an invalid price too: market_not_open is checked first.
    assert_eq!(
        h.order(ALICE, Side::Buy, 0, 1, TimeInForce::Gtc),
        rejected(RejectReason::MarketNotOpen)
    );
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Open,
    });
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Halted,
    });
    assert_eq!(
        h.order(ALICE, Side::Buy, 50, 1, TimeInForce::Gtc),
        rejected(RejectReason::MarketNotOpen)
    );
}

#[test]
fn price_and_quantity_out_of_range_are_rejected_not_clipped() {
    let mut h = Harness::open();
    for bad in [0, 100, 255] {
        assert_eq!(
            h.order(ALICE, Side::Buy, bad, 1, TimeInForce::Gtc),
            rejected(RejectReason::InvalidPrice)
        );
    }
    for bad in [0, 25_001] {
        assert_eq!(
            h.order(ALICE, Side::Buy, 50, bad, TimeInForce::Gtc),
            rejected(RejectReason::InvalidQuantity)
        );
    }
    // Both invalid: price is checked before quantity.
    assert_eq!(
        h.order(ALICE, Side::Buy, 0, 0, TimeInForce::Gtc),
        rejected(RejectReason::InvalidPrice)
    );
    assert_eq!(h.book(Side::Buy), EMPTY_SIDE);
}

#[test]
fn post_only_rests_or_is_rejected_and_needs_gtc() {
    let mut h = Harness::open();
    let ask = h.gtc(BOB, Side::Sell, 60, 5);
    let post = |price, tif| {
        Command::Place(NewOrder {
            account: ALICE,
            market: M,
            side: Side::Buy,
            price,
            qty: 1,
            tif,
            post_only: true,
        })
    };
    assert_eq!(
        h.cmd(post(55, TimeInForce::Ioc)),
        rejected(RejectReason::InvalidOrder)
    );
    assert_eq!(
        h.cmd(post(60, TimeInForce::Gtc)),
        rejected(RejectReason::PostOnlyWouldCross)
    );
    let id = h.next_id();
    assert_eq!(
        h.cmd(post(59, TimeInForce::Gtc)),
        vec![
            Event::Accepted { order: id },
            Event::Rested {
                order: id,
                remaining: q(1)
            }
        ]
    );
    assert_eq!(h.book(Side::Sell), vec![(ask.get(), 60, 5)]);
}

#[test]
fn better_price_first_then_time_and_trades_at_the_makers_price() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 55, 2);
    let b = h.gtc(CAROL, Side::Sell, 55, 2);
    let c = h.gtc(BOB, Side::Sell, 54, 1); // better price, later in time
    let taker = h.next_id();
    let events = h.order(ALICE, Side::Buy, 60, 4, TimeInForce::Gtc);
    assert_eq!(
        events,
        vec![
            Event::Accepted { order: taker },
            trade(c, taker, BOB, ALICE, Side::Buy, 54, 1),
            trade(a, taker, BOB, ALICE, Side::Buy, 55, 2),
            trade(b, taker, CAROL, ALICE, Side::Buy, 55, 1),
        ]
    );
    assert_eq!(h.book(Side::Sell), vec![(b.get(), 55, 1)]);
    assert_eq!(h.book(Side::Buy), EMPTY_SIDE);
}

#[test]
fn a_gtc_remainder_rests_after_partial_fills() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Buy, 40, 3);
    let taker = h.next_id();
    let events = h.order(ALICE, Side::Sell, 40, 5, TimeInForce::Gtc);
    assert_eq!(
        events,
        vec![
            Event::Accepted { order: taker },
            trade(a, taker, BOB, ALICE, Side::Sell, 40, 3),
            Event::Rested {
                order: taker,
                remaining: q(2)
            },
        ]
    );
    assert_eq!(h.book(Side::Sell), vec![(taker.get(), 40, 2)]);
}

#[test]
fn ioc_cancels_its_remainder_including_when_nothing_crosses() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 30, 2);
    let taker = h.next_id();
    assert_eq!(
        h.order(ALICE, Side::Buy, 30, 5, TimeInForce::Ioc),
        vec![
            Event::Accepted { order: taker },
            trade(a, taker, BOB, ALICE, Side::Buy, 30, 2),
            Event::Cancelled {
                order: taker,
                remaining: q(3),
                reason: CancelReason::IocRemainder
            },
        ]
    );
    let lone = h.next_id();
    assert_eq!(
        h.order(ALICE, Side::Buy, 30, 4, TimeInForce::Ioc),
        vec![
            Event::Accepted { order: lone },
            Event::Cancelled {
                order: lone,
                remaining: q(4),
                reason: CancelReason::IocRemainder
            },
        ]
    );
    assert_eq!(h.book(Side::Buy), EMPTY_SIDE);
}

#[test]
fn fok_fills_completely_across_levels_or_rejects_without_trading() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 50, 2);
    let b = h.gtc(CAROL, Side::Sell, 52, 2);
    let before = (h.book(Side::Sell), h.engine.state_hash().unwrap());
    assert_eq!(
        h.order(ALICE, Side::Buy, 52, 5, TimeInForce::Fok),
        rejected(RejectReason::FokUnfillable)
    );
    assert_eq!(
        h.order(ALICE, Side::Buy, 51, 3, TimeInForce::Fok),
        rejected(RejectReason::FokUnfillable)
    );
    assert_eq!(
        (h.book(Side::Sell), h.engine.state_hash().unwrap()).0,
        before.0
    );
    let taker = h.next_id();
    assert_eq!(
        h.order(ALICE, Side::Buy, 52, 3, TimeInForce::Fok),
        vec![
            Event::Accepted { order: taker },
            trade(a, taker, BOB, ALICE, Side::Buy, 50, 2),
            trade(b, taker, CAROL, ALICE, Side::Buy, 52, 1),
        ]
    );
}

#[test]
fn fok_counts_liquidity_only_up_to_the_accounts_own_order() {
    let mut h = Harness::open();
    h.gtc(BOB, Side::Sell, 50, 1);
    h.gtc(ALICE, Side::Sell, 50, 5); // self-trade prevention would stop here
    h.gtc(CAROL, Side::Sell, 50, 5);
    assert_eq!(
        h.order(ALICE, Side::Buy, 50, 2, TimeInForce::Fok),
        rejected(RejectReason::FokUnfillable)
    );
}

#[test]
fn self_trade_prevention_cancels_the_taker_and_keeps_earlier_trades() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 50, 1);
    let own = h.gtc(ALICE, Side::Sell, 50, 3);
    h.gtc(CAROL, Side::Sell, 50, 3);
    let taker = h.next_id();
    assert_eq!(
        h.order(ALICE, Side::Buy, 50, 4, TimeInForce::Gtc),
        vec![
            Event::Accepted { order: taker },
            trade(a, taker, BOB, ALICE, Side::Buy, 50, 1),
            Event::Cancelled {
                order: taker,
                remaining: q(3),
                reason: CancelReason::SelfTrade
            },
        ]
    );
    // The account's own resting order is untouched and keeps its place.
    assert_eq!(h.book(Side::Sell).first(), Some(&(own.get(), 50, 3)));
    assert_eq!(h.book(Side::Buy), EMPTY_SIDE);
}

#[test]
fn cancel_only_reaches_the_owners_live_orders() {
    let mut h = Harness::open();
    let a = h.gtc(ALICE, Side::Buy, 45, 2);
    assert_eq!(
        h.cmd(Command::Cancel {
            account: BOB,
            order: a
        }),
        rejected(RejectReason::UnknownOrder)
    );
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Halted,
    });
    assert_eq!(
        h.cmd(Command::Cancel {
            account: ALICE,
            order: a
        }),
        vec![Event::Cancelled {
            order: a,
            remaining: q(2),
            reason: CancelReason::User
        }]
    );
    assert_eq!(
        h.cmd(Command::Cancel {
            account: ALICE,
            order: a
        }),
        rejected(RejectReason::UnknownOrder)
    );
    assert_eq!(
        h.cmd(Command::Cancel {
            account: ALICE,
            order: OrderId::new(999)
        }),
        rejected(RejectReason::UnknownOrder)
    );
}

#[test]
fn amend_down_keeps_priority_and_everything_else_is_rejected() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 50, 5);
    let b = h.gtc(CAROL, Side::Sell, 50, 5);
    let amend = |order, qty| Command::Amend {
        account: BOB,
        order,
        qty,
    };
    assert_eq!(h.cmd(amend(a, 5)), rejected(RejectReason::InvalidAmend));
    assert_eq!(h.cmd(amend(a, 6)), rejected(RejectReason::InvalidAmend));
    assert_eq!(h.cmd(amend(a, 0)), rejected(RejectReason::InvalidQuantity));
    assert_eq!(
        h.cmd(Command::Amend {
            account: ALICE,
            order: a,
            qty: 1
        }),
        rejected(RejectReason::UnknownOrder)
    );
    assert_eq!(
        h.cmd(amend(a, 2)),
        vec![Event::Amended {
            order: a,
            remaining: q(2)
        }]
    );
    // Still first in time: the next buy fills the amended order before b.
    let taker = h.next_id();
    let events = h.order(ALICE, Side::Buy, 50, 3, TimeInForce::Ioc);
    assert_eq!(
        events.get(1),
        Some(&trade(a, taker, BOB, ALICE, Side::Buy, 50, 2))
    );
    assert_eq!(
        events.get(2),
        Some(&trade(b, taker, CAROL, ALICE, Side::Buy, 50, 1))
    );
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Halted,
    });
    assert_eq!(
        h.cmd(Command::Amend {
            account: CAROL,
            order: b,
            qty: 1
        }),
        rejected(RejectReason::MarketNotOpen)
    );
}

#[test]
fn closing_cancels_every_resting_order_in_ascending_id_after_the_state_event() {
    let mut h = Harness::open();
    let a = h.gtc(BOB, Side::Sell, 70, 1);
    let b = h.gtc(ALICE, Side::Buy, 20, 2);
    let c = h.gtc(CAROL, Side::Sell, 60, 3);
    assert_eq!(
        h.cmd(Command::SetState {
            market: M,
            state: MarketState::Closed
        }),
        vec![
            Event::MarketStateChanged {
                market: M,
                state: MarketState::Closed
            },
            Event::Cancelled {
                order: a,
                remaining: q(1),
                reason: CancelReason::MarketClosed
            },
            Event::Cancelled {
                order: b,
                remaining: q(2),
                reason: CancelReason::MarketClosed
            },
            Event::Cancelled {
                order: c,
                remaining: q(3),
                reason: CancelReason::MarketClosed
            },
        ]
    );
    assert_eq!(h.book(Side::Buy), EMPTY_SIDE);
    assert_eq!(h.book(Side::Sell), EMPTY_SIDE);
    assert_eq!(
        h.order(ALICE, Side::Buy, 50, 1, TimeInForce::Gtc),
        rejected(RejectReason::MarketNotOpen)
    );
    assert_eq!(
        h.cmd(Command::SetState {
            market: M,
            state: MarketState::Open
        }),
        rejected(RejectReason::InvalidTransition)
    );
    assert_eq!(
        h.cmd(Command::SetState {
            market: M,
            state: MarketState::Resolved
        }),
        vec![Event::MarketStateChanged {
            market: M,
            state: MarketState::Resolved
        }]
    );
}

#[test]
fn the_state_hash_tracks_the_book_exactly() {
    let build = |extra: bool| {
        let mut h = Harness::open();
        h.gtc(BOB, Side::Sell, 55, 2);
        if extra {
            h.gtc(ALICE, Side::Buy, 10, 1);
        }
        h
    };
    assert_eq!(
        build(false).engine.state_hash().unwrap(),
        build(false).engine.state_hash().unwrap()
    );
    assert_ne!(
        build(false).engine.state_hash().unwrap(),
        build(true).engine.state_hash().unwrap()
    );
}
