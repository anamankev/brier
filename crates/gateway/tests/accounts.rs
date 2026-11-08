//! Public accounts end to end (G10 U1): sign-up confirms age and terms, recovery codes reset a
//! lost password, the export holds the user's data, and deletion removes the person.
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::indexing_slicing
)] // test code

mod common;

use std::sync::Arc;

use axum::http::{Method, StatusCode};
use brier_core::{CoreConfig, start};
use brier_gateway::account::{RECOVERY_CODES, TERMS_VERSION};
use brier_gateway::limits::Limiter;
use brier_gateway::{AppState, router};
use common::{call, connect, login};
use serde_json::json;

const PW: &str = "a long enough password";
const NEW_PW: &str = "another long password";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn public_accounts_end_to_end() {
    let schema = common::schema("gateway_accounts");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let mut state = AppState::new(core.clone(), connect(&schema).await)
        .unwrap()
        .with_totp_key(common::TOTP_KEY);
    // Many password calls from one test address; the limit itself is tested in rest.rs.
    state.auth_limits = Limiter::per_minute(1_000, 1_000);
    let state = Arc::new(state);
    let db = connect(&schema).await;
    let app = router(Arc::clone(&state));
    let register = |body| {
        let app = app.clone();
        async move { call(&app, Method::POST, "/v1/register", None, Some(body)).await }
    };

    // Sign-up needs both confirmations.
    let (status, body) = register(
        json!({ "username": "kim", "password": PW, "adult": false, "terms": TERMS_VERSION }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("adult_required"))
    );
    let (status, body) = register(
        json!({ "username": "kim", "password": PW, "adult": true, "terms": "2020-01-01" }),
    )
    .await;
    assert_eq!(
        (status, body["error"]["code"].as_str()),
        (StatusCode::BAD_REQUEST, Some("terms_required"))
    );
    let (status, body) = register(json!({ "username": "kim", "password": PW })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A good sign-up records both and hands out the codes once.
    let (status, body) = register(
        json!({ "username": "kim", "password": PW, "adult": true, "terms": TERMS_VERSION }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let codes: Vec<String> = body["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(codes.len(), RECOVERY_CODES);
    assert!(
        codes
            .iter()
            .all(|c| c.len() == 19 && c.matches('-').count() == 3),
        "{codes:?}"
    );
    let row = db
        .query_one("SELECT adult_confirmed_at IS NOT NULL, terms_version, (SELECT count(*) FROM recovery_codes WHERE user_id = users.id) FROM users WHERE username = 'kim'", &[])
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0));
    assert_eq!(row.get::<_, &str>(1), TERMS_VERSION);
    assert_eq!(row.get::<_, i64>(2), 10);

    // Recovery: a wrong code fails, a right one (typed loosely) sets the password and ends
    // every session, and it works only once.
    let old_session = login(&app, "kim", PW).await;
    let recover = |code: String, user: &'static str| {
        let app = app.clone();
        async move {
            call(
                &app,
                Method::POST,
                "/v1/recover",
                None,
                Some(json!({ "username": user, "code": code, "new_password": NEW_PW })),
            )
            .await
        }
    };
    let (status, _) = recover("AAAA-AAAA-AAAA-AAAA".into(), "kim").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = recover(codes[0].clone(), "nobody").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let loose = codes[0].to_lowercase().replace('-', " ");
    let (status, body) = recover(loose, "kim").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["codes_left"], 9);
    let (status, _) = call(&app, Method::GET, "/v1/me", Some(&old_session), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "sessions end on recovery");
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/login",
        None,
        Some(json!({ "username": "kim", "password": PW })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "the old password is gone");
    let session = login(&app, "kim", NEW_PW).await;
    let (status, _) = recover(codes[0].clone(), "kim").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "a code works once");

    // New codes replace the old ones.
    let (status, body) = call(
        &app,
        Method::POST,
        "/v1/recovery-codes",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["recovery_codes"].as_array().unwrap().len(),
        RECOVERY_CODES
    );
    let (status, _) = recover(codes[1].clone(), "kim").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "old codes stop working");

    // The daily allowance: once per UTC day, journaled as a grant.
    let (status, body) = call(&app, Method::GET, "/v1/allowance", Some(&session), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["claimed_today"], false);
    assert_eq!(body["amount"], 10_000);
    let (status, body) = call(&app, Method::POST, "/v1/allowance", Some(&session), None).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["granted"], 10_000);
    let (status, _) = call(&app, Method::POST, "/v1/allowance", Some(&session), None).await;
    assert_eq!(status, StatusCode::CONFLICT, "one claim a day");
    let (_, body) = call(&app, Method::GET, "/v1/allowance", Some(&session), None).await;
    assert_eq!(body["claimed_today"], true);

    // Export holds the profile and what was confirmed.
    let (status, body) = call(&app, Method::GET, "/v1/me/export", Some(&session), None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["profile"]["username"], "kim");
    assert_eq!(body["profile"]["terms_version"], TERMS_VERSION);
    assert!(body["profile"]["adult_confirmed_at"].is_string());
    assert_eq!(
        body["balance_cents"]["cash"], 1_010_000,
        "the grant plus the allowance"
    );
    assert_eq!(body["orders"], json!([]));

    // Deletion needs the password, then removes the person: no sign-in, no session, the name
    // is free again, and nothing identifying is left.
    let (status, _) = call(
        &app,
        Method::DELETE,
        "/v1/me",
        Some(&session),
        Some(json!({ "password": "not the password at all" })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = call(
        &app,
        Method::DELETE,
        "/v1/me",
        Some(&session),
        Some(json!({ "password": NEW_PW })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(&app, Method::GET, "/v1/me", Some(&session), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(
        &app,
        Method::POST,
        "/v1/login",
        None,
        Some(json!({ "username": "kim", "password": NEW_PW })),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let row = db
        .query_one("SELECT username, password_hash, deleted_at IS NOT NULL, (SELECT count(*) FROM recovery_codes r WHERE r.user_id = users.id) FROM users WHERE username LIKE 'deleted_%'", &[])
        .await
        .unwrap();
    assert!(row.get::<_, &str>(0).starts_with("deleted_"));
    assert_eq!(row.get::<_, &str>(1), "!");
    assert!(row.get::<_, bool>(2));
    assert_eq!(row.get::<_, i64>(3), 0);
    let (status, body) = register(
        json!({ "username": "kim", "password": PW, "adult": true, "terms": TERMS_VERSION }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the name is free again: {body}"
    );

    // An admin (with TOTP) can close a rule-breaker's account; a reason is required, and admin
    // accounts cannot be closed this way.
    for name in ["ada", "rob"] {
        let (status, body) = register(
            json!({ "username": name, "password": PW, "adult": true, "terms": TERMS_VERSION }),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    db.batch_execute("UPDATE users SET role = 'admin' WHERE username = 'ada'")
        .await
        .unwrap();
    let ada = common::with_totp(&app, &login(&app, "ada", PW).await, "ada", PW).await;
    let rob_session = login(&app, "rob", PW).await;
    let ids = db
        .query_one("SELECT (SELECT id FROM users WHERE username = 'rob'), (SELECT id FROM users WHERE username = 'ada')", &[])
        .await
        .unwrap();
    let (rob, ada_id): (i64, i64) = (ids.get(0), ids.get(1));
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/v1/admin/users/{rob}/close"),
        Some(&rob_session),
        Some(json!({ "reason": "wash trading" })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "traders cannot close accounts"
    );
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/v1/admin/users/{rob}/close"),
        Some(&ada),
        Some(json!({ "reason": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "a reason is required");
    let (status, _) = call(
        &app,
        Method::POST,
        &format!("/v1/admin/users/{ada_id}/close"),
        Some(&ada),
        Some(json!({ "reason": "testing the guard" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, body) = call(
        &app,
        Method::POST,
        &format!("/v1/admin/users/{rob}/close"),
        Some(&ada),
        Some(json!({ "reason": "wash trading with a second account" })),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = call(&app, Method::GET, "/v1/me", Some(&rob_session), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let audited: i64 = db
        .query_one(
            "SELECT count(*) FROM audit_log WHERE action = 'close_account'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(audited, 1);
}
