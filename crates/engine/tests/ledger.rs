//! One test per money rule in docs/protocol.md §3 and §5 (SPEC §2.3-§2.4, §4). Every command also
//! re-checks every engine and ledger invariant (I1-I5).
#![allow(clippy::arithmetic_side_effects)] // test bookkeeping (sequence counter)
#![allow(clippy::unwrap_used)] // test helpers: a failure must stop the test

use brier_engine::{
    CancelReason, Command, Engine, Entry, EntryKind, Event, LedgerAccount, MAX_RESTING_ORDERS,
    MarketState, NewOrder, RejectReason, SIGNUP_GRANT, Sequenced, TimeInForce,
};
use brier_types::{AccountId, Credits, MarketId, OrderId, Outcome, Seq, Side};

const M: MarketId = MarketId::new(1);
const M2: MarketId = MarketId::new(2);
const ALICE: AccountId = AccountId::new(1);
const BOB: AccountId = AccountId::new(2);
const CAROL: AccountId = AccountId::new(3);

struct Harness {
    engine: Engine,
    seq: u64,
}

impl Harness {
    /// Accounts ALICE, BOB, CAROL opened; markets M and M2 listed and open.
    fn new() -> Self {
        let mut h = Self {
            engine: Engine::new(),
            seq: 0,
        };
        for account in [ALICE, BOB, CAROL] {
            h.cmd(Command::OpenAccount { account });
        }
        for market in [M, M2] {
            h.cmd(Command::CreateMarket { market });
            h.cmd(Command::SetState {
                market,
                state: MarketState::Open,
            });
        }
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

    fn next_id(&self) -> OrderId {
        OrderId::new(self.seq + 1)
    }

    fn order(
        &mut self,
        account: AccountId,
        market: MarketId,
        side: Side,
        price: u8,
        qty: u32,
        tif: TimeInForce,
    ) -> Vec<Event> {
        self.cmd(Command::Place(NewOrder {
            account,
            market,
            side,
            price,
            qty,
            tif,
            post_only: false,
        }))
    }

    fn cash(&self, a: AccountId) -> i64 {
        self.engine
            .accounts()
            .into_iter()
            .find(|v| v.account == a)
            .unwrap()
            .cash
            .cents()
    }

    fn reserved(&self, a: AccountId) -> i64 {
        self.engine
            .accounts()
            .into_iter()
            .find(|v| v.account == a)
            .unwrap()
            .reserved
            .cents()
    }

    fn position(&self, a: AccountId, m: MarketId) -> i64 {
        let view = self
            .engine
            .accounts()
            .into_iter()
            .find(|v| v.account == a)
            .unwrap();
        view.positions
            .iter()
            .find(|(market, _)| *market == m)
            .map_or(0, |&(_, x)| x)
    }
}

fn c(cents: i64) -> Credits {
    Credits::from_cents(cents)
}

fn entries(events: &[Event]) -> Vec<(EntryKind, Vec<(LedgerAccount, i64)>)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Entry(entry) => Some((
                entry.kind(),
                entry.legs().iter().map(|&(a, x)| (a, x.cents())).collect(),
            )),
            _ => None,
        })
        .collect()
}

fn reserved_events(events: &[Event]) -> Vec<(AccountId, MarketId, i64)> {
    events
        .iter()
        .filter_map(|e| match *e {
            Event::Reserved {
                account,
                market,
                amount,
            } => Some((account, market, amount.cents())),
            _ => None,
        })
        .collect()
}

#[test]
fn opening_an_account_grants_once_from_issuance() {
    let mut engine = Engine::new();
    let mut out = Vec::new();
    engine
        .apply(
            &Sequenced {
                seq: Seq::FIRST,
                command: Command::OpenAccount { account: ALICE },
            },
            &mut out,
        )
        .unwrap();
    // SPEC §1.2: SIGNUP_GRANT = 1,000,000 credit-cents (10,000 credits).
    assert_eq!(SIGNUP_GRANT, c(1_000_000));
    assert_eq!(
        out.first(),
        Some(&Event::AccountOpened {
            account: ALICE,
            cash: SIGNUP_GRANT
        })
    );
    assert_eq!(
        entries(&out),
        vec![(
            EntryKind::Grant,
            vec![
                (LedgerAccount::Issuance, -1_000_000),
                (LedgerAccount::Cash(ALICE), 1_000_000)
            ]
        )]
    );
    let mut again = Vec::new();
    engine
        .apply(
            &Sequenced {
                seq: Seq::from_raw(2),
                command: Command::OpenAccount { account: ALICE },
            },
            &mut again,
        )
        .unwrap();
    assert_eq!(
        again,
        vec![Event::Rejected {
            reason: RejectReason::AccountExists
        }]
    );
    engine.check_invariants().unwrap();
}

