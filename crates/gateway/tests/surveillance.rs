//! Login signals, the surveillance role, and cases end to end (docs/surveillance.md).
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::panic
)] // test code

mod common;

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use brier_core::{CoreConfig, start};
use brier_gateway::{AppState, CLIENT_ADDRESS_HEADER, PROXY_KEY_HEADER, router};
use common::{call, connect, send};
use serde_json::{Value, json};

const PROXY_KEY: &str = "a proxy key of at least thirty-two characters";
const PW: &str = "a long enough password";

/// Log in as the web server would: with the proxy key (or a wrong one) and a client address.
async fn login_via(app: &Router, username: &str, key: Option<&str>, address: &str) -> String {
    let mut req = Request::builder()
        .method(Method::POST)
        .uri("/v1/login")
        .header("content-type", "application/json")
        .header(CLIENT_ADDRESS_HEADER, address);
    if let Some(k) = key {
        req = req.header(PROXY_KEY_HEADER, k);
    }
    let body = json!({ "username": username, "password": PW }).to_string();
    let (status, body) = send(app, req.body(Body::from(body)).unwrap()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn logins_link_accounts_and_cases_are_opened_and_closed() {
    let schema = common::schema("gateway_surveillance");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let mut base = AppState::new(core, connect(&schema).await).unwrap();
    base.auth_limits = brier_gateway::limits::Limiter::per_minute(1_000, 1_000);
    let app = router(Arc::new(
        base.with_totp_key(common::TOTP_KEY)
            .with_proxy_key(PROXY_KEY),
    ));
    let db = connect(&schema).await;
    for name in ["alice", "bob", "carol", "dave", "erin", "watch"] {
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
    db.batch_execute(
        "UPDATE users SET role = 'admin' WHERE username = 'erin'; \
         UPDATE users SET role = 'surveillance' WHERE username = 'watch';",
    )
    .await
    .unwrap();
    let id = |name: &'static str| {
        let db = &db;
        async move {
            db.query_one("SELECT id FROM users WHERE username = $1", &[&name])
                .await
                .unwrap()
                .get::<_, i64>(0)
        }
    };

    // ---- Login signals ---------------------------------------------------------------------
    let alice = login_via(&app, "alice", Some(PROXY_KEY), "203.0.113.5").await;
    login_via(&app, "bob", Some(PROXY_KEY), "::ffff:203.0.113.5").await; // the same address
    login_via(&app, "carol", Some(PROXY_KEY), "198.51.100.7").await;
    // A wrong key: the forwarded address is ignored, and the test request has no address of
    // its own, so nothing is recorded, even though dave claims alice's address.
    login_via(&app, "dave", Some("not the key"), "203.0.113.5").await;
    let watch = login_via(&app, "watch", Some(PROXY_KEY), "192.0.2.1").await;
    let stored: Vec<(i64, Vec<u8>)> = db
        .query(
            "SELECT user_id, address_hash FROM login_signals ORDER BY user_id",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(stored.len(), 4, "alice, bob, carol, and watch");
    assert!(
        stored
            .iter()
            .all(|(_, h)| !h.windows(11).any(|w| w == b"203.0.113.5")),
        "the address itself is never stored"
    );
    let (status, links) = call(
        &app,
        Method::GET,
        "/v1/surveillance/links",
        Some(&watch),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{links}");
    assert_eq!(
        links["links"],
        json!([{ "accounts": [id("alice").await, id("bob").await], "shared_addresses": 1 }])
    );
    let (status, _) = call(
        &app,
        Method::GET,
        "/v1/surveillance/links",
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a trader cannot read links");

    // ---- The surveillance role can do nothing else -------------------------------------------
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/orders",
        Some(&watch),
        Some(json!({ "market": 1, "side": "buy", "price": 50, "qty": 1 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::PUT,
        "/v1/markets/1/forecast",
        Some(&watch),
        Some(json!({ "probability": 50 })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // ---- Opening cases -----------------------------------------------------------------------
    let alert = json!({
        "typology": "wash", "market": 7, "account": id("alice").await, "score": 0.61,
        "detector": "wash v1", "evidence": { "linked_contracts": 120, "timeline": [[12, "fill", "buy", 50, 40]] },
    });
    let (status, opened) = call(
        &app,
        Method::POST,
        "/v1/surveillance/cases",
        Some(&watch),
        Some(alert.clone()),
    )
    .await;
    assert_eq!(
        (status, &opened["opened"]),
        (StatusCode::CREATED, &json!(true)),
        "{opened}"
    );
    let case = opened["case"].as_i64().unwrap();
    let (status, again) = call(
        &app,
        Method::POST,
        "/v1/surveillance/cases",
        Some(&watch),
        Some(alert.clone()),
    )
    .await;
    assert_eq!(
        (status, &again["case"], &again["opened"]),
        (StatusCode::OK, &json!(case), &json!(false))
    );
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/surveillance/cases",
        Some(&alice),
        Some(alert.clone()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only the service opens cases"
    );
    for (field, bad) in [
        ("typology", json!("front_running")),
        ("score", json!(0)),
        ("evidence", json!([1])),
    ] {
        let mut b = alert.clone();
        b[field] = bad;
        let (status, body) = call(
            &app,
            Method::POST,
            "/v1/surveillance/cases",
            Some(&watch),
            Some(b),
        )
        .await;
        assert_eq!(
            (status, code(&body)),
            (StatusCode::BAD_REQUEST, "bad_request"),
            "{field}"
        );
    }

    // ---- Working them --------------------------------------------------------------------------
    let erin = common::login(&app, "erin", PW).await;
    let (status, body) = call(&app, Method::GET, "/v1/admin/cases", Some(&erin), None).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "admin needs a TOTP sign-in: {body}"
    );
    let erin = common::with_totp(&app, &erin, "erin", PW).await;
    let (status, list) = call(&app, Method::GET, "/v1/admin/cases", Some(&erin), None).await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list["cases"].as_array().unwrap().len(), 1);
    assert_eq!(list["cases"][0]["username"], "alice");
    let (status, detail) = call(
        &app,
        Method::GET,
        &format!("/v1/admin/cases/{case}"),
        Some(&erin),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{detail}");
    assert_eq!(detail["evidence"]["linked_contracts"], 120);
    assert_eq!(
        detail["linked_accounts"],
        json!([{ "account": id("bob").await, "username": "bob" }])
    );

    let path = format!("/v1/admin/cases/{case}/close");
    let (status, body) = call(
        &app,
        Method::POST,
        &path,
        Some(&erin),
        Some(json!({ "outcome": "ignored", "reason": "nothing" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, closed) = call(
        &app,
        Method::POST,
        &path,
        Some(&erin),
        Some(json!({ "outcome": "dismissed", "reason": "a shared household, trades at market prices" })),
    )
    .await;
    assert_eq!(
        (status, &closed["status"]),
        (StatusCode::OK, &json!("dismissed")),
        "{closed}"
    );
    let (status, body) = call(
        &app,
        Method::POST,
        &path,
        Some(&erin),
        Some(json!({ "outcome": "actioned", "reason": "second try" })),
    )
    .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "conflict"));
    let (_, open) = call(&app, Method::GET, "/v1/admin/cases", Some(&erin), None).await;
    assert_eq!(open["cases"], json!([]));
    let (_, all) = call(
        &app,
        Method::GET,
        "/v1/admin/cases?status=all",
        Some(&erin),
        None,
    )
    .await;
    assert_eq!(all["cases"][0]["closed_by"], "erin");
    let (_, audit) = call(&app, Method::GET, "/v1/admin/audit", Some(&erin), None).await;
    let actions: Vec<&str> = audit["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(
        actions.contains(&"open_case") && actions.contains(&"close_case"),
        "{actions:?}"
    );
    let (status, _) = call(&app, Method::GET, "/v1/admin/cases/999", Some(&erin), None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
