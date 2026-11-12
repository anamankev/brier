//! The WebSocket API end to end, over real sockets to a running server: order entry, the
//! account channel, trades, and book channels whose snapshot plus deltas always equal the
//! engine's book, including after a subscriber lags far enough to be resynchronized.
//! Needs PostgreSQL: `make up`, then `BRIER_TEST_DATABASE_URL=$(make db-url) cargo test -p
//! brier-gateway -- --ignored` (also run by `make test-all` and the CI integration job).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::too_many_lines,
    clippy::indexing_slicing,
    clippy::panic
)] // test code

mod common;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::http::{Method, StatusCode};
use brier_core::{CoreConfig, CoreHandle, start};
use brier_engine::{Command, Event, NewOrder, TimeInForce};
use brier_gateway::{AppState, router};
use brier_types::{AccountId, MarketId, Side};
use common::{call, connect, login};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

async fn open(addr: SocketAddr) -> Ws {
    connect_async(format!("ws://{addr}/v1/ws")).await.unwrap().0
}

async fn say(ws: &mut Ws, message: Value) {
    ws.send(Message::Text(message.to_string().into()))
        .await
        .unwrap();
}

async fn next(ws: &mut Ws) -> Value {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .expect("a message within 10 s")
            .unwrap()
            .unwrap();
        if let Message::Text(text) = message {
            return serde_json::from_str(&text).unwrap();
        }
    }
}

/// Read until the reply with this `id`, returning it and everything read before it.
async fn reply(ws: &mut Ws, id: u64) -> (Value, Vec<Value>) {
    let mut before = Vec::new();
    loop {
        let m = next(ws).await;
        if m["type"] == "reply" && m["id"] == id {
            return (m, before);
        }
        before.push(m);
    }
}

/// A client-side book maintained from a book channel, as SPEC §6 requires of clients: apply a
/// snapshot, then deltas whose sequence number is exactly the previous plus one.
#[derive(Default, Debug)]
struct Book {
    seq: Option<u64>,
    journal_seq: u64,
    bids: BTreeMap<u64, u64>,
    asks: BTreeMap<u64, u64>,
    /// Deltas that arrived out of sequence (should never happen: the server resynchronizes).
    gaps: u64,
    snapshots: u64,
    /// Messages a resynchronizing snapshot skipped over.
    skipped: u64,
}

impl Book {
    fn apply(&mut self, m: &Value) {
        let seq = m["seq"].as_u64().unwrap();
        let levels = |side: &Value, into: &mut BTreeMap<u64, u64>| {
            for l in side.as_array().unwrap() {
                let (p, q) = (l[0].as_u64().unwrap(), l[1].as_u64().unwrap());
                if q == 0 {
                    into.remove(&p);
                } else {
                    into.insert(p, q);
                }
            }
        };
        match m["type"].as_str().unwrap() {
            "snapshot" => {
                if let Some(last) = self.seq {
                    self.skipped += seq.saturating_sub(last + 1);
                }
                self.bids.clear();
                self.asks.clear();
                self.snapshots += 1;
            }
            "delta" => {
                if self.seq.is_some_and(|s| s + 1 != seq) {
                    self.gaps += 1;
                    self.seq = None; // wait for the snapshot the server sends after a lag
                }
                if self.seq.is_none() {
                    return;
                }
            }
            other => panic!("unexpected book message {other}"),
        }
        levels(&m["bids"], &mut self.bids);
        levels(&m["asks"], &mut self.asks);
        self.seq = Some(seq);
        self.journal_seq = m["journal_seq"].as_u64().unwrap();
    }

    fn as_rest(&self) -> (Value, Value) {
        let bids: Vec<Value> = self.bids.iter().rev().map(|(p, q)| json!([p, q])).collect();
        let asks: Vec<Value> = self.asks.iter().map(|(p, q)| json!([p, q])).collect();
        (Value::Array(bids), Value::Array(asks))
    }
}

fn place(account: u64, side: Side, price: u8, qty: u32) -> Command {
    Command::Place(NewOrder {
        account: AccountId::new(account),
        market: MarketId::new(1),
        side,
        price,
        qty,
        tif: TimeInForce::Gtc,
        post_only: false,
        client_order_id: None,
    })
}

