//! `brier-gateway-bench`: the registered P3/P3b/P4 study (docs/notes/gateway.md).
//!
//! Each run starts a fresh gateway in this process (its own Tokio runtime, the core thread,
//! a fresh journal on the internal SSD, a fresh PostgreSQL schema, the read-model follower),
//! then drives it over real `localhost` WebSocket connections from a separate client runtime:
//! - Phase P3: 100 accounts, one connection each, send orders open-loop at 100/s each (10,000/s
//!   in all) for a 10 s warm-up and a 60 s measured window. The mix is P1's: 60% passive GTC
//!   (buys 40-49, sells 51-60), 35% cancels of one of the account's live orders, 5% aggressive
//!   IOC (buys at 60, sells at 40); quantity 1-100; every place has a client order ID. Latency
//!   is from the client's send to its receipt of the reply, on the client's monotonic clock.
//!   The schedule's lag (how late sends went out) is reported so a run where the client could
//!   not keep up is visible.
//! - Phase P4: the same load, plus 1,000 connections subscribed to `book:1`. Each book delta
//!   carries `ts`, the publisher's wall-clock stamp; subscribers record receipt time minus `ts`
//!   (one machine, one clock).
//!
//! All 1,100 connections come from 127.0.0.1, so the per-address limits are raised for the
//! benchmark (they exist to separate different clients); the per-account order limit stays as
//! in production. Run with `make gateway-bench` (needs `make up`).
#![allow(clippy::float_arithmetic, clippy::arithmetic_side_effects)] // reporting and counters
#![allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)] // reporting only
#![allow(clippy::too_many_lines)]
// a linear benchmark script reads best top to bottom
// A measurement script: any setup or protocol failure must stop the run loudly, not be handled.
#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use brier_core::{CoreConfig, start};
use brier_gateway::feed::now_us;
use brier_gateway::limits::Limiter;
use brier_gateway::{AppState, router, run_projector};
use futures_util::{SinkExt, StreamExt};
use hdrhistogram::Histogram;
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_postgres::{Client, NoTls};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

const ACCOUNTS: usize = 100;
const MARKETS: u32 = 10;
const ORDERS_PER_SECOND_PER_ACCOUNT: u64 = 100; // 10,000/s in all
const SUBSCRIBERS: usize = 1_000;
const WARMUP: Duration = Duration::from_secs(10);
const MEASURED: Duration = Duration::from_secs(60);
const RUNS: usize = 3;

/// The registered parameters, or a short dry run (`--dry-run`) that only checks the harness
/// works: 1 run, 100 subscribers, 2 s warm-up, 5 s measured. Its output is not a result.
#[derive(Clone, Copy)]
struct Plan {
    runs: usize,
    subscribers: usize,
    warmup: Duration,
    measured: Duration,
}

const REGISTERED: Plan = Plan {
    runs: RUNS,
    subscribers: SUBSCRIBERS,
    warmup: WARMUP,
    measured: MEASURED,
};
const DRY_RUN: Plan = Plan {
    runs: 1,
    subscribers: 100,
    warmup: Duration::from_secs(2),
    measured: Duration::from_secs(5),
};
const PASSWORD: &str = "benchmark password, local only";

/// `SplitMix64`, as in `brier-bench`.
struct Rng(u64);

