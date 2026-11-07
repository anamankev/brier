//! Live projection equals a rebuild from the journal (docs/gates/G4-plan.md, unit 4).
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-readmodel -- --ignored` (also run by `make test-all` and the CI integration job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects
)] // test code

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use brier_core::{CoreConfig, start};
use brier_engine::{Command, MarketState, NewOrder, TimeInForce};
use brier_readmodel::{Projector, migrate};
use brier_types::{AccountId, MarketId, OrderId, Outcome, Side};
use tokio_postgres::{Client, NoTls};

async fn connect(schema: &str) -> Client {
    let url = std::env::var("BRIER_TEST_DATABASE_URL")
        .expect("set BRIER_TEST_DATABASE_URL (make db-url)");
    let (mut client, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema}; SET search_path TO {schema};"
        ))
        .await
        .unwrap();
    migrate(&mut client).await.unwrap();
    client
}

/// A deterministic workload: three accounts trading one market, then close and settle.
fn workload() -> Vec<Command> {
    let mut out = vec![
        Command::OpenAccount {
            account: AccountId::new(1),
        },
        Command::OpenAccount {
            account: AccountId::new(2),
        },
        Command::OpenAccount {
            account: AccountId::new(3),
        },
        Command::CreateMarket {
            market: MarketId::new(1),
            rules_hash: None,
        },
        Command::SetState {
            market: MarketId::new(1),
            state: MarketState::Open,
        },
    ];
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for i in 0..3_000u64 {
        let r = next();
        let account = AccountId::new(1 + r % 3);
        if r % 10 < 7 {
            out.push(Command::Place(NewOrder {
                account,
                market: MarketId::new(1),
                side: if (r >> 8) % 2 == 0 {
                    Side::Buy
                } else {
                    Side::Sell
                },
                price: u8::try_from(45 + (r >> 16) % 11).unwrap(),
                qty: u32::try_from(1 + (r >> 24) % 20).unwrap(),
                tif: if (r >> 32) % 5 == 0 {
                    TimeInForce::Ioc
                } else {
                    TimeInForce::Gtc
                },
                post_only: false,
                client_order_id: ((r >> 40) % 2 == 0).then_some(i % 500),
            }));
        } else {
            out.push(Command::Cancel {
                account,
                order: OrderId::new(1 + (r >> 8) % (5 + i)),
            });
        }
    }
    out.push(Command::SetState {
        market: MarketId::new(1),
        state: MarketState::Closed,
    });
    out.push(Command::Settle {
        market: MarketId::new(1),
        outcome: Outcome::Yes,
        evidence_hash: None,
    });
    out
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs PostgreSQL: make up, then BRIER_TEST_DATABASE_URL=$(make db-url)"]
async fn live_projection_equals_a_rebuild_from_the_journal() {
    let tag = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        % 1_000_000_000;
    let (live_schema, rebuild_schema) = (format!("live_{tag}"), format!("rebuild_{tag}"));
    let dir = tempfile::tempdir().unwrap();
    let (handle, _, core) = start(CoreConfig::new(dir.path())).unwrap();
    let commands = workload();
    let total = commands.len() as u64;

    // Live: project while the core is still taking commands.
    let mut live = connect(&live_schema).await;
    let mut projector = Projector::open(&live, dir.path()).await.unwrap();
    let mut durable = handle.durable();
    let submitter = handle.clone();
    let submitting = tokio::task::spawn_blocking(move || {
        let receivers: Vec<_> = commands
            .into_iter()
            .map(|c| submitter.submit_nowait(c).unwrap())
            .collect();
        let mut acknowledged: u64 = 0;
        for r in receivers {
            r.blocking_recv().unwrap().unwrap();
            acknowledged += 1;
        }
        acknowledged
    });
    while projector.cursor() < total {
        let bound = *durable.borrow_and_update();
        if projector.project(&mut live, bound, 250).await.unwrap() == 0 {
            let _ = tokio::time::timeout(Duration::from_millis(50), durable.changed()).await;
        }
    }
    assert_eq!(submitting.await.unwrap(), total);

    // Rebuild: a fresh schema, projected from the journal alone.
    let mut rebuild = connect(&rebuild_schema).await;
    let mut fresh = Projector::open(&rebuild, dir.path()).await.unwrap();
    while fresh.project(&mut rebuild, total, 10_000).await.unwrap() > 0 {}
    assert_eq!(fresh.cursor(), total);

    for (table, order) in [("fills", "seq, ordinal"), ("order_events", "seq, ordinal")] {
        let sql = format!("SELECT t::text FROM {table} t ORDER BY {order}");
        let (a, b) = (rows(&live, &sql).await, rows(&rebuild, &sql).await);
        assert!(!a.is_empty(), "{table} has rows");
        assert_eq!(a, b, "{table}: live and rebuilt differ");
    }
    // Duplicates and rejections were exercised, not just happy paths.
    let kinds = rows(
        &live,
        "SELECT DISTINCT kind FROM order_events ORDER BY kind",
    )
    .await;
    assert!(
        kinds.contains(&"duplicate".to_owned()) && kinds.contains(&"cancelled".to_owned()),
        "{kinds:?}"
    );

    // A restarted projector resumes at its cursor and finds nothing new.
    let mut resumed = Projector::open(&live, dir.path()).await.unwrap();
    assert_eq!(resumed.cursor(), total);
    assert_eq!(resumed.project(&mut live, total, 10_000).await.unwrap(), 0);

    live.batch_execute(&format!(
        "DROP SCHEMA {live_schema} CASCADE; DROP SCHEMA {rebuild_schema} CASCADE"
    ))
    .await
    .unwrap();
    drop(handle);
    core.join().unwrap().unwrap();
}
