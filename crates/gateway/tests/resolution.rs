//! Listing from rulebooks, the schedule, and the resolution workflow end to end, on a manual
//! clock (docs/resolution.md): list, open and close by schedule, propose, confirm (four eyes),
//! dispute, rule, settle, and the settler finishing a settlement after a crash.
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
use brier_engine::{Command, MarketState};
use brier_gateway::clock::Clock;
use brier_gateway::scheduler::{TickReport, tick};
use brier_gateway::{AppState, router, run_projector};
use brier_rules::time::{format_instant, instant};
use brier_types::{Digest, MarketId, Outcome};
use common::{call, connect, login};
use serde_json::{Value, json};
use tokio_postgres::Client;

const WINDOW: i64 = 3_600;

fn t(s: &str) -> i64 {
    instant(s).unwrap()
}

/// The CPI market of docs/rulebook.md, as market `market`.
fn cpi(market: u32) -> Value {
    json!({
        "schema": 1, "market": market,
        "title": "CPI inflation above 3.0% in September 2026",
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

fn evidence(market: u32, rules_hash: &str, outcome: &str, retrieved_at: i64) -> Value {
    json!({
        "market": market, "rules_hash": rules_hash, "outcome": outcome,
        "retrieved_at": format_instant(retrieved_at).unwrap(),
        "source": "bls", "series": "CUUR0000SA0", "value": "3.4",
        "body_sha256": "ab".repeat(32),
    })
}

struct World {
    app: Router,
    state: Arc<AppState>,
    clock: Clock,
    db: Client,
}

impl World {
    async fn post(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        call(&self.app, Method::POST, path, Some(token), Some(body)).await
    }

    async fn get(&self, path: &str) -> (StatusCode, Value) {
        call(&self.app, Method::GET, path, None, None).await
    }

    async fn tick_at(&self, at: i64) -> TickReport {
        self.clock.set(at);
        tick(&self.state).await.unwrap()
    }

    async fn state_of(&self, market: u32) -> Option<MarketState> {
        brier_gateway::markets::market_state(&self.state, MarketId::new(market))
            .await
            .unwrap()
    }

    /// Wait until the read models reach everything durable.
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

fn code(body: &Value) -> &str {
    body["error"]["code"].as_str().unwrap_or("")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn markets_resolve_through_propose_confirm_dispute_and_settle() {
    let schema = common::schema("gateway_resolution");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let clock = Clock::manual(t("2026-08-20T00:00:00Z"));
    // Several users sign up and sign in from one test address, beyond the per-address auth
    // burst; the limit itself is tested in rest.rs.
    let mut base = AppState::new(core.clone(), connect(&schema).await).unwrap();
    base.auth_limits = brier_gateway::limits::Limiter::per_minute(1_000, 1_000);
    let state = Arc::new(
        base.with_totp_key(common::TOTP_KEY)
            .with_clock(clock.clone())
            .with_dispute_window(WINDOW),
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
        db: connect(&schema).await,
    };
    let pw = "a long enough password";
    for name in ["alice", "carol", "rex", "bob", "dave"] {
        let (status, body) = call(
            &w.app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": pw, "adult": true, "terms": "2026-11-12" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    w.db.batch_execute(
        "UPDATE users SET role = 'admin' WHERE username IN ('alice', 'carol'); UPDATE users SET role = 'resolver' WHERE username = 'rex';",
    )
    .await
    .unwrap();
    let mut tokens = Vec::new();
    for name in ["alice", "carol", "rex", "bob", "dave"] {
        tokens.push(login(&w.app, name, pw).await);
    }
    let [alice, carol, rex, bob, dave] = tokens.try_into().unwrap();
    let alice = common::with_totp(&w.app, &alice, "alice", pw).await;
    let carol = common::with_totp(&w.app, &carol, "carol", pw).await;

    // ---- Listing ---------------------------------------------------------------------------
    let mut bad = cpi(7);
    bad["condition"]["threshold"] = json!("3.00");
    let (status, body) = w
        .post(
            "/v1/admin/markets",
            &alice,
            json!({ "rulebook": bad, "reason": "list CPI" }),
        )
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::BAD_REQUEST, "invalid_rulebook")
    );
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("threshold"),
        "{body}"
    );
    let (status, _) = w
        .post(
            "/v1/admin/markets",
            &rex,
            json!({ "rulebook": cpi(7), "reason": "list CPI" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a resolver cannot list markets"
    );
    let expected = brier_rules::validate(&cpi(7)).unwrap();
    let (status, body) = w
        .post(
            "/v1/admin/markets",
            &alice,
            json!({ "rulebook": cpi(7), "reason": "list CPI" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(body["rules_hash"], json!(expected.hash_hex()));
    assert_eq!(body["events"][0]["rules_hash"], json!(expected.hash_hex()));
    let (status, body) = w
        .post(
            "/v1/admin/markets",
            &alice,
            json!({ "rulebook": cpi(7), "reason": "list CPI" }),
        )
        .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "conflict"));
    let (status, _) = w
        .post(
            "/v1/admin/markets",
            &alice,
            json!({ "rulebook": cpi(8), "reason": "list the second CPI market" }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    w.projected().await;
    let (status, rules) = w.get("/v1/markets/7/rules").await;
    assert_eq!(status, StatusCode::OK, "{rules}");
    assert_eq!(rules["rules_hash"], json!(expected.hash_hex()));
    assert_eq!(rules["canonical"], json!(expected.canonical));
    assert_eq!(rules["text"], json!(expected.describe()));
    assert_eq!(rules["journal"]["matches"], json!(true), "{rules}");
    let (status, _) = w.get("/v1/markets/9/rules").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // ---- Schedule ----------------------------------------------------------------------------
    let (status, body) = w
        .post(
            "/v1/admin/markets/7/state",
            &alice,
            json!({ "state": "open", "reason": "open early" }),
        )
        .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "too_early"));
    assert_eq!(
        w.tick_at(t("2026-09-01T13:29:59Z")).await,
        TickReport::default()
    );
    let opened = w.tick_at(t("2026-09-01T13:30:00Z")).await;
    assert_eq!(opened.opened, 2);
    assert_eq!(w.state_of(7).await, Some(MarketState::Open));

    // Bob buys 10 YES at 40 from Alice; Dave holds nothing. A resolver cannot trade.
    let (status, body) = w
        .post(
            "/v1/orders",
            &alice,
            json!({ "market": 7, "side": "sell", "price": 40, "qty": 10 }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, body) = w
        .post(
            "/v1/orders",
            &bob,
            json!({ "market": 7, "side": "buy", "price": 40, "qty": 10 }),
        )
        .await;
    assert_eq!(body["filled"], json!(10), "{status} {body}");
    let (status, _) = w
        .post(
            "/v1/orders",
            &rex,
            json!({ "market": 7, "side": "buy", "price": 40, "qty": 1 }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let release = t("2026-10-15T12:30:00Z");
    let rules_hash = expected.hash_hex();
    let yes = evidence(7, &rules_hash, "yes", release + 30);
    let propose = |outcome: &str, ev: &Value| json!({ "outcome": outcome, "evidence": ev, "reason": "BLS published September CPI" });
    let (status, body) = w
        .post("/v1/markets/7/resolution", &rex, propose("yes", &yes))
        .await;
    assert_eq!(
        (status, code(&body)),
        (StatusCode::CONFLICT, "conflict"),
        "an open market cannot be resolved"
    );
    let closed = w.tick_at(t("2026-10-15T12:00:00Z")).await;
    assert_eq!(closed.closed, 2);
    assert_eq!(w.state_of(7).await, Some(MarketState::Closed));
    let (status, body) = w
        .post(
            "/v1/admin/markets/7/state",
            &alice,
            json!({ "state": "open", "reason": "reopen after close" }),
        )
        .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "conflict"));

    // ---- Propose -----------------------------------------------------------------------------
    w.clock.set(release - 1);
    let (status, body) = w
        .post("/v1/markets/7/resolution", &rex, propose("yes", &yes))
        .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "too_early"));
    w.clock.set(release + 60);
    let (status, _) = w
        .post("/v1/markets/7/resolution", &bob, propose("yes", &yes))
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a trader cannot propose");
    for (body, why) in [
        (propose("no", &yes), "evidence for another outcome"),
        (
            propose("void", &evidence(7, &rules_hash, "void", release + 30)),
            "void without a condition",
        ),
        (
            propose("yes", &evidence(7, &"00".repeat(32), "yes", release + 30)),
            "another rulebook",
        ),
        (
            propose("yes", &evidence(7, &rules_hash, "yes", release - 1)),
            "retrieved before release",
        ),
        (
            propose("yes", &evidence(7, &rules_hash, "yes", release + 3_600)),
            "retrieved in the future",
        ),
        (
            propose("yes", &evidence(8, &rules_hash, "yes", release + 30)),
            "another market",
        ),
    ] {
        let (status, err) = w.post("/v1/markets/7/resolution", &rex, body).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {err}");
    }
    let (status, body) = w
        .post("/v1/markets/7/resolution", &rex, propose("yes", &yes))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let first = body["resolution"].as_i64().unwrap();
    let (status, _) = w
        .post("/v1/markets/7/resolution", &rex, propose("yes", &yes))
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "one live resolution per market"
    );

    // ---- Confirm, dispute, uphold ------------------------------------------------------------
    let (status, _) = w
        .post(
            &format!("/v1/resolutions/{first}/disputes"),
            &bob,
            json!({ "reason": "too soon" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "only a confirmed resolution can be disputed"
    );
    let (status, _) = w
        .post(
            &format!("/v1/admin/resolutions/{first}/confirm"),
            &rex,
            json!({ "reason": "looks right" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "a resolver cannot confirm");
    let (status, body) = w
        .post(
            &format!("/v1/admin/resolutions/{first}/confirm"),
            &alice,
            json!({ "reason": "checked against the BLS release" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body["dispute_until"],
        json!(format_instant(release + 60 + WINDOW).unwrap())
    );
    let (_, queue) = call(&w.app, Method::GET, "/v1/admin/queue", Some(&carol), None).await;
    assert_eq!(
        queue["resolutions"][0]["state"],
        json!("confirmed"),
        "{queue}"
    );
    assert_eq!(queue["resolutions"][0]["yours"], json!(false));
    let path = format!("/v1/resolutions/{first}/disputes");
    let (status, _) = w
        .post(&path, &dave, json!({ "reason": "I disagree" }))
        .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "only position holders dispute"
    );
    let (status, body) = w
        .post(&path, &bob, json!({ "reason": "the value was revised" }))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let first_dispute = body["dispute"].as_i64().unwrap();
    let (status, _) = w.post(&path, &bob, json!({ "reason": "again" })).await;
    assert_eq!(status, StatusCode::CONFLICT, "one dispute per holder");
    let (_, queue) = call(&w.app, Method::GET, "/v1/admin/queue", Some(&carol), None).await;
    assert_eq!(
        queue["disputes"][0]["reason"],
        json!("the value was revised"),
        "{queue}"
    );
    let (status, _) = call(&w.app, Method::GET, "/v1/admin/queue", Some(&bob), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let report = w.tick_at(release + 60 + WINDOW + 1).await;
    assert_eq!(report.settled, 0, "an open dispute stops settlement");
    let rule = format!("/v1/admin/disputes/{first_dispute}/rule");
    let (status, body) = w
        .post(&rule, &carol, json!({ "decision": "uphold", "reason": "the evidence was retrieved from the wrong release" }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = w
        .post(
            &rule,
            &carol,
            json!({ "decision": "reject", "reason": "changed my mind" }),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT, "a dispute is ruled on once");
    let (_, list) = w.get("/v1/markets/7/resolution").await;
    assert_eq!(
        list["resolutions"][0]["state"],
        json!("withdrawn"),
        "{list}"
    );

    // ---- Propose again (admin), withdraw, four eyes, reject a dispute, settle ---------------
    let now = w.clock.now();
    let yes_again = evidence(7, &rules_hash, "yes", now - 10);
    let (status, body) = w
        .post(
            "/v1/markets/7/resolution",
            &alice,
            propose("yes", &yes_again),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let second = body["resolution"].as_i64().unwrap();
    let (status, _) = w
        .post(
            &format!("/v1/admin/resolutions/{second}/withdraw"),
            &carol,
            json!({ "reason": "wrong reason text" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = w
        .post(
            "/v1/markets/7/resolution",
            &alice,
            propose("yes", &yes_again),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the same evidence can back a new proposal: {body}"
    );
    let third = body["resolution"].as_i64().unwrap();
    let evidence_hash = body["evidence_hash"].as_str().unwrap().to_owned();
    let (status, _) = w
        .post(
            &format!("/v1/admin/resolutions/{third}/confirm"),
            &alice,
            json!({ "reason": "my own proposal" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "four eyes");
    let (status, body) = w
        .post(
            &format!("/v1/admin/resolutions/{third}/confirm"),
            &carol,
            json!({ "reason": "checked against the BLS release" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let confirmed_at = w.clock.now();
    let (status, body) = w
        .post(
            &format!("/v1/resolutions/{third}/disputes"),
            &alice,
            json!({ "reason": "testing my own" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the seller holds NO and may dispute: {body}"
    );
    let own = body["dispute"].as_i64().unwrap();
    let (status, _) = w
        .post(
            &format!("/v1/admin/disputes/{own}/rule"),
            &alice,
            json!({ "decision": "reject", "reason": "my own proposal" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "the proposer cannot rule");
    let (status, _) = w
        .post(
            &format!("/v1/admin/disputes/{own}/rule"),
            &carol,
            json!({ "decision": "reject", "reason": "the value matches the release" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        w.tick_at(confirmed_at + WINDOW - 1).await.settled,
        0,
        "inside the window"
    );
    let report = w.tick_at(confirmed_at + WINDOW).await;
    assert_eq!(report.settled, 1);
    assert_eq!(w.state_of(7).await, Some(MarketState::Resolved));
    let (status, _) = w
        .post(
            &format!("/v1/resolutions/{third}/disputes"),
            &bob,
            json!({ "reason": "late" }),
        )
        .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a settled resolution cannot be disputed"
    );
    let (_, portfolio) = call(&w.app, Method::GET, "/v1/portfolio", Some(&bob), None).await;
    assert_eq!(
        portfolio["cash"],
        json!(brier_engine::SIGNUP_GRANT.cents() - 400 + 1_000),
        "{portfolio}"
    );

    // The journal pins the rules and the evidence; the stored rows agree with it.
    w.projected().await;
    let row = w
        .db
        .query_one(
            "SELECT j.rules_hash = b.rules_hash, j.evidence_hash = r.evidence_hash, j.outcome, j.settled_seq = r.settled_seq \
             FROM market_journal j JOIN rulebooks b ON b.market = j.market JOIN resolutions r ON r.market = j.market AND r.state = 'settled' \
             WHERE j.market = 7",
            &[],
        )
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0) && row.get::<_, bool>(1) && row.get::<_, bool>(3));
    assert_eq!(row.get::<_, &str>(2), "yes");
    let (_, list) = w.get("/v1/markets/7/resolution").await;
    assert_eq!(list["resolutions"][0]["state"], json!("settled"));
    assert_eq!(
        list["resolutions"][0]["evidence_hash"],
        json!(evidence_hash)
    );
    assert_eq!(
        list["resolutions"][0]["disputes"],
        json!({ "open": 0, "total": 1 })
    );

    // ---- Void after the deadline, and the settler finishing after a crash --------------------
    let deadline = t("2026-12-15T12:30:00Z");
    w.clock.set(deadline - 1);
    let void = |at| {
        json!({
            "outcome": "void", "void_condition": "no_value_by_deadline",
            "evidence": evidence(8, &brier_rules::validate(&cpi(8)).unwrap().hash_hex(), "void", at),
            "reason": "no value by the deadline",
        })
    };
    let (status, body) = w
        .post("/v1/markets/8/resolution", &rex, void(deadline - 10))
        .await;
    assert_eq!((status, code(&body)), (StatusCode::CONFLICT, "too_early"));
    w.clock.set(deadline + 60);
    let (status, body) = w
        .post("/v1/markets/8/resolution", &rex, void(deadline - 10))
        .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "evidence from before the deadline: {body}"
    );
    let (status, body) = w
        .post("/v1/markets/8/resolution", &rex, void(deadline + 30))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let voided = body["resolution"].as_i64().unwrap();
    let void_hash = body["evidence_hash"].as_str().unwrap().to_owned();
    let (status, _) = w
        .post(
            &format!("/v1/admin/resolutions/{voided}/confirm"),
            &carol,
            json!({ "reason": "no release by the deadline" }),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    // A crash after `settle` was journaled but before the row was updated: the claimed row and
    // the journal disagree until the settler's next pass completes the row from the journal.
    w.db.execute(
        "UPDATE resolutions SET state = 'settling' WHERE id = $1",
        &[&voided],
    )
    .await
    .unwrap();
    let mut bytes = [0u8; 32];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&void_hash[2 * i..2 * i + 2], 16).unwrap();
    }
    let settled = w
        .state
        .core
        .submit(Command::Settle {
            market: MarketId::new(8),
            outcome: Outcome::Void,
            evidence_hash: Some(Digest::new(bytes)),
        })
        .await
        .unwrap();
    w.projected().await;
    assert_eq!(w.tick_at(deadline + 120).await.settled, 1);
    let seq: i64 =
        w.db.query_one(
            "SELECT settled_seq FROM resolutions WHERE id = $1 AND state = 'settled'",
            &[&voided],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(u64::try_from(seq).unwrap(), settled.seq.get());

    let (_, audit) = call(&w.app, Method::GET, "/v1/admin/audit", Some(&alice), None).await;
    assert!(
        audit["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["actor"] == "system" && e["action"] == "settle"),
        "{audit}"
    );
    // Every step is audited; the gateway's own commands as `system`, with their seq.
    let system: Vec<String> =
        w.db.query(
            "SELECT action FROM audit_log WHERE actor = 'system' AND seq IS NOT NULL ORDER BY id",
            &[],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect();
    assert_eq!(
        system,
        [
            "schedule_open",
            "schedule_open",
            "schedule_close",
            "schedule_close",
            "settle"
        ]
    );
    let steps: i64 = w
        .db
        .query_one(
            "SELECT count(*) FROM audit_log WHERE action IN ('propose_resolution', 'confirm_resolution', 'withdraw_resolution', 'file_dispute', 'rule_dispute')",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(steps, 4 + 3 + 1 + 2 + 2);
}