impl Rng {
    const fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    const fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn histogram() -> Histogram<u64> {
    Histogram::new_with_bounds(1, 60_000_000, 3).expect("valid bounds") // 1 µs to 60 s
}

fn summary(h: &Histogram<u64>) -> Value {
    let ms = |q: f64| h.value_at_quantile(q) as f64 / 1_000.0;
    json!({
        "count": h.len(), "p50_ms": ms(0.50), "p90_ms": ms(0.90), "p99_ms": ms(0.99),
        "p99_9_ms": ms(0.999), "max_ms": h.max() as f64 / 1_000.0,
    })
}

async fn connect_db(url: &str, schema: &str) -> Client {
    let (mut client, connection) = tokio_postgres::connect(url, NoTls).await.expect("database");
    tokio::spawn(connection);
    client
        .batch_execute(&format!(
            "CREATE SCHEMA IF NOT EXISTS {schema}; SET search_path TO {schema};"
        ))
        .await
        .expect("schema");
    brier_readmodel::migrate(&mut client)
        .await
        .expect("migrate");
    client
}

/// A plain HTTP/1.1 JSON request over a fresh connection (setup only).
async fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: &Value,
) -> Value {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let body = body.to_string();
    let auth = token
        .map(|t| format!("authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nhost: {addr}\r\ncontent-type: application/json\r\n{auth}content-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(addr).await.expect("connect");
    stream.write_all(request.as_bytes()).await.expect("write");
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).await.expect("read");
    let text = String::from_utf8_lossy(&raw);
    let (head, body) = text.split_once("\r\n\r\n").expect("an HTTP response");
    assert!(
        head.starts_with("HTTP/1.1 2"),
        "{method} {path}: {head} {body}"
    );
    serde_json::from_str(body).unwrap_or(Value::Null)
}

#[derive(Default)]
struct Outcomes {
    by_status: BTreeMap<String, u64>,
}

/// One trader connection: sends on schedule, records reply latency once `measuring` is set.
async fn trader(
    addr: SocketAddr,
    token: String,
    seed: u64,
    stop: Arc<AtomicBool>,
    measuring: Arc<AtomicBool>,
) -> (Histogram<u64>, Histogram<u64>, Outcomes) {
    let (ws, _) = connect_async(format!("ws://{addr}/v1/ws"))
        .await
        .expect("ws");
    let (mut sink, mut stream) = ws.split();
    sink.send(Message::Text(
        json!({ "op": "auth", "token": token, "id": 0 })
            .to_string()
            .into(),
    ))
    .await
    .expect("auth");
    let pending: Arc<Mutex<HashMap<u64, Instant>>> = Arc::default();
    let live: Arc<Mutex<Vec<u64>>> = Arc::default();
    let reader = {
        let (pending, live, measuring) = (
            Arc::clone(&pending),
            Arc::clone(&live),
            Arc::clone(&measuring),
        );
        tokio::spawn(async move {
            let (mut latency, mut outcomes) = (histogram(), Outcomes::default());
            while let Some(Ok(message)) = stream.next().await {
                let Message::Text(text) = message else {
                    continue;
                };
                let received = Instant::now();
                let m: Value = serde_json::from_str(&text).expect("json");
                let Some(id) = m["id"].as_u64().filter(|&id| id > 0) else {
                    continue;
                };
                let sent = pending.lock().expect("lock").remove(&id);
                let status = if m["ok"] == true {
                    let status = m["result"]["status"].as_str().unwrap_or("?").to_owned();
                    if status == "resting"
                        && let Some(order) = m["result"]["order"].as_u64()
                    {
                        live.lock().expect("lock").push(order);
                    }
                    status
                } else {
                    format!("error:{}", m["error"]["code"].as_str().unwrap_or("?"))
                };
                if let (Some(sent), true) = (sent, measuring.load(Ordering::Relaxed)) {
                    let us = u64::try_from(received.duration_since(sent).as_micros())
                        .unwrap_or(u64::MAX);
                    latency.saturating_record(us.max(1));
                    *outcomes.by_status.entry(status).or_default() += 1;
                }
            }
            (latency, outcomes)
        })
    };
    let mut rng = Rng(seed);
    let interval = Duration::from_micros(1_000_000 / ORDERS_PER_SECOND_PER_ACCOUNT);
    let mut next = tokio::time::Instant::now() + Duration::from_micros(rng.below(10_000));
    let (mut lag, mut id, mut client_order_id) = (histogram(), 0u64, seed << 32);
    while !stop.load(Ordering::Relaxed) {
        tokio::time::sleep_until(next).await;
        let late = tokio::time::Instant::now().saturating_duration_since(next);
        if measuring.load(Ordering::Relaxed) {
            lag.saturating_record(u64::try_from(late.as_micros()).unwrap_or(u64::MAX).max(1));
        }
        next += interval;
        id += 1;
        let r = rng.below(100);
        let market = 1 + rng.below(u64::from(MARKETS));
        let qty = 1 + rng.below(100);
        let cancel = if r < 35 {
            let mut live = live.lock().expect("lock");
            (!live.is_empty()).then(|| {
                let i = rng.below(live.len() as u64) as usize;
                live.swap_remove(i)
            })
        } else {
            None
        };
        let message = if let Some(order) = cancel {
            json!({ "op": "cancel", "id": id, "order": order })
        } else {
            client_order_id += 1;
            let (side, price, tif) = if r >= 95 {
                if rng.below(2) == 0 {
                    ("buy", 60, "ioc")
                } else {
                    ("sell", 40, "ioc")
                }
            } else if rng.below(2) == 0 {
                ("buy", 40 + rng.below(10), "gtc")
            } else {
                ("sell", 51 + rng.below(10), "gtc")
            };
            json!({ "op": "place", "id": id, "market": market, "side": side, "price": price, "qty": qty, "tif": tif, "client_order_id": client_order_id })
        };
        pending.lock().expect("lock").insert(id, Instant::now());
        if sink
            .send(Message::Text(message.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }
    tokio::time::sleep(Duration::from_secs(2)).await; // let the last replies arrive
    let _ = sink.close().await;
    let (latency, outcomes) = reader.await.expect("reader");
    (latency, lag, outcomes)
}

/// One subscriber of `book:1`: records receipt minus `ts` for every delta once measuring.
async fn subscriber(
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    measuring: Arc<AtomicBool>,
    ready: Arc<AtomicU64>,
) -> Histogram<u64> {
    let (mut ws, _) = connect_async(format!("ws://{addr}/v1/ws"))
        .await
        .expect("ws");
    ws.send(Message::Text(
        json!({ "op": "subscribe", "channel": "book:1", "id": 1 })
            .to_string()
            .into(),
    ))
    .await
    .expect("subscribe");
    ready.fetch_add(1, Ordering::Relaxed);
    let mut h = histogram();
    while !stop.load(Ordering::Relaxed) {
        let Ok(Some(Ok(Message::Text(text)))) =
            tokio::time::timeout(Duration::from_millis(200), ws.next()).await
        else {
            continue;
        };
        let now = now_us();
        if !measuring.load(Ordering::Relaxed) || !text.contains("\"type\":\"delta\"") {
            continue;
        }
        // The stamp without a full JSON parse, to keep 1,000 parsers off the measurement.
        let Some(ts) = text
            .split_once("\"ts\":")
            .and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|d| d.parse::<u64>().ok())
        else {
            continue;
        };
        h.saturating_record(now.saturating_sub(ts).max(1));
    }
    let _ = ws.close(None).await;
    h
}

struct Run {
    p3: Histogram<u64>,
    p3_lag: Histogram<u64>,
    p4: Histogram<u64>,
    p4_orders: Histogram<u64>,
    outcomes: BTreeMap<String, u64>,
}

async fn load(
    addr: SocketAddr,
    tokens: &[String],
    run: usize,
    subscribers: usize,
    plan: Plan,
) -> (Histogram<u64>, Histogram<u64>, Outcomes, Histogram<u64>) {
    let (stop, measuring, ready) = (
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicBool::new(false)),
        Arc::new(AtomicU64::new(0)),
    );
    let subs: Vec<_> = (0..subscribers)
        .map(|_| {
            tokio::spawn(subscriber(
                addr,
                Arc::clone(&stop),
                Arc::clone(&measuring),
                Arc::clone(&ready),
            ))
        })
        .collect();
    while ready.load(Ordering::Relaxed) < subscribers as u64 {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let traders: Vec<_> = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| {
            tokio::spawn(trader(
                addr,
                t.clone(),
                (run as u64) << 40 | (i as u64 + 1),
                Arc::clone(&stop),
                Arc::clone(&measuring),
            ))
        })
        .collect();
    tokio::time::sleep(plan.warmup).await;
    measuring.store(true, Ordering::Relaxed);
    tokio::time::sleep(plan.measured).await;
    measuring.store(false, Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);
    let (mut latency, mut lag, mut outcomes) = (histogram(), histogram(), Outcomes::default());
    for t in traders {
        let (l, g, o) = t.await.expect("trader");
        latency.add(&l).expect("merge");
        lag.add(&g).expect("merge");
        for (k, v) in o.by_status {
            *outcomes.by_status.entry(k).or_default() += v;
        }
    }
    let mut fanout = histogram();
    for s in subs {
        fanout.add(s.await.expect("subscriber")).expect("merge");
    }
    (latency, lag, outcomes, fanout)
}

fn one_run(url: &str, run: usize, plan: Plan) -> Run {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/gateway-bench-data");
    let _ = std::fs::remove_dir_all(&data);
    let schema = format!("bench_{}", now_us());
    let server = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("gateway")
        .enable_all()
        .build()
        .expect("runtime");
    let clients = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_name("clients")
        .enable_all()
        .build()
        .expect("runtime");
    let (core, _, core_thread) = start(CoreConfig::new(&data)).expect("core");
    let (addr, admin_db) = server.block_on(async {
        let mut state = AppState::new(core.clone(), connect_db(url, &schema).await).expect("state");
        state.address_limits = Limiter::per_second(10_000_000, 10_000_000);
        state.auth_limits = Limiter::per_second(10_000_000, 10_000_000);
        let state = Arc::new(state);
        tokio::spawn(run_projector(
            connect_db(url, &schema).await,
            data.clone(),
            core.durable(),
            Arc::clone(&state.projected),
        ));
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(listener, app).await });
        (addr, connect_db(url, &schema).await)
    });
    let tokens: Vec<String> = clients.block_on(async {
        let mut tokens = Vec::new();
        for i in 0..=ACCOUNTS {
            let name = format!("bench_{i}");
            http(
                addr,
                "POST",
                "/v1/register",
                None,
                &json!({ "username": name, "password": PASSWORD, "adult": true, "terms": "2026-11-12" }),
            )
            .await;
            if i == 0 {
                admin_db
                    .execute(
                        "UPDATE users SET role = 'admin' WHERE username = 'bench_0'",
                        &[],
                    )
                    .await
                    .expect("promote");
            }
            let login = http(
                addr,
                "POST",
                "/v1/login",
                None,
                &json!({ "username": name, "password": PASSWORD }),
            )
            .await;
            tokens.push(login["token"].as_str().expect("token").to_owned());
        }
        let admin = tokens.remove(0);
        // Admin actions need a sign-in with TOTP. The benchmark measures order flow, not
        // sign-in, so it marks the administrator's session directly.
        admin_db
            .execute(
                "UPDATE sessions SET mfa = true WHERE user_id = (SELECT id FROM users WHERE username = 'bench_0')",
                &[],
            )
            .await
            .expect("mark the admin session");
        for m in 1..=MARKETS {
            http(
                addr,
                "POST",
                "/v1/admin/markets",
                Some(&admin),
                &json!({ "rulebook": bench_rulebook(m), "reason": "gateway benchmark" }),
            )
            .await;
            http(
                addr,
                "POST",
                &format!("/v1/admin/markets/{m}/state"),
                Some(&admin),
                &json!({ "state": "open", "reason": "gateway benchmark" }),
            )
            .await;
            // The scheduler may open it first (the open time has passed); either way it is open.
        }
        tokens
    });
    eprintln!(
        "{}",
        json!({ "run": run, "phase": "P3", "msg": "measuring" })
    );
    let (p3, p3_lag, outcomes, _) = clients.block_on(load(addr, &tokens, run, 0, plan));
    eprintln!(
        "{}",
        json!({ "run": run, "phase": "P4", "msg": "measuring" })
    );
    let (p4_orders, _, _, p4) =
        clients.block_on(load(addr, &tokens, run + 100, plan.subscribers, plan));
    // Stop everything (the projector included) before dropping its schema.
    clients.shutdown_timeout(Duration::from_secs(5));
    drop(admin_db);
    server.shutdown_timeout(Duration::from_secs(5));
    drop(core);
    let _ = core_thread.join();
    let _ = std::fs::remove_dir_all(&data);
    let cleanup = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    cleanup.block_on(async {
        let (db, connection) = tokio_postgres::connect(url, NoTls).await.expect("database");
        tokio::spawn(connection);
        db.batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .expect("drop");
    });
    Run {
        p3,
        p3_lag,
        p4,
        p4_orders,
        outcomes: outcomes.by_status,
    }
}