#[test]
fn orders_from_unknown_accounts_are_rejected_after_the_market_check() {
    let mut h = Harness::new();
    let ghost = AccountId::new(99);
    assert_eq!(
        h.order(ghost, MarketId::new(42), Side::Buy, 50, 1, TimeInForce::Gtc),
        vec![Event::Rejected {
            reason: RejectReason::UnknownMarket
        }]
    );
    assert_eq!(
        h.order(ghost, M, Side::Buy, 50, 1, TimeInForce::Gtc),
        vec![Event::Rejected {
            reason: RejectReason::UnknownAccount
        }]
    );
}

#[test]
fn two_opening_sides_create_pairs_backed_by_100_in_escrow() {
    let mut h = Harness::new();
    let resting = h.order(BOB, M, Side::Sell, 40, 10, TimeInForce::Gtc);
    // BOB sells YES without holding any, so each unit opens NO at cost 100 − 40 = 60.
    assert_eq!(reserved_events(&resting), vec![(BOB, M, 600)]);
    let events = h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc);
    assert_eq!(
        entries(&events),
        vec![(
            EntryKind::Trade,
            vec![
                (LedgerAccount::Cash(ALICE), -400),
                (LedgerAccount::Cash(BOB), -600),
                (LedgerAccount::Escrow(M), 1_000)
            ]
        )]
    );
    assert_eq!(reserved_events(&events), vec![(BOB, M, 0)]);
    assert_eq!((h.position(ALICE, M), h.position(BOB, M)), (10, -10));
    assert_eq!(h.engine.escrow(M), Some(c(1_000)));
}

#[test]
fn selling_held_contracts_transfers_them_and_needs_no_collateral() {
    let mut h = Harness::new();
    h.order(BOB, M, Side::Sell, 40, 10, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc); // ALICE +10 YES
    let rest = h.order(ALICE, M, Side::Sell, 45, 4, TimeInForce::Gtc);
    assert_eq!(reserved_events(&rest), vec![], "held YES covers the sell");
    let events = h.order(CAROL, M, Side::Buy, 45, 4, TimeInForce::Gtc);
    // A transfer: escrow unchanged, so its zero leg is dropped.
    assert_eq!(
        entries(&events),
        vec![(
            EntryKind::Trade,
            vec![
                (LedgerAccount::Cash(CAROL), -180),
                (LedgerAccount::Cash(ALICE), 180)
            ]
        )]
    );
    assert_eq!((h.position(ALICE, M), h.position(CAROL, M)), (6, 4));
}

#[test]
fn two_closing_sides_destroy_pairs_and_release_escrow() {
    let mut h = Harness::new();
    h.order(BOB, M, Side::Sell, 40, 10, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc); // ALICE +10 YES, BOB 10 NO
    h.order(ALICE, M, Side::Sell, 30, 6, TimeInForce::Gtc);
    let events = h.order(BOB, M, Side::Buy, 30, 6, TimeInForce::Gtc);
    // BOB closes 6 NO: pays 30 and receives 100 per unit. ALICE closes 6 YES: receives 30.
    assert_eq!(
        entries(&events),
        vec![(
            EntryKind::Trade,
            vec![
                (LedgerAccount::Cash(BOB), 420),
                (LedgerAccount::Cash(ALICE), 180),
                (LedgerAccount::Escrow(M), -600)
            ]
        )]
    );
    assert_eq!(h.engine.escrow(M), Some(c(400)));
}

#[test]
fn held_contracts_cover_only_the_cheapest_units_of_a_larger_order() {
    let mut h = Harness::new();
    h.order(BOB, M, Side::Sell, 40, 10, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc); // ALICE +10 YES
    // Selling 15 at 60: 10 are covered by held YES; 5 open NO at 100 − 60 = 40 each.
    let events = h.order(ALICE, M, Side::Sell, 60, 15, TimeInForce::Gtc);
    assert_eq!(reserved_events(&events), vec![(ALICE, M, 200)]);
    assert_eq!(h.reserved(ALICE), 200);
}

