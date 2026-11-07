//! Helpers shared by the gateway's integration tests.
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)] // test code; each test uses a subset

use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use serde_json::{Value, json};
use tokio_postgres::{Client, NoTls};
use tower::ServiceExt;

/// A fresh schema name for one test run.
pub fn schema(prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

pub async fn connect(schema: &str) -> Client {
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
    brier_readmodel::migrate(&mut client).await.unwrap();
    client
}

pub async fn send(app: &Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    // Non-JSON bodies (only axum's own WebSocket-upgrade rejection) come back as a string.
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

pub async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    send(app, req.body(body).unwrap()).await
}

pub fn now_ms() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

pub fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub async fn login(app: &Router, username: &str, password: &str) -> String {
    let (status, body) = call(
        app,
        Method::POST,
        "/v1/login",
        None,
        Some(json!({ "username": username, "password": password })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}

/// A rulebook for `market` that is open from 2026 until 2099, for tests that only need a
/// tradable market (`tests/fixtures/long_window_rulebook.json`).
pub fn rulebook(market: u32) -> Value {
    let mut r: Value =
        serde_json::from_str(include_str!("../fixtures/long_window_rulebook.json")).unwrap();
    r["market"] = json!(market);
    r
}

/// The TOTP key every test exchange is built with.
pub const TOTP_KEY: [u8; 32] = [0x42; 32];

fn base32_decode(s: &str) -> Vec<u8> {
    let (mut out, mut buffer, mut bits) = (Vec::new(), 0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'2'..=b'7' => c - b'2' + 26,
            _ => panic!("not base32: {s}"),
        };
        buffer = (buffer << 5) | u32::from(v);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((buffer >> bits) & 0xff).unwrap());
        }
    }
    out
}

/// Turn TOTP on for the user signed in with `token`, then sign in again with a code; returns
/// the new session token, which counts as signed in with TOTP (needed for admin actions).
pub async fn with_totp(app: &Router, token: &str, username: &str, password: &str) -> String {
    let (status, setup) = call(app, Method::POST, "/v1/totp/setup", Some(token), None).await;
    assert_eq!(status, StatusCode::OK, "{setup}");
    let secret = base32_decode(setup["secret"].as_str().unwrap());
    let step = brier_gateway::totp::step(now_ms() / 1_000);
    let code = brier_gateway::totp::hotp(&secret, step);
    let (status, body) = call(
        app,
        Method::POST,
        "/v1/totp/enable",
        Some(token),
        Some(json!({ "code": code })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The next step's code: each step is accepted once, and one step ahead is allowed.
    let next = brier_gateway::totp::hotp(&secret, step + 1);
    let (status, body) = call(
        app,
        Method::POST,
        "/v1/login",
        None,
        Some(json!({ "username": username, "password": password, "totp": next })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    body["token"].as_str().unwrap().to_owned()
}
