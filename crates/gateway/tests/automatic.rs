//! Automatic review (G9): a re-fetch that agrees confirms a proposal on a market whose
//! rulebook allows it, with a short dispute window for a short market; a re-fetch that
//! disagrees sends it to an admin for good.
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

use axum::http::{Method, StatusCode};
use brier_core::{CoreConfig, start};
use brier_engine::MarketState;
use brier_gateway::clock::Clock;
use brier_gateway::scheduler::tick;
use brier_gateway::{AppState, router};
use brier_rules::time::{format_instant, instant};
use brier_types::MarketId;
use common::{call, connect, login};
use serde_json::{Value, json};

const PW: &str = "a long enough password";
const NOAA_URL: &str = "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries&stations=USW00094728&dataTypes=TMAX";
const RELEASE: &str = "2026-09-01T00:00:00Z";

fn t(s: &str) -> i64 {
    instant(s).unwrap()
}

fn rulebook(market: u32, op: &str, threshold: &str, review: &str) -> Value {
    json!({
        "schema": 2, "market": market,
        "title": format!("New York high {op} {threshold} on 30 August"),
        "question": format!("Will the maximum temperature at Central Park on 30 August 2026 be {op} {threshold} °F?"),
        "source": { "provider": "noaa", "series": "USW00094728/TMAX", "url": NOAA_URL },
        "observation": "2026-08-30",
        "condition": { "transform": "level", "op": op, "threshold": threshold },
        "schedule": {
            "open": "2026-08-25T00:00:00Z", "close": "2026-08-30T00:00:00Z",
            "release": RELEASE, "deadline": "2026-10-01T00:00:00Z",
        },
        "revision": "first_release", "void": ["no_value_by_deadline"],
        "event": {
            "slug": format!("nyc-high-{market}"), "title": "Highest temperature in New York on 30 August?",
            "tags": [], "exclusive": false, "outcome": format!("{op} {threshold}"),
        },
        "review": review,
    })
}

fn evidence(market: u32, rules_hash: &str, outcome: &str, retrieved_at: i64) -> Value {
    json!({
        "market": market, "rules_hash": rules_hash, "outcome": outcome,
        "retrieved_at": format_instant(retrieved_at).unwrap(),
        "source": "noaa", "series": "USW00094728/TMAX", "value": "84",
    })
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn a_matching_refetch_confirms_and_a_mismatch_needs_an_admin() {
    let schema = common::schema("gateway_automatic");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let clock = Clock::manual(t("2026-08-20T00:00:00Z"));
    let mut base = AppState::new(core.clone(), connect(&schema).await).unwrap();
    base.auth_limits = brier_gateway::limits::Limiter::per_minute(1_000, 1_000);
    let state = Arc::new(
        base.with_totp_key(common::TOTP_KEY)
            .with_clock(clock.clone()),
    );
    let app = router(Arc::clone(&state));
    let db = connect(&schema).await;
    for name in ["ada", "rex"] {
        let (status, body) = call(
            &app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": PW })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    db.batch_execute("UPDATE users SET role = 'admin' WHERE username = 'ada'; UPDATE users SET role = 'resolver' WHERE username = 'rex';")
        .await
        .unwrap();
    let ada = login(&app, "ada", PW).await;
    let ada = common::with_totp(&app, &ada, "ada", PW).await;
    let rex = login(&app, "rex", PW).await;
    let post = |path: String, token: String, body: Value| {
        let app = app.clone();
        async move { call(&app, Method::POST, &path, Some(&token), Some(body)).await }
    };
    let mut hashes = Vec::new();
    for (m, op, th, review) in [
        (401, ">=", "80", "automatic"),
        (402, "<", "70", "automatic"),
        (403, ">=", "90", "manual"),
    ] {
        let rb = rulebook(m, op, th, review);
        hashes.push(brier_rules::validate(&rb).unwrap().hash_hex());
        let (status, body) = post(
            "/v1/admin/markets".into(),
            ada.clone(),
            json!({ "rulebook": rb, "reason": "list" }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    for at in ["2026-08-25T00:00:00Z", "2026-08-30T00:00:00Z"] {
        clock.set(t(at));
        tick(&state).await.unwrap();
    }
    let released = t(RELEASE) + 60;
    clock.set(released + 1_000);
    let mut ids = Vec::new();
    for (i, (m, outcome)) in [(401, "yes"), (402, "no"), (403, "no")]
        .into_iter()
        .enumerate()
    {
        let (status, body) = post(
            format!("/v1/markets/{m}/resolution"),
            rex.clone(),
            json!({ "outcome": outcome, "evidence": evidence(m, &hashes[i], outcome, released), "reason": "NOAA published 84 °F" }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        ids.push(body["resolution"].as_i64().unwrap());
    }
    let verify = |id: i64, m: u32, hash: &str, outcome: &str, at: i64| {
        post(
            format!("/v1/resolutions/{id}/verify"),
            rex.clone(),
            json!({ "evidence": evidence(m, hash, outcome, at) }),
        )
    };

    // Too soon after the proposal's fetch: not independent enough.
    let (status, body) = verify(ids[0], 401, &hashes[0], "yes", released + 300).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "bad_request"),
        "{body}"
    );
    // A later re-fetch that agrees confirms, with the one-hour window of a five-day market.
    let (status, body) = verify(ids[0], 401, &hashes[0], "yes", released + 700).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["dispute_until"],
        json!(format_instant(released + 1_000 + 3_600).unwrap())
    );
    // It settles once the window passes, with no admin involved.
    clock.set(released + 1_000 + 3_601);
    tick(&state).await.unwrap();
    assert_eq!(
        brier_gateway::markets::market_state(&state, MarketId::new(401))
            .await
            .unwrap(),
        Some(MarketState::Resolved)
    );

    // A re-fetch that disagrees is kept, and the proposal now needs an admin.
    let (status, body) = verify(ids[1], 402, &hashes[1], "yes", released + 700).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "conflict"),
        "{body}"
    );
    let (status, body) = verify(ids[1], 402, &hashes[1], "no", released + 800).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "no second chance after a mismatch: {body}"
    );
    let (status, body) = post(
        format!("/v1/admin/resolutions/{}/confirm", ids[1]),
        ada.clone(),
        json!({ "reason": "checked the station record" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "an admin can still confirm: {body}");

    // A manual-review market cannot be verified automatically.
    let (status, body) = verify(ids[2], 403, &hashes[2], "no", released + 700).await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "conflict"),
        "{body}"
    );

    let mismatch: Option<Vec<u8>> = db
        .query_one(
            "SELECT verify_mismatch FROM resolutions WHERE id = $1",
            &[&ids[1]],
        )
        .await
        .unwrap()
        .get(0);
    assert!(mismatch.is_some());
    let (_, audit) = call(&app, Method::GET, "/v1/admin/audit", Some(&ada), None).await;
    let verifications = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["action"] == "verify_resolution")
        .count();
    assert_eq!(
        verifications, 2,
        "one agreement and one disagreement are audited"
    );
}
