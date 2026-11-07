//! The core library through its handle: submit, query, subscribe, durability, restart.
#![allow(clippy::unwrap_used)] // a failure must stop the test

use brier_core::{Answer, CoreConfig, Query, start};
use brier_engine::{Command, Event, MarketState, NewOrder, TimeInForce};
use brier_types::{AccountId, MarketId, Price, Seq, Side};

const M: MarketId = MarketId::new(1);
const A: AccountId = AccountId::new(1);
const B: AccountId = AccountId::new(2);

fn setup() -> Vec<Command> {
    vec![
        Command::OpenAccount { account: A },
        Command::OpenAccount { account: B },
        Command::CreateMarket { market: M },
        Command::SetState {
            market: M,
            state: MarketState::Open,
        },
        Command::Place(NewOrder {
            account: A,
            market: M,
            side: Side::Sell,
            price: 60,
            qty: 5,
            tif: TimeInForce::Gtc,
            post_only: false,
        }),
        Command::Place(NewOrder {
            account: A,
            market: M,
            side: Side::Sell,
            price: 60,
            qty: 2,
            tif: TimeInForce::Gtc,
            post_only: false,
        }),
    ]
}

#[tokio::test(flavor = "current_thread")]
async fn commands_are_acknowledged_in_order_and_queries_see_them() {
    let dir = tempfile::tempdir().unwrap();
    let (handle, recovery, core) = start(CoreConfig::new(dir.path())).unwrap();
    assert_eq!(recovery.next_seq, Seq::FIRST);
    let mut batches = handle.subscribe();
    let mut durable = handle.durable();
    for (i, command) in setup().into_iter().enumerate() {
        let c = handle.submit(command).await.unwrap();
        assert_eq!(c.seq, Seq::from_raw(u64::try_from(i).unwrap() + 1));
    }
    assert_eq!(*durable.borrow_and_update(), 6);
    // Every durable command reaches subscribers, in order.
    let mut seen = Vec::new();
    while seen.len() < 6 {
        let batch = batches.recv().await.unwrap();
        seen.extend(batch.iter().map(|c| c.seq.get()));
    }
    assert_eq!(seen, (1..=6).collect::<Vec<_>>());
    // Queries answer from durable state: one level of 7 at 60.
    let Answer::Book {
        as_of, asks, bids, ..
    } = handle.query(Query::Book(M)).await.unwrap()
    else {
        panic!()
    };
    assert_eq!((as_of, bids.len()), (6, 0));
    assert_eq!(asks, vec![(Price::new(60).unwrap(), 7)]);
    let Answer::Account {
        view, open_orders, ..
    } = handle.query(Query::Account(A)).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(open_orders.len(), 2);
    assert_eq!(view.unwrap().reserved.cents(), 7 * 40);
    drop((handle, batches));
    core.join().unwrap().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_restart_recovers_every_acknowledged_command() {
    let dir = tempfile::tempdir().unwrap();
    let mut config = CoreConfig::new(dir.path());
    config.snapshot_every = 4; // a snapshot mid-way, then a journal tail
    let (handle, _, core) = start(config.clone()).unwrap();
    let mut last = None;
    for command in setup() {
        last = Some(handle.submit(command).await.unwrap());
    }
    drop(handle);
    core.join().unwrap().unwrap();
    let (handle, recovery, core) = start(config).unwrap();
    assert_eq!(recovery.next_seq, Seq::from_raw(7));
    assert!(
        recovery.snapshot.is_some(),
        "recovered from the snapshot, then the tail"
    );
    let taker = handle
        .submit(Command::Place(NewOrder {
            account: B,
            market: M,
            side: Side::Buy,
            price: 60,
            qty: 6,
            tif: TimeInForce::Ioc,
            post_only: false,
        }))
        .await
        .unwrap();
    assert_eq!(taker.seq, Seq::from_raw(7));
    assert_eq!(
        taker
            .events
            .iter()
            .filter(|e| matches!(e, Event::Trade(_)))
            .count(),
        2
    );
    assert!(last.is_some());
    drop(handle);
    core.join().unwrap().unwrap();
}