/// Swap in use, as `sysctl vm.swapusage` reports it (macOS), for the run record.
fn swap() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "vm.swapusage"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Whether macOS reports battery power (`pmset`); false elsewhere or if unknown.
fn on_battery() -> bool {
    std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("'Battery Power'"))
}

fn main() -> ExitCode {
    let Ok(url) = std::env::var("BRIER_DATABASE_URL") else {
        eprintln!("set BRIER_DATABASE_URL (make db-url); start PostgreSQL with make up");
        return ExitCode::FAILURE;
    };
    let (mut p3, mut p4) = (histogram(), histogram());
    let args: Vec<String> = std::env::args().collect();
    // `--diagnose N`: one shorter run with N subscribers, for explaining a result; not a result.
    let diagnose = args
        .iter()
        .position(|a| a == "--diagnose")
        .and_then(|i| args.get(i + 1))
        .and_then(|n| n.parse::<usize>().ok());
    let dry_run = args.iter().any(|a| a == "--dry-run") || diagnose.is_some();
    let plan = match diagnose {
        Some(subscribers) => Plan {
            runs: 1,
            subscribers,
            warmup: Duration::from_secs(5),
            measured: Duration::from_secs(20),
        },
        None if dry_run => DRY_RUN,
        None => REGISTERED,
    };
    if dry_run {
        eprintln!("dry run: checks the harness only; the output below is not a result");
    } else if on_battery() {
        // The registered environment is AC power (docs/notes/gateway.md §3).
        eprintln!("on battery power: plug in before a registered run (or use --dry-run)");
        return ExitCode::FAILURE;
    }
    let mut valid = true;
    for run in 1..=plan.runs {
        let (battery_at_start, swap_at_start) = (on_battery(), swap());
        let r = one_run(&url, run, plan);
        let battery = battery_at_start || on_battery(); // the source can change during a run
        valid &= !battery;
        println!(
            "{}",
            json!({ "run": run, "power": if battery { "battery at start or end" } else { "ac" },
                    "swap_at_start": swap_at_start, "swap_at_end": swap(),
                    "p3_order_to_ack": summary(&r.p3), "p3_schedule_lag": summary(&r.p3_lag), "p3_outcomes": r.outcomes,
                    "p4_fanout": summary(&r.p4), "p4_order_to_ack_during_fanout": summary(&r.p4_orders) })
        );
        p3.add(&r.p3).expect("merge");
        p4.add(&r.p4).expect("merge");
    }
    let p3_p99 = p3.value_at_quantile(0.99) as f64 / 1_000.0;
    let p4_p99 = p4.value_at_quantile(0.99) as f64 / 1_000.0;
    let verdict = |ok: bool| match (dry_run || !valid, ok) {
        (true, _) => "not a result",
        (false, true) => "supported",
        (false, false) => "refuted",
    };
    println!(
        "{}",
        json!({ "pooled": { "p3_order_to_ack": summary(&p3), "p4_fanout": summary(&p4) },
                "valid": valid && !dry_run,
                "verdicts": { "P3 (p99 <= 5 ms)": verdict(p3_p99 <= 5.0),
                              "P3b (p99 <= 9.9 ms)": verdict(p3_p99 <= 9.9),
                              "P4 (p99 <= 10 ms)": verdict(p4_p99 <= 10.0) } })
    );
    if valid || dry_run {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The gateway tests' long-window rulebook, listed as market `m`.
fn bench_rulebook(m: u32) -> serde_json::Value {
    let mut r: serde_json::Value = serde_json::from_str(include_str!(
        "../../gateway/tests/fixtures/long_window_rulebook.json"
    ))
    .expect("the fixture rulebook is JSON");
    r["market"] = json!(m);
    r
}
