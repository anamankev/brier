//! Events (G9): schema 2 listings grouped into events, the checks on an event's markets, and
//! the events API with 24-hour statistics from the minute samples.
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)] // test code

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use brier_core::{CoreConfig, start};
use brier_gateway::clock::Clock;
use brier_gateway::scheduler::tick;
use brier_gateway::{AppState, router, run_projector};
use brier_rules::time::instant;
use common::{call, connect, login};
use serde_json::{Value, json};

const PW: &str = "a long enough password";
const NOAA_URL: &str = "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries&stations=USW00094728&dataTypes=TMAX";

fn t(s: &str) -> i64 {
    instant(s).unwrap()
}

/// One outcome of "Highest temperature in New York on {day}?".
fn bracket(
    market: u32,
    day: &str,
    op: &str,
    threshold: &str,
    upper: Option<&str>,
    outcome: &str,
) -> Value {
    let mut condition = json!({ "transform": "level", "op": op, "threshold": threshold });
    if let Some(u) = upper {
        condition["upper"] = json!(u);
    }
    json!({
        "schema": 2, "market": market,
        "title": format!("New York high {outcome} on {day}"),
        "question": format!("Will the maximum temperature at Central Park on {day} be {outcome}?"),
        "source": { "provider": "noaa", "series": "USW00094728/TMAX", "url": NOAA_URL },
        "observation": day, "condition": condition,
        "schedule": {
            "open": "2026-08-21T00:00:00Z", "close": format!("{day}T00:00:00Z"),
            "release": "2026-09-05T00:00:00Z", "deadline": "2026-10-01T00:00:00Z",
        },
        "revision": "first_release", "void": ["no_value_by_deadline"],
        "event": {
            "slug": format!("nyc-high-{day}"), "title": format!("Highest temperature in New York on {day}?"),
            "tags": ["daily", "new-york"], "series": "nyc-daily-high", "exclusive": true, "outcome": outcome,
        },
        "review": "automatic",
    })
}

