//! Forecasts, forecaster scores, and both leaderboards, on a manual clock, checked against a
//! scoring example computed by hand (docs/gates/G6-plan.md).
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::panic
)] // test code

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::Router;
use axum::http::{Method, StatusCode};
use brier_core::{CoreConfig, start};
use brier_engine::Command;
use brier_gateway::clock::Clock;
use brier_gateway::scheduler::tick;
use brier_gateway::{AppState, router, run_projector};
use brier_rules::time::instant;
use brier_types::{MarketId, Outcome};
use common::{call, connect, login};
use serde_json::{Value, json};

const MARKETS: std::ops::RangeInclusive<u32> = 21..=30;

fn cpi(market: u32) -> Value {
    json!({
        "schema": 1, "market": market,
        "title": format!("CPI test market {market}"),
        "question": "Will the 12-month change in CPI-U for September 2026 be above 3.0%?",
        "observation": "2026-09", "revision": "first_release",
        "void": ["no_value_by_deadline", "source_discontinued"],
        "source": { "provider": "bls", "series": "CUUR0000SA0", "url": "https://data.bls.gov/timeseries/CUUR0000SA0" },
        "condition": { "transform": "pct_change_12m", "op": ">", "threshold": "3.0" },
        "schedule": {
            "open": "2026-09-01T13:30:00Z", "close": "2026-10-15T12:00:00Z",
            "release": "2026-10-15T12:30:00Z", "deadline": "2026-12-15T12:30:00Z",
        },
    })
}

struct World {
    app: Router,
    state: Arc<AppState>,
    clock: Clock,
}

impl World {
    async fn at(&self, t: i64) {
        self.clock.set(t);
        tick(&self.state).await.unwrap();
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        token: &str,
        body: Value,
    ) -> (StatusCode, Value) {
        call(&self.app, method, path, Some(token), Some(body)).await
    }

    async fn forecast(&self, token: &str, market: u32, p: i64) -> StatusCode {
        let path = format!("/v1/markets/{market}/forecast");
        self.send(Method::PUT, &path, token, json!({ "probability": p }))
            .await
            .0
    }

