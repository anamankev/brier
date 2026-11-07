//! The REST API end to end: a real core and journal in a temporary directory, real
//! PostgreSQL (one schema per run), requests through the router.
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored` (also run by `make test-all` and the CI integration job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    clippy::indexing_slicing
)] // test code

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use brier_core::{CoreConfig, start};
use brier_gateway::auth::signing_payload;
use brier_gateway::{AppState, router, run_projector};
use common::{base64_encode, call, connect, login, now_ms, send};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

fn signed(
    key: &SigningKey,
    key_id: i64,
    method: Method,
    path: &str,
    body: &str,
    ts: u64,
) -> Request<Body> {
    let signature = key.sign(&signing_payload(ts, &method, path, body.as_bytes()));
    Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("brier-key", key_id.to_string())
        .header("brier-timestamp", ts.to_string())
        .header("brier-signature", base64_encode(&signature.to_bytes()))
        .body(Body::from(body.to_owned()))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn the_rest_api_end_to_end() {
    let schema = common::schema("gateway_rest");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let state = Arc::new(AppState::new(core.clone(), connect(&schema).await).unwrap());
    let admin_db = connect(&schema).await;
    tokio::spawn(run_projector(
        connect(&schema).await,
        data.path().to_owned(),
        core.durable(),
        Arc::clone(&state.projected),
    ));
    let app = router(Arc::clone(&state));
    let pw = "a long enough password";

    // Registration and login.
    for name in ["alice", "bob"] {
        let (status, body) = call(
            &app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": pw })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let (status, body) = call(
        &app,
        Method::POST,
        "/v1/register",
        None,
        Some(json!({ "username": "alice", "password": pw })),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::CONFLICT, Some("username_taken"))
    );
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/register",
        None,
        Some(json!({ "username": "Eve!", "password": pw })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/register",
        None,
        Some(json!({ "username": "eve", "password": "short" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/login",
        None,
        Some(json!({ "username": "alice", "password": "wrong password!!" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let alice = login(&app, "alice", pw).await;
    let bob = login(&app, "bob", pw).await;

    let (status, _) = call(&app, Method::GET, "/v1/portfolio", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(&app, Method::GET, "/v1/portfolio", Some(&bob), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["cash"], json!(brier_engine::SIGNUP_GRANT.cents()));

    // Administration: role, reason, audit.
    admin_db
        .execute(
            "UPDATE users SET role = 'admin' WHERE username = 'alice'",
            &[],
        )
        .await
        .unwrap();
    let create = json!({ "market": 1, "reason": "list the first test market" });
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/admin/markets",
        Some(&bob),
        Some(create.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/admin/markets",
        Some(&alice),
        Some(json!({ "market": 1, "reason": " x " })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a reason is required");
    let (status, body) = call(
        &app,
        Method::POST,
        "/v1/admin/markets",
        Some(&alice),
        Some(create),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/admin/markets/1/state",
        Some(&alice),
        Some(json!({ "state": "open", "reason": "open for trading" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &app,
        Method::POST,
        "/v1/admin/markets/1/settle",
        Some(&alice),
        Some(json!({ "outcome": "yes", "reason": "too early" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "an open market cannot settle: {body}"
    );
    assert!(
        body["error"]["seq"].is_u64(),
        "the rejection was sequenced: {body}"
    );

    // Orders: rest, idempotent retry, take, amend, cancel.
    let ask = json!({ "market": 1, "side": "sell", "price": 60, "qty": 10, "client_order_id": 7 });
    let (status, rested) = call(
        &app,
        Method::POST,
        "/v1/orders",
        Some(&bob),
        Some(ask.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{rested}");
    assert_eq!(
        (rested["status"].as_str(), rested["remaining"].as_u64()),
        (Some("resting"), Some(10))
    );
    let order = rested["order"].as_u64().unwrap();
    let (status, retry) = call(&app, Method::POST, "/v1/orders", Some(&bob), Some(ask)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (retry["status"].as_str(), retry["order"].as_u64()),
        (Some("duplicate"), Some(order))
    );
    let bid = json!({ "market": 1, "side": "buy", "price": 60, "qty": 4, "tif": "ioc" });
    let (status, taken) = call(&app, Method::POST, "/v1/orders", Some(&alice), Some(bid)).await;
    assert_eq!(status, StatusCode::OK, "{taken}");
    assert_eq!(taken["status"], "filled");
    assert_eq!(taken["fills"], json!([{ "price": 60, "qty": 4 }]));
    let (_, book) = call(&app, Method::GET, "/v1/markets/1/book", None, None).await;
    assert_eq!(
        (book["asks"].clone(), book["bids"].clone()),
        (json!([[60, 6]]), json!([]))
    );
    let (status, _) = call(&app, Method::GET, "/v1/markets/9/book", None, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let path = format!("/v1/orders/{order}");
    let (status, amended) = call(
        &app,
        Method::PATCH,
        &path,
        Some(&bob),
        Some(json!({ "qty": 3 })),
    )
    .await;
    assert_eq!(
        (status, amended["remaining"].as_u64()),
        (StatusCode::OK, Some(3))
    );
    let (status, _) = call(&app, Method::DELETE, &path, Some(&alice), None).await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "another account cannot cancel it"
    );
    let (status, cancelled) = call(&app, Method::DELETE, &path, Some(&bob), None).await;
    assert_eq!(
        (status, cancelled["cancelled"].as_u64()),
        (StatusCode::OK, Some(3))
    );
    let (status, again) = call(&app, Method::DELETE, &path, Some(&bob), None).await;
    assert_eq!(
        (status, again["error"]["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("unknown_order"))
    );

    // Signed requests: valid, replayed, stale, tampered, and session-only endpoints.
    let key = SigningKey::from_bytes(&[7; 32]);
    let (status, added) = call(
        &app,
        Method::POST,
        "/v1/keys",
        Some(&alice),
        Some(json!({ "public_key": base64_encode(key.verifying_key().as_bytes()) })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{added}");
    let key_id = added["key_id"].as_i64().unwrap();
    let ts = now_ms();
    let (status, portfolio) = send(
        &app,
        signed(&key, key_id, Method::GET, "/v1/portfolio", "", ts),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{portfolio}");
    assert_eq!(
        portfolio["positions"],
        json!([{ "market": 1, "position": 4 }])
    );
    let (status, _) = send(
        &app,
        signed(&key, key_id, Method::GET, "/v1/portfolio", "", ts),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a replayed signature is refused"
    );
    let (status, _) = send(
        &app,
        signed(
            &key,
            key_id,
            Method::GET,
            "/v1/portfolio",
            "",
            now_ms() - 6_000,
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a stale timestamp is refused"
    );
    let mut tampered = signed(
        &key,
        key_id,
        Method::POST,
        "/v1/orders",
        r#"{"market":1,"side":"buy","price":1,"qty":1}"#,
        now_ms(),
    );
    *tampered.body_mut() = Body::from(r#"{"market":1,"side":"buy","price":99,"qty":1}"#);
    let (status, _) = send(&app, tampered).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a changed body does not verify"
    );
    let (status, _) = send(
        &app,
        signed(&key, key_id, Method::GET, "/v1/keys", "", now_ms()),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "keys are managed with a session only"
    );
    let (status, _) = call(
        &app,
        Method::DELETE,
        &format!("/v1/keys/{key_id}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(
        &app,
        signed(&key, key_id, Method::GET, "/v1/portfolio", "", now_ms()),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a revoked key is refused");

    // History from the read models, once they catch up.
    let durable = *core.durable().borrow();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while state.projected.load(Ordering::Acquire) < durable {
        assert!(
            std::time::Instant::now() < deadline,
            "the projector did not catch up"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let (_, fills) = call(&app, Method::GET, "/v1/fills", Some(&alice), None).await;
    assert_eq!(fills["fills"].as_array().unwrap().len(), 1, "{fills}");
    assert_eq!(
        (
            fills["fills"][0]["side"].as_str(),
            fills["fills"][0]["liquidity"].as_str()
        ),
        (Some("buy"), Some("taker"))
    );
    let (_, fills) = call(&app, Method::GET, "/v1/fills", Some(&bob), None).await;
    assert_eq!(
        (
            fills["fills"][0]["side"].as_str(),
            fills["fills"][0]["liquidity"].as_str()
        ),
        (Some("sell"), Some("maker"))
    );
    let (mut seen, mut query) = (Vec::new(), "/v1/orders/history?limit=2".to_owned());
    loop {
        let (status, page) = call(&app, Method::GET, &query, Some(&bob), None).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        seen.extend(
            page["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["kind"].as_str().unwrap().to_owned()),
        );
        let Some(next) = page["next"].as_object() else {
            break;
        };
        query = format!(
            "/v1/orders/history?limit=2&after_seq={}&after_ordinal={}",
            next["after_seq"], next["after_ordinal"]
        );
    }
    assert_eq!(
        seen,
        ["accepted", "rested", "duplicate", "amended", "cancelled",],
        "every page, in order (a rejected cancel names no market, so it is not an order event)"
    );
    let (status, one) = call(&app, Method::GET, &path, Some(&bob), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["events"].as_array().unwrap().len(), 5, "{one}");
    let (status, _) = call(&app, Method::GET, &path, Some(&alice), None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another account's order is not visible"
    );

    let audited: i64 = admin_db
        .query_one("SELECT count(*) FROM audit_log WHERE seq IS NOT NULL", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        audited, 3,
        "create, open, and the rejected settle are audited with their seq"
    );

    // Limits and errors.
    let big = json!({ "username": "x".repeat(10_000), "password": pw });
    let (status, body) = call(&app, Method::POST, "/v1/login", None, Some(big)).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("too_large"))
    );
    let (status, body) = call(&app, Method::GET, "/v1/nothing", None, None).await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("not_found"))
    );

    // Logout ends the session.
    let (status, _) = call(&app, Method::POST, "/v1/logout", Some(&alice), None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(&app, Method::GET, "/v1/portfolio", Some(&alice), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    admin_db
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}