#[test]
fn cancel_and_amend_release_collateral() {
    let mut h = Harness::new();
    let id = h.next_id();
    assert_eq!(
        reserved_events(&h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc)),
        vec![(ALICE, M, 400)]
    );
    assert_eq!(
        reserved_events(&h.cmd(Command::Amend {
            account: ALICE,
            order: id,
            qty: 4
        })),
        vec![(ALICE, M, 160)]
    );
    assert_eq!(
        reserved_events(&h.cmd(Command::Cancel {
            account: ALICE,
            order: id
        })),
        vec![(ALICE, M, 0)]
    );
    assert_eq!(h.reserved(ALICE), 0);
}

#[test]
fn collateral_must_cover_the_whole_order_exactly() {
    let mut h = Harness::new();
    // 20,000 × 50 = 1,000,000 = the whole grant: affordable, exactly.
    let ok = h.order(ALICE, M, Side::Buy, 50, 20_000, TimeInForce::Gtc);
    assert!(matches!(ok.first(), Some(Event::Accepted { .. })));
    // Nothing left: any further opening order is refused, IOC included.
    assert_eq!(
        h.order(ALICE, M2, Side::Buy, 1, 1, TimeInForce::Ioc),
        vec![Event::Rejected {
            reason: RejectReason::InsufficientCollateral
        }]
    );
    // 25,000 × 41 = 1,025,000 exceeds BOB's 1,000,000.
    assert_eq!(
        h.order(BOB, M, Side::Buy, 41, 25_000, TimeInForce::Gtc),
        vec![Event::Rejected {
            reason: RejectReason::InsufficientCollateral
        }]
    );
    assert_eq!(h.cash(ALICE), 1_000_000);
}

#[test]
fn the_resting_order_limit_applies_to_gtc_only() {
    let mut h = Harness::new();
    for _ in 0..MAX_RESTING_ORDERS {
        h.order(ALICE, M, Side::Buy, 1, 1, TimeInForce::Gtc);
    }
    assert_eq!(
        h.order(ALICE, M, Side::Buy, 1, 1, TimeInForce::Gtc),
        vec![Event::Rejected {
            reason: RejectReason::TooManyOrders
        }]
    );
    let ioc = h.order(ALICE, M, Side::Buy, 1, 1, TimeInForce::Ioc);
    assert!(matches!(ioc.first(), Some(Event::Accepted { .. })));
}

fn open_interest_of_four() -> Harness {
    // Ends with BOB 4 NO, CAROL 4 YES, and 400 in escrow (ALICE flat).
    let mut h = Harness::new();
    h.order(BOB, M, Side::Sell, 40, 10, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Buy, 40, 10, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Sell, 45, 4, TimeInForce::Gtc);
    h.order(CAROL, M, Side::Buy, 45, 4, TimeInForce::Gtc);
    h.order(ALICE, M, Side::Sell, 30, 6, TimeInForce::Gtc);
    h.order(BOB, M, Side::Buy, 30, 6, TimeInForce::Gtc);
    assert_eq!(
        (
            h.position(ALICE, M),
            h.position(BOB, M),
            h.position(CAROL, M)
        ),
        (0, -4, 4)
    );
    assert_eq!(h.engine.escrow(M), Some(c(400)));
    h
}

#[test]
fn settlement_needs_a_closed_market_and_pays_the_winning_side() {
    let mut h = open_interest_of_four();
    assert_eq!(
        h.cmd(Command::Settle {
            market: M,
            outcome: Outcome::Yes
        }),
        vec![Event::Rejected {
            reason: RejectReason::InvalidTransition
        }]
    );
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Closed,
    });
    let carol_before = h.cash(CAROL);
    let events = h.cmd(Command::Settle {
        market: M,
        outcome: Outcome::Yes,
    });
    assert_eq!(
        events.first(),
        Some(&Event::Settled {
            market: M,
            outcome: Outcome::Yes
        })
    );
    assert_eq!(
        entries(&events),
        vec![(
            EntryKind::Settle,
            vec![
                (LedgerAccount::Escrow(M), -400),
                (LedgerAccount::Cash(CAROL), 400)
            ]
        )]
    );
    assert_eq!(h.cash(CAROL), carol_before + 400);
    assert_eq!(h.engine.escrow(M), Some(Credits::ZERO));
    assert_eq!((h.position(BOB, M), h.position(CAROL, M)), (0, 0));
    assert_eq!(h.engine.market_state(M), Some(MarketState::Resolved));
    // The settled market leaves nothing behind in any account (SPEC §2.4).
    assert!(
        h.engine
            .accounts()
            .iter()
            .all(|v| v.positions.iter().all(|&(m, _)| m != M))
    );
}