/// Many book changes at once, straight to the core: waves of resting orders, then their
/// cancels, so the book channel carries far more messages than a subscriber's buffers hold.
async fn churn(core: &CoreHandle, account: u64, waves: u32, per_wave: u32) {
    for wave in 0..waves {
        let placed = futures_util::future::join_all((0..per_wave).map(|i| {
            let price = u8::try_from(2 + (wave + i) % 40).unwrap();
            core.submit(place(account, Side::Buy, price, 1 + i % 5))
        }))
        .await;
        let cancels = placed.into_iter().filter_map(|c| {
            c.unwrap().events.iter().find_map(|e| {
                if let Event::Rested { order, .. } = e {
                    Some(*order)
                } else {
                    None
                }
            })
        });
        let cancels: Vec<_> = cancels
            .map(|order| {
                core.submit(Command::Cancel {
                    account: AccountId::new(account),
                    order,
                })
            })
            .collect();
        for c in futures_util::future::join_all(cancels).await {
            c.unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs PostgreSQL (make up; BRIER_TEST_DATABASE_URL)"]
async fn the_websocket_api_end_to_end() {
    let schema = common::schema("gateway_ws");
    let data = tempfile::tempdir().unwrap();
    let (core, _, _thread) = start(CoreConfig::new(data.path())).unwrap();
    let state = Arc::new(
        AppState::new(core.clone(), connect(&schema).await)
            .unwrap()
            .with_totp_key(common::TOTP_KEY),
    );
    let admin_db = connect(&schema).await;
    let app = router(Arc::clone(&state));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = app
        .clone()
        .into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(listener, server).await });

    let pw = "a long enough password";
    for name in ["alice", "bob"] {
        let (status, _) = call(
            &app,
            Method::POST,
            "/v1/register",
            None,
            Some(json!({ "username": name, "password": pw, "adult": true, "terms": "2026-11-12" })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    admin_db
        .execute(
            "UPDATE users SET role = 'admin' WHERE username = 'alice'",
            &[],
        )
        .await
        .unwrap();
    let (alice, bob) = (login(&app, "alice", pw).await, login(&app, "bob", pw).await);
    let alice = common::with_totp(&app, &alice, "alice", pw).await;
    for (path, body) in [
        (
            "/v1/admin/markets",
            json!({ "rulebook": common::rulebook(1), "reason": "websocket test market" }),
        ),
        (
            "/v1/admin/markets/1/state",
            json!({ "state": "open", "reason": "open for the test" }),
        ),
    ] {
        let (status, body) = call(&app, Method::POST, path, Some(&alice), Some(body)).await;
        assert!(status.is_success(), "{body}");
    }

    // Public market data needs no auth; private channels and orders do.
    let mut watcher = open(addr).await;
    say(
        &mut watcher,
        json!({ "op": "subscribe", "channel": "book:1", "id": 1 }),
    )
    .await;
    let (ack, _) = reply(&mut watcher, 1).await;
    assert_eq!(ack["ok"], true, "{ack}");
    let mut book = Book::default();
    book.apply(&next(&mut watcher).await);
    assert_eq!(book.snapshots, 1);
    say(
        &mut watcher,
        json!({ "op": "subscribe", "channel": "trades:1", "id": 2 }),
    )
    .await;
    let (_, before) = reply(&mut watcher, 2).await;
    assert!(before.is_empty(), "{before:?}");
    assert_eq!(next(&mut watcher).await["type"], "subscribed");
    for (id, message) in [
        (3, json!({ "op": "subscribe", "channel": "account" })),
        (
            4,
            json!({ "op": "place", "market": 1, "side": "buy", "price": 40, "qty": 1 }),
        ),
        (5, json!({ "op": "subscribe", "channel": "book:99" })),
        (6, json!({ "op": "fly" })),
    ] {
        let mut m = message;
        m["id"] = json!(id);
        say(&mut watcher, m).await;
        let (r, _) = reply(&mut watcher, id).await;
        assert_eq!(r["ok"], false, "{r}");
    }

    // Order entry and the account channel.
    let mut maker = open(addr).await;
    say(&mut maker, json!({ "op": "auth", "token": bob, "id": 10 })).await;
    assert_eq!(reply(&mut maker, 10).await.0["ok"], true);
    say(
        &mut maker,
        json!({ "op": "subscribe", "channel": "account", "id": 11 }),
    )
    .await;
    assert_eq!(reply(&mut maker, 11).await.0["ok"], true);
    assert_eq!(next(&mut maker).await["type"], "subscribed");
    say(&mut maker, json!({ "op": "place", "id": 12, "market": 1, "side": "sell", "price": 60, "qty": 10, "client_order_id": 1 })).await;
    let (placed, private) = reply(&mut maker, 12).await;
    assert_eq!(placed["result"]["status"], "resting", "{placed}");
    let order = placed["result"]["order"].as_u64().unwrap();
    let mut private = private;
    while private.len() < 2 {
        private.push(next(&mut maker).await);
    }
    let events: Vec<&str> = private
        .iter()
        .map(|m| m["event"].as_str().unwrap())
        .collect();
    assert_eq!(events, ["accepted", "rested"]);
    assert_eq!(
        private[1]["seq"].as_u64(),
        private[0]["seq"].as_u64().map(|s| s + 1)
    );

    let mut taker = open(addr).await;
    say(
        &mut taker,
        json!({ "op": "auth", "token": alice, "id": 20 }),
    )
    .await;
    assert_eq!(reply(&mut taker, 20).await.0["ok"], true);
    say(&mut taker, json!({ "op": "place", "id": 21, "market": 1, "side": "buy", "price": 60, "qty": 4, "tif": "ioc" })).await;
    let (ioc_reply, _) = reply(&mut taker, 21).await;
    assert_eq!(
        ioc_reply["result"]["fills"],
        json!([{ "price": 60, "qty": 4 }]),
        "{ioc_reply}"
    );
    let fill = loop {
        let m = next(&mut maker).await;
        if m["type"] == "fill" {
            break m;
        }
    };
    assert_eq!(
        (
            fill["order"].as_u64(),
            fill["liquidity"].as_str(),
            fill["qty"].as_u64()
        ),
        (Some(order), Some("maker"), Some(4))
    );
    say(
        &mut maker,
        json!({ "op": "amend", "id": 13, "order": order, "qty": 2 }),
    )
    .await;
    assert_eq!(reply(&mut maker, 13).await.0["result"]["remaining"], 2);
    say(
        &mut maker,
        json!({ "op": "cancel", "id": 14, "order": order }),
    )
    .await;
    assert_eq!(reply(&mut maker, 14).await.0["result"]["cancelled"], 2);
    say(
        &mut maker,
        json!({ "op": "cancel", "id": 15, "order": order }),
    )
    .await;
    let (again, _) = reply(&mut maker, 15).await;
    assert_eq!(again["error"]["code"], "unknown_order", "{again}");

    // The watcher saw every change, in order, and a trade without identities.
    let mut trades = Vec::new();
    while book.journal_seq < *core.durable().borrow() - 1 || trades.is_empty() {
        let m = next(&mut watcher).await;
        match m["channel"].as_str().unwrap() {
            "book:1" => book.apply(&m),
            "trades:1" => trades.push(m),
            other => panic!("{other}"),
        }
    }
    assert_eq!(trades.len(), 1);
    assert_eq!(
        (
            trades[0]["price"].as_u64(),
            trades[0]["qty"].as_u64(),
            trades[0]["taker_side"].as_str()
        ),
        (Some(60), Some(4), Some("buy"))
    );
    assert!(trades[0].get("maker_account").is_none());
    assert_eq!(book.gaps, 0);
    let (_, rest) = call(&app, Method::GET, "/v1/markets/1/book", None, None).await;
    assert_eq!(book.as_rest(), (rest["bids"].clone(), rest["asks"].clone()));

    // A subscriber that stops reading lags, gets a fresh snapshot, and its book is right again.
    let mut slow = open(addr).await;
    say(
        &mut slow,
        json!({ "op": "subscribe", "channel": "book:1", "id": 30 }),
    )
    .await;
    let (_, _) = reply(&mut slow, 30).await;
    let mut slow_book = Book::default();
    slow_book.apply(&next(&mut slow).await);
    let bob_id: i64 = admin_db
        .query_one("SELECT id FROM users WHERE username = 'bob'", &[])
        .await
        .unwrap()
        .get(0);
    churn(&core, u64::try_from(bob_id).unwrap(), 40, 500).await; // 40,000 book changes
    core.submit(place(u64::try_from(bob_id).unwrap(), Side::Sell, 70, 3))
        .await
        .unwrap();
    let durable = *core.durable().borrow();
    while slow_book.journal_seq < durable {
        slow_book.apply(&next(&mut slow).await);
    }
    assert!(
        slow_book.snapshots >= 2 && slow_book.skipped > 0 && slow_book.gaps == 0,
        "the slow subscriber lagged and was resynchronized by a snapshot, never a gap: {slow_book:?}"
    );
    let (_, rest) = call(&app, Method::GET, "/v1/markets/1/book", None, None).await;
    assert_eq!(
        slow_book.as_rest(),
        (rest["bids"].clone(), rest["asks"].clone())
    );
    assert_eq!(rest["asks"], json!([[70, 3]]));

    // Shutdown closes connections with "going away".
    state.shutdown.send_replace(true);
    let closed = loop {
        match tokio::time::timeout(Duration::from_secs(5), watcher.next())
            .await
            .unwrap()
        {
            Some(Ok(Message::Close(frame))) => break frame,
            Some(Ok(_)) => {}
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(closed.map(|f| u16::from(f.code)), Some(1001));

    admin_db
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}