    async fn projected(&self) {
        let target = *self.state.core.durable().borrow();
        for _ in 0..500 {
            if self.state.projected.load(Ordering::Acquire) >= target {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the projector did not reach {target}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn forecasts_are_scored_against_the_market_and_ranked() {
    let schema = common::schema("gateway_forecasts");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let t0 = instant("2026-09-01T13:30:00Z").unwrap();
    let clock = Clock::manual(t0 - 3_600);
    // Several users sign up and sign in from one test address, beyond the per-address auth
    // burst; the limit itself is tested in rest.rs.
    let mut base = AppState::new(core.clone(), connect(&schema).await).unwrap();
    base.auth_limits = brier_gateway::limits::Limiter::per_minute(1_000, 1_000);
    let state = Arc::new(
        base.with_totp_key(common::TOTP_KEY)
            .with_clock(clock.clone()),
    );
    tokio::spawn(run_projector(
        connect(&schema).await,
        data.path().to_owned(),
        core.durable(),
        Arc::clone(&state.projected),
    ));
    let w = World {
        app: router(Arc::clone(&state)),
        state,
        clock,
    };
    let db = connect(&schema).await;
    let pw = "a long enough password";
    let names = ["alice", "rex", "ann", "ben", "cal", "bob", "dave"];
    for name in names {
        let (status, _) = call(
            &w.app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": pw })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    db.batch_execute(
        "UPDATE users SET role = 'admin' WHERE username = 'alice'; UPDATE users SET role = 'resolver' WHERE username = 'rex';",
    )
    .await
    .unwrap();
    let mut tokens = Vec::new();
    for name in names {
        tokens.push(login(&w.app, name, pw).await);
    }
    let [alice, rex, ann, ben, cal, bob, dave] = tokens.try_into().unwrap();
    let alice = common::with_totp(&w.app, &alice, "alice", pw).await;

    for m in MARKETS {
        let (status, body) = w
            .send(
                Method::POST,
                "/v1/admin/markets",
                &alice,
                json!({ "rulebook": cpi(m), "reason": "forecast test" }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    assert_eq!(
        w.forecast(&ann, 21, 80).await,
        StatusCode::CONFLICT,
        "not open yet"
    );

    // Minute 0: the scheduler opens the markets, then samples an empty book (50).
    w.at(t0).await;
    w.clock.set(t0 + 5);
    for m in MARKETS {
        assert_eq!(w.forecast(&cal, m, 60).await, StatusCode::OK);
    }
    w.clock.set(t0 + 10);
    for m in MARKETS {
        assert_eq!(w.forecast(&ann, m, 80).await, StatusCode::OK);
    }
    assert_eq!(
        w.forecast(&rex, 21, 50).await,
        StatusCode::FORBIDDEN,
        "a resolver does not forecast"
    );
    for p in [0, 100] {
        assert_eq!(w.forecast(&ann, 21, p).await, StatusCode::BAD_REQUEST);
    }
    for m in MARKETS {
        for (token, side, price) in [(&alice, "sell", 70), (&ben, "buy", 30)] {
            let (status, body) = w
                .send(
                    Method::POST,
                    "/v1/orders",
                    token,
                    json!({ "market": m, "side": side, "price": price, "qty": 5 }),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }
    // Minute 1: no trade yet, so the midpoint of 30 and 70.
    w.at(t0 + 60).await;
    w.clock.set(t0 + 90);
    for m in MARKETS {
        assert_eq!(w.forecast(&cal, m, 95).await, StatusCode::OK);
        let (_, body) = w
            .send(
                Method::POST,
                "/v1/orders",
                &bob,
                json!({ "market": m, "side": "buy", "price": 70, "qty": 5 }),
            )
            .await;
        assert_eq!(body["filled"], json!(5), "{body}");
    }
    w.projected().await;
    // Minutes 2 and 3: the last trade, 70. Ben forecasts in between; Dave after the last sample.
    w.at(t0 + 120).await;
    w.clock.set(t0 + 125);
    for m in MARKETS {
        assert_eq!(w.forecast(&ben, m, 30).await, StatusCode::OK);
    }
    w.at(t0 + 180).await;
    w.clock.set(t0 + 210);
    assert_eq!(w.forecast(&dave, 21, 50).await, StatusCode::OK);
    let prices: Vec<i16> = db
        .query(
            "SELECT price FROM score_samples WHERE market = 21 ORDER BY at",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(prices, [50, 50, 70, 70]);
    let (_, history) = call(&w.app, Method::GET, "/v1/markets/21/prices", None, None).await;
    assert_eq!(
        history["points"],
        json!([
            ["2026-09-01T13:30:00Z", 50],
            ["2026-09-01T13:31:00Z", 50],
            ["2026-09-01T13:32:00Z", 70],
            ["2026-09-01T13:33:00Z", 70]
        ])
    );

    // Forecasts freeze at close, and the table refuses edits.
    let close = instant("2026-10-15T12:00:00Z").unwrap();
    w.at(close).await;
    assert_eq!(
        w.forecast(&ann, 21, 90).await,
        StatusCode::CONFLICT,
        "frozen at close"
    );
    assert!(
        db.execute("UPDATE forecasts SET probability = 99", &[])
            .await
            .is_err()
    );
    assert!(db.execute("DELETE FROM forecasts", &[]).await.is_err());
    let (_, mine) = call(&w.app, Method::GET, "/v1/forecasts", Some(&cal), None).await;
    assert_eq!(
        mine["forecasts"].as_array().unwrap().len(),
        20,
        "both of Cal's forecasts on each market are kept"
    );

    // Settle every market YES (directly: this test is about scoring, not the workflow).
    for m in MARKETS {
        w.state
            .core
            .submit(Command::Settle {
                market: MarketId::new(m),
                outcome: Outcome::Yes,
                evidence_hash: None,
            })
            .await
            .unwrap();
    }
    w.projected().await;
    w.at(close + 60).await;
    let scored: i64 = db
        .query_one("SELECT count(*) FROM scored_markets", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(scored, 10);

    // By hand, per market (outcome YES, so Brier = (p - 100)^2):
    //   market: 2500, 2500, 900, 900                      -> average 1700
    //   Ann (80 from minute 1): 0, 2100, 500, 500         -> skill 3100/4 = 775, Brier 400
    //   Cal (60 from minute 1, 95 from minute 2): 0, 900, 875, 875
    //                                                     -> skill 2650/4 = 662.5 -> 663, Brier 550
    //   Ben (30 from minute 3): 0, 0, 0, 900 - 4900       -> skill -4000/4 = -1000, Brier 4900
    //   Dave forecast after the last sample: not scored.
    for (token, skill, brier) in [(&ann, 775, 400), (&cal, 663, 550), (&ben, -1000, 4900)] {
        let (_, card) = call(&w.app, Method::GET, "/v1/scorecard", Some(token), None).await;
        let first = &card["markets"][0];
        assert_eq!(
            (
                first["skill"].as_i64(),
                first["brier"].as_i64(),
                first["market_brier"].as_i64()
            ),
            (Some(skill), Some(brier), Some(1700)),
            "{card}"
        );
        assert_eq!(card["markets"].as_array().unwrap().len(), 10);
        assert_eq!(card["average_skill"], json!(skill));
        assert_eq!(card["ranked"], json!(true));
    }
    let (_, card) = call(&w.app, Method::GET, "/v1/scorecard", Some(&dave), None).await;
    assert_eq!(card["markets"], json!([]));

    let (_, skill) = call(&w.app, Method::GET, "/v1/leaderboards/skill", None, None).await;
    let order: Vec<&str> = skill["leaders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["username"].as_str().unwrap())
        .collect();
    assert_eq!(order, ["ann", "cal", "ben"]);
    // Bob bought 5 YES at 70 in each market from Alice: +150 each for Bob, -150 for Alice.
    let (_, profit) = call(&w.app, Method::GET, "/v1/leaderboards/profit", None, None).await;
    let rows: Vec<(String, i64)> = profit["leaders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            (
                l["username"].as_str().unwrap().to_owned(),
                l["profit"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [("bob".to_owned(), 1_500), ("alice".to_owned(), -1_500)]
    );

    // Scoring is done once.
    w.at(close + 120).await;
    let rows: i64 = db
        .query_one("SELECT count(*) FROM scores", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(rows, 30);
}