#[test]
fn a_void_pays_50_per_contract_on_both_sides() {
    let mut h = open_interest_of_four();
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Closed,
    });
    let events = h.cmd(Command::Settle {
        market: M,
        outcome: Outcome::Void,
    });
    // ADR-0002: 50 × 4 to each side, in ascending account order.
    assert_eq!(
        entries(&events),
        vec![
            (
                EntryKind::Settle,
                vec![
                    (LedgerAccount::Escrow(M), -200),
                    (LedgerAccount::Cash(BOB), 200)
                ]
            ),
            (
                EntryKind::Settle,
                vec![
                    (LedgerAccount::Escrow(M), -200),
                    (LedgerAccount::Cash(CAROL), 200)
                ]
            ),
        ]
    );
    assert_eq!(h.engine.escrow(M), Some(Credits::ZERO));
    assert_eq!(h.engine.market_state(M), Some(MarketState::Voided));
}

#[test]
fn cancel_all_filters_by_account_and_market_in_ascending_order() {
    let mut h = Harness::new();
    let a1 = h.next_id();
    h.order(ALICE, M, Side::Buy, 10, 1, TimeInForce::Gtc);
    let b1 = h.next_id();
    h.order(BOB, M2, Side::Buy, 10, 2, TimeInForce::Gtc);
    let a2 = h.next_id();
    h.order(ALICE, M2, Side::Buy, 10, 3, TimeInForce::Gtc);
    let cancelled = |events: &[Event]| -> Vec<OrderId> {
        events
            .iter()
            .filter_map(|e| match *e {
                Event::Cancelled {
                    order,
                    reason: CancelReason::KillSwitch,
                    ..
                } => Some(order),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        h.cmd(Command::CancelAll {
            account: Some(AccountId::new(99)),
            market: None
        }),
        vec![Event::Rejected {
            reason: RejectReason::UnknownAccount
        }]
    );
    assert_eq!(
        h.cmd(Command::CancelAll {
            account: None,
            market: Some(MarketId::new(99))
        }),
        vec![Event::Rejected {
            reason: RejectReason::UnknownMarket
        }]
    );
    assert_eq!(
        cancelled(&h.cmd(Command::CancelAll {
            account: Some(ALICE),
            market: Some(M2)
        })),
        vec![a2]
    );
    assert_eq!(
        cancelled(&h.cmd(Command::CancelAll {
            account: None,
            market: None
        })),
        vec![a1, b1]
    );
    assert_eq!((h.reserved(ALICE), h.reserved(BOB)), (0, 0));
}

#[test]
fn halt_all_halts_only_open_markets_in_ascending_order() {
    let mut h = Harness::new();
    h.cmd(Command::SetState {
        market: M2,
        state: MarketState::Halted,
    });
    assert_eq!(
        h.cmd(Command::HaltAll),
        vec![Event::MarketStateChanged {
            market: M,
            state: MarketState::Halted
        }]
    );
    h.cmd(Command::SetState {
        market: M,
        state: MarketState::Open,
    });
    h.cmd(Command::SetState {
        market: M2,
        state: MarketState::Open,
    });
    assert_eq!(
        h.cmd(Command::HaltAll),
        vec![
            Event::MarketStateChanged {
                market: M,
                state: MarketState::Halted
            },
            Event::MarketStateChanged {
                market: M2,
                state: MarketState::Halted
            },
        ]
    );
}

#[test]
fn an_unbalanced_entry_cannot_be_built() {
    assert!(
        Entry::new(
            EntryKind::Trade,
            &[
                (LedgerAccount::Cash(ALICE), c(-5)),
                (LedgerAccount::Cash(BOB), c(4))
            ]
        )
        .is_err()
    );
    let e = Entry::new(
        EntryKind::Trade,
        &[
            (LedgerAccount::Cash(ALICE), c(-5)),
            (LedgerAccount::Escrow(M), c(0)),
            (LedgerAccount::Cash(BOB), c(5)),
        ],
    )
    .unwrap();
    assert_eq!(e.legs().len(), 2, "zero legs are dropped");
}