fn code(body: &Value) -> String {
    body["error"]["message"]
        .as_str()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_owned()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn markets_group_into_events_with_daily_statistics() {
    let schema = common::schema("gateway_events");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let clock = Clock::manual(t("2026-08-20T00:00:00Z"));
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
    let app = router(Arc::clone(&state));
    let db = connect(&schema).await;
    for name in ["ada", "bob", "cyd", "lst"] {
        let (status, body) = call(
            &app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": PW, "adult": true, "terms": "2026-11-12" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    db.batch_execute("UPDATE users SET role = 'admin' WHERE username = 'ada'; UPDATE users SET role = 'lister' WHERE username = 'lst';")
        .await
        .unwrap();
    let ada = login(&app, "ada", PW).await;
    let ada = common::with_totp(&app, &ada, "ada", PW).await;
    let (bo, cy) = (login(&app, "bob", PW).await, login(&app, "cyd", PW).await);
    let list = |rulebook: Value| {
        let app = app.clone();
        let ada = ada.clone();
        async move {
            call(
                &app,
                Method::POST,
                "/v1/admin/markets",
                Some(&ada),
                Some(json!({ "rulebook": rulebook, "reason": "test listing" })),
            )
            .await
        }
    };

    // ---- Listing an event -------------------------------------------------------------------
    let day = "2026-08-30";
    for (m, op, lo, hi, label) in [
        (201, "<=", "79", None, "79°F or below"),
        (202, "between", "80", Some("81"), "80-81°F"),
        (203, ">=", "82", None, "82°F or above"),
    ] {
        let (status, body) = list(bracket(m, day, op, lo, hi, label)).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = list(bracket(204, day, "between", "81", Some("82"), "81-82°F")).await;
    assert_eq!(
        (status, code(&body).as_str()),
        (StatusCode::BAD_REQUEST, "event_overlap"),
        "{body}"
    );
    let mut other_title = bracket(205, day, "<=", "70", None, "70°F or below");
    other_title["event"]["title"] = json!("Something else?");
    let (status, body) = list(other_title).await;
    assert_eq!(
        (status, code(&body).as_str()),
        (StatusCode::BAD_REQUEST, "event_mismatch"),
        "{body}"
    );
    let mut same_label = bracket(206, day, "<=", "60", None, "79°F or below");
    same_label["event"]["exclusive"] = json!(true);
    let (status, body) = list(same_label).await;
    assert_eq!(
        (status, code(&body).as_str()),
        (StatusCode::BAD_REQUEST, "event_outcome_taken"),
        "{body}"
    );
    // The next day's event in the same series, and a market listed without an event.
    let (status, body) = list(bracket(
        211,
        "2026-08-31",
        ">=",
        "85",
        None,
        "85°F or above",
    ))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, body) = list(common::rulebook(301)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // ---- Trading, sampled a day apart ---------------------------------------------------------
    clock.set(t("2026-08-21T00:00:00Z"));
    tick(&state).await.unwrap(); // opens the markets
    let order = |token: &str, market: u32, side: &str, price: u8, qty: u32| {
        let (app, token) = (app.clone(), token.to_owned());
        let side = side.to_owned();
        async move {
            let (status, body) = call(
                &app,
                Method::POST,
                "/v1/orders",
                Some(&token),
                Some(json!({ "market": market, "side": side, "price": price, "qty": qty })),
            )
            .await;
            assert!(status.is_success(), "{body}");
        }
    };
    order(&bo, 202, "sell", 40, 10).await;
    order(&cy, 202, "buy", 40, 10).await;
    let projected = || async {
        let target = *state.core.durable().borrow();
        for _ in 0..500 {
            if state.projected.load(Ordering::Acquire) >= target {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the projector did not reach {target}");
    };
    projected().await;
    clock.set(t("2026-08-21T00:01:00Z"));
    tick(&state).await.unwrap(); // samples price 40, volume 10
    order(&bo, 202, "sell", 55, 25).await;
    order(&cy, 202, "buy", 55, 25).await;
    projected().await;
    clock.set(t("2026-08-22T00:02:00Z"));
    tick(&state).await.unwrap();

    // ---- The social layer (G10 U3) ------------------------------------------------------------
    let (status, body) = call(&app, Method::GET, "/v1/markets/202/holders", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["yes"], json!([{ "username": "cyd", "contracts": 35 }]));
    assert_eq!(body["no"], json!([{ "username": "bob", "contracts": 35 }]));
    let (status, body) = call(&app, Method::GET, "/v1/markets/202/activity", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let trades = body["trades"].as_array().unwrap();
    assert_eq!(trades.len(), 2, "{body}");
    assert_eq!(
        (
            &trades[0]["username"],
            &trades[0]["side"],
            &trades[0]["price"],
            &trades[0]["qty"]
        ),
        (&json!("cyd"), &json!("buy"), &json!(55), &json!(25)),
        "newest first, named by the taker"
    );
    assert!(trades[0]["at"].is_string());
    let (status, body) = call(&app, Method::GET, "/v1/users/cyd", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        (body["volume"].as_i64(), body["markets_traded"].as_i64()),
        (Some(35), Some(1))
    );
    assert_eq!(body["positions"][0]["market"], 202);
    assert_eq!(body["positions"][0]["position"], 35);
    assert_eq!(body["positions"][0]["outcome"], "80-81°F");
    assert_eq!(body["positions"][0]["event"], "nyc-high-2026-08-30");
    assert_eq!(body["trades"].as_array().unwrap().len(), 2);
    let (status, body) = call(&app, Method::GET, "/v1/users/bob", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["positions"][0]["position"], -35,
        "bob is short YES (holds NO)"
    );
    assert_eq!(body["trades"][0]["side"], "sell", "the maker's own side");
    let (status, _) = call(&app, Method::GET, "/v1/users/nobody", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, body) = call(&app, Method::GET, "/v1/notifications", Some(&bo), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["unread"], 2, "{body}");
    assert_eq!(body["items"][0]["kind"], "fill");
    let newest = body["items"][0]["seq"].as_i64().unwrap();
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/notifications/seen",
        Some(&bo),
        Some(json!({ "seq": newest })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, body) = call(&app, Method::GET, "/v1/notifications", Some(&bo), None).await;
    assert_eq!(body["unread"], 0);

    // ---- The events API -----------------------------------------------------------------------
    let (status, body) = call(&app, Method::GET, "/v1/events", None, None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let events = body["events"].as_array().unwrap();
    assert_eq!(
        events.len(),
        3,
        "two days of brackets and one standalone market: {body}"
    );
    let e = &events[0];
    assert_eq!(e["slug"], "nyc-high-2026-08-30", "the busiest first");
    assert_eq!(
        (
            e["category"].as_str(),
            e["exclusive"].as_bool(),
            e["status"].as_str()
        ),
        (Some("weather"), Some(true), Some("open"))
    );
    let outcomes: Vec<&str> = e["markets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(
        outcomes,
        ["79°F or below", "80-81°F", "82°F or above"],
        "in range order"
    );
    let m = &e["markets"][1];
    assert_eq!(
        (m["chance"].as_i64(), m["volume"].as_i64()),
        (Some(55), Some(35))
    );
    assert_eq!(
        (m["change_24h"].as_i64(), m["volume_24h"].as_i64()),
        (Some(15), Some(25)),
        "against the sample 24 hours earlier: {m}"
    );
    assert_eq!(body["categories"]["weather"], 2);
    let standalone = events.iter().find(|e| e["slug"] == "market-301").unwrap();
    assert_eq!(standalone["standalone"], true);

    let (_, found) = call(
        &app,
        Method::GET,
        "/v1/events?q=new%20york%2031",
        None,
        None,
    )
    .await;
    assert_eq!(found["events"].as_array().unwrap().len(), 1, "{found}");
    let (_, movers) = call(
        &app,
        Method::GET,
        "/v1/events?sort=movers&limit=1",
        None,
        None,
    )
    .await;
    assert_eq!(movers["events"][0]["slug"], "nyc-high-2026-08-30");
    let (_, series) = call(
        &app,
        Method::GET,
        "/v1/events?series=nyc-daily-high&sort=closing",
        None,
        None,
    )
    .await;
    let slugs: Vec<&str> = series["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, ["nyc-high-2026-08-30", "nyc-high-2026-08-31"]);
    let (status, _) = call(&app, Method::GET, "/v1/events?sort=random", None, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, one) = call(
        &app,
        Method::GET,
        "/v1/events/nyc-high-2026-08-31",
        None,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    let dates: Vec<&str> = one["siblings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["observation"].as_str().unwrap())
        .collect();
    assert_eq!(dates, ["2026-08-30", "2026-08-31"]);
    let (status, _) = call(&app, Method::GET, "/v1/events/no-such-event", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ---- The lister: recurring-series markets with automatic review, and nothing else -------
    let lister = login(&app, "lst", PW).await;
    let by_lister = |rulebook: Value| {
        let (app, lister) = (app.clone(), lister.clone());
        async move {
            call(
                &app,
                Method::POST,
                "/v1/admin/markets",
                Some(&lister),
                Some(json!({ "rulebook": rulebook, "reason": "series listing" })),
            )
            .await
        }
    };
    let (status, body) = by_lister(bracket(
        221,
        "2026-09-01",
        ">=",
        "85",
        None,
        "85°F or above",
    ))
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let mut manual = bracket(222, "2026-09-01", "<=", "84", None, "84°F or below");
    manual["review"] = json!("manual");
    let (status, _) = by_lister(manual).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "manual review needs an admin"
    );
    let mut one_off = bracket(223, "2026-09-02", ">=", "85", None, "85°F or above");
    one_off["event"].as_object_mut().unwrap().remove("series");
    let (status, _) = by_lister(one_off).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a one-off event needs an admin"
    );
    // A single game (schema 3) settles by an admin's confirmation, so the lister may list it.
    let game = json!({
        "schema": 3, "market": 100_000_011, "title": "Lakers at Celtics, Sep 5: Celtics win",
        "question": "Will the Boston Celtics beat the Los Angeles Lakers on September 5, 2026?",
        "source": { "provider": "balldontlie", "series": "NBA/MARGIN", "url": "https://www.balldontlie.io/",
                    "game": { "id": "1", "starts": "2026-09-05T00:30:00Z" } },
        "observation": "2026-09-05",
        "condition": { "transform": "level", "op": ">", "threshold": "0" },
        "schedule": { "open": "2026-09-02T00:30:00Z", "close": "2026-09-05T00:30:00Z",
                      "release": "2026-09-05T04:30:00Z", "deadline": "2026-09-12T04:30:00Z" },
        "revision": "first_release", "void": ["no_value_by_deadline"],
        "event": { "slug": "nba-lal-bos-2026-09-05", "title": "Lakers at Celtics, Sep 5",
                   "tags": ["basketball", "nba"], "series": "nba-games", "exclusive": true, "outcome": "Celtics" },
        "review": "manual",
    });
    let (status, body) = by_lister(game).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _) = by_lister(common::rulebook(302)).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a schema 1 market needs an admin"
    );
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/orders",
        Some(&lister),
        Some(json!({ "market": 202, "side": "buy", "price": 50, "qty": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the lister does not trade");
}
