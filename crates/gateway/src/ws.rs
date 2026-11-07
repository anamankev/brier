//! WebSocket `/v1/ws` (SPEC §6; every message is in `docs/api/asyncapi.yaml`): market data
//! subscriptions and order entry over one connection.
//!
//! Client messages are JSON objects with an `op`:
//! - `auth` with `token` (a session), or `key`, `timestamp`, `signature` (an Ed25519 signature
//!   of a `GET /v1/ws` request with an empty body, exactly as for REST);
//! - `subscribe` / `unsubscribe` with a `channel` (`book:<m>`, `trades:<m>`, `account`);
//! - `place` (the REST order body), `cancel` (`order`), `amend` (`order`, `qty`);
//! - `ping`.
//!
//! An optional `id` is echoed in the `reply`. Order replies arrive when the command is durable,
//! and may arrive in a different order from the requests; the `id` pairs them.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade, close_code};
use axum::extract::{Extension, State};
use axum::http::{HeaderMap, HeaderValue, Method};
use axum::response::Response;
use brier_engine::Command;
use brier_types::OrderId;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;

use crate::auth::{self, User};
use crate::error::ApiError;
use crate::feed::{Channel, FeedHandle, Subscription};
use crate::routes::{PlaceBody, amend_result, cancel_result, place_result};
use crate::{AppState, ClientIp};

/// Every client `op`, as `docs/api/asyncapi.yaml` lists them.
pub const OPS: &[&str] = &[
    "ping",
    "auth",
    "subscribe",
    "unsubscribe",
    "place",
    "cancel",
    "amend",
];
/// Every server message `type`, as `docs/api/asyncapi.yaml` lists them.
pub const SERVER_MESSAGES: &[&str] = &[
    "reply",
    "snapshot",
    "delta",
    "trade",
    "subscribed",
    "resync",
    "order",
    "fill",
];

/// Largest client message. The largest legitimate one (a `place`) is under 300 bytes.
pub const MAX_MESSAGE_BYTES: usize = 8 * 1_024;
/// Channels one connection may subscribe to at once.
pub const MAX_SUBSCRIPTIONS: usize = 64;
/// Orders one connection may have waiting for durability at once.
pub const MAX_IN_FLIGHT: usize = 256;
/// Messages queued for a connection's socket. A subscriber that falls this far behind stops
/// draining its channels and so lags them, which resubscribes it (a fresh book snapshot).
const OUT_QUEUE: usize = 1_024;

/// `GET /v1/ws`: upgrade to a WebSocket.
pub async fn upgrade(
    State(s): State<Arc<AppState>>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.max_message_size(MAX_MESSAGE_BYTES)
        .max_frame_size(MAX_MESSAGE_BYTES)
        .on_upgrade(move |socket| connection(s, socket, ip))
}

struct Session {
    user: Option<User>,
    subscriptions: HashMap<Channel, JoinHandle<()>>,
    in_flight: Arc<Semaphore>,
}

fn frame(v: &Value) -> Utf8Bytes {
    Utf8Bytes::from(v.to_string())
}

fn reply(id: Option<&Value>, result: Result<Value, ApiError>) -> Utf8Bytes {
    frame(&match result {
        Ok(v) => json!({ "type": "reply", "id": id, "ok": true, "result": v }),
        Err(e) => json!({ "type": "reply", "id": id, "ok": false, "error": e.body() }),
    })
}

async fn connection(s: Arc<AppState>, mut socket: WebSocket, ip: IpAddr) {
    let (out, mut outbox) = mpsc::channel::<Utf8Bytes>(OUT_QUEUE);
    let mut shutdown = s.shutdown.subscribe();
    let mut session = Session {
        user: None,
        subscriptions: HashMap::new(),
        in_flight: Arc::new(Semaphore::new(MAX_IN_FLIGHT)),
    };
    let mut closing = None;
    loop {
        tokio::select! {
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let answer = if s.address_limits.allow(ip) {
                        handle(&s, &mut session, &out, text.as_str()).await
                    } else {
                        Some(reply(None, Err(ApiError::rate_limited())))
                    };
                    if let Some(answer) = answer
                        && socket.send(Message::Text(answer)).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Binary(_))) => {
                    closing = Some((close_code::UNSUPPORTED, "text frames only"));
                    break;
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
                Some(Ok(_)) => {} // ping and pong: answered by the library
            },
            Some(message) = outbox.recv() => {
                if socket.send(Message::Text(message)).await.is_err() {
                    break;
                }
            }
            _ = shutdown.changed() => {
                closing = Some((close_code::AWAY, "the exchange is shutting down"));
                break;
            }
        }
    }
    for (_, task) in session.subscriptions.drain() {
        task.abort();
    }
    if let Some((code, reason)) = closing {
        let _ = socket
            .send(Message::Close(Some(CloseFrame {
                code,
                reason: Utf8Bytes::from_static(reason),
            })))
            .await;
    }
}

fn parse<T: DeserializeOwned>(body: Map<String, Value>) -> Result<T, ApiError> {
    serde_json::from_value(Value::Object(body))
        .map_err(|e| ApiError::bad_request(format!("invalid message: {e}")))
}

/// Handle one client message; returns the immediate answer, if any (order replies come later,
/// through the outbox).
async fn handle(
    s: &Arc<AppState>,
    session: &mut Session,
    out: &mpsc::Sender<Utf8Bytes>,
    text: &str,
) -> Option<Utf8Bytes> {
    let mut body: Map<String, Value> = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            return Some(reply(
                None,
                Err(ApiError::bad_request(format!(
                    "messages are JSON objects: {e}"
                ))),
            ));
        }
    };
    let id = body.remove("id");
    let Some(Value::String(op)) = body.remove("op") else {
        return Some(reply(
            id.as_ref(),
            Err(ApiError::bad_request("every message needs an op")),
        ));
    };
    let result = match op.as_str() {
        "ping" => Ok(json!({ "pong": true })),
        "auth" => authenticate(s, session, body).await,
        "subscribe" => subscribe(s, session, out, body).await,
        "unsubscribe" => unsubscribe(session, body),
        "place" | "cancel" | "amend" => match order(s, session, out, &op, id.clone(), body) {
            Ok(()) => return None,
            Err(e) => Err(e),
        },
        _ => Err(ApiError::bad_request("unknown op")),
    };
    Some(reply(id.as_ref(), result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthMessage {
    token: Option<String>,
    key: Option<i64>,
    timestamp: Option<u64>,
    signature: Option<String>,
}

async fn authenticate(
    s: &AppState,
    session: &mut Session,
    body: Map<String, Value>,
) -> Result<Value, ApiError> {
    let m: AuthMessage = parse(body)?;
    let mut headers = HeaderMap::new();
    let value = |v: String| {
        HeaderValue::from_str(&v)
            .map_err(|_| ApiError::unauthorized("credentials contain invalid characters"))
    };
    if let Some(token) = m.token {
        headers.insert("authorization", value(format!("Bearer {token}"))?);
    }
    if let (Some(key), Some(timestamp), Some(signature)) = (m.key, m.timestamp, m.signature) {
        headers.insert("brier-key", value(key.to_string())?);
        headers.insert("brier-timestamp", value(timestamp.to_string())?);
        headers.insert("brier-signature", value(signature)?);
    }
    let user = auth::authenticate(
        &s.db,
        &s.replays,
        &headers,
        &Method::GET,
        "/v1/ws",
        b"",
        true,
    )
    .await?;
    session.user = Some(user);
    Ok(json!({ "user_id": user.id }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelMessage {
    channel: String,
}

fn channel(session: &Session, body: Map<String, Value>) -> Result<Channel, ApiError> {
    let ChannelMessage { channel } = parse(body)?;
    let account = session.user.map(|u| u.account()).transpose()?;
    Channel::parse(&channel, account).map_err(|code| match code {
        "unauthorized" => ApiError::unauthorized("the account channel needs auth first"),
        _ => ApiError::bad_request("channels are book:<market>, trades:<market>, and account"),
    })
}

async fn subscribe(
    s: &AppState,
    session: &mut Session,
    out: &mpsc::Sender<Utf8Bytes>,
    body: Map<String, Value>,
) -> Result<Value, ApiError> {
    let channel = channel(session, body)?;
    if session.subscriptions.contains_key(&channel) {
        return Err(ApiError::bad_request("already subscribed"));
    }
    if session.subscriptions.len() >= MAX_SUBSCRIPTIONS {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_SUBSCRIPTIONS} subscriptions per connection"
        )));
    }
    let subscription = s.feed.subscribe(channel).await.map_err(|code| match code {
        "unknown_market" => ApiError::not_found("no such market"),
        _ => ApiError::internal(&"the market-data publisher stopped"),
    })?;
    let task = tokio::spawn(forward(s.feed.clone(), channel, subscription, out.clone()));
    session.subscriptions.insert(channel, task);
    Ok(json!({ "subscribed": channel.name() }))
}

/// Copy a channel to the connection. A lagging subscriber resubscribes, so it starts again
/// from a fresh first message (on a book channel, a snapshot) rather than silently skipping.
async fn forward(
    feed: FeedHandle,
    channel: Channel,
    mut subscription: Subscription,
    out: mpsc::Sender<Utf8Bytes>,
) {
    loop {
        if out.send(subscription.first.clone()).await.is_err() {
            return;
        }
        loop {
            match subscription.rx.recv().await {
                Ok(message) => {
                    if out.send(message).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => break,
                Err(RecvError::Closed) => return,
            }
        }
        match feed.subscribe(channel).await {
            Ok(fresh) => subscription = fresh,
            Err(_) => return,
        }
    }
}

fn unsubscribe(session: &mut Session, body: Map<String, Value>) -> Result<Value, ApiError> {
    let channel = channel(session, body)?;
    let task = session
        .subscriptions
        .remove(&channel)
        .ok_or_else(|| ApiError::bad_request("not subscribed"))?;
    task.abort();
    Ok(json!({ "unsubscribed": channel.name() }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OrderMessage {
    order: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AmendMessage {
    order: u64,
    qty: u32,
}

enum Kind {
    Place,
    Cancel(OrderId),
    Amend(OrderId),
}

/// Start an order command; its reply goes to the outbox once durable.
fn order(
    s: &AppState,
    session: &Session,
    out: &mpsc::Sender<Utf8Bytes>,
    op: &str,
    id: Option<Value>,
    body: Map<String, Value>,
) -> Result<(), ApiError> {
    let user = session
        .user
        .ok_or_else(|| ApiError::unauthorized("orders need auth first"))?;
    let account = user.account()?;
    let (kind, command) = match op {
        "place" => (Kind::Place, parse::<PlaceBody>(body)?.command(account)?),
        "cancel" => {
            let order = OrderId::new(parse::<OrderMessage>(body)?.order);
            (Kind::Cancel(order), Command::Cancel { account, order })
        }
        _ => {
            let AmendMessage { order, qty } = parse(body)?;
            let order = OrderId::new(order);
            (
                Kind::Amend(order),
                Command::Amend {
                    account,
                    order,
                    qty,
                },
            )
        }
    };
    if !s.order_limits.allow(user.id) {
        return Err(ApiError::rate_limited());
    }
    let permit = Arc::clone(&session.in_flight)
        .try_acquire_owned()
        .map_err(|_| {
            ApiError::new(
                axum::http::StatusCode::TOO_MANY_REQUESTS,
                "too_many_in_flight",
                format!("at most {MAX_IN_FLIGHT} orders awaiting durability per connection"),
            )
        })?;
    let (core, out) = (s.core.clone(), out.clone());
    tokio::spawn(async move {
        let result = match core.submit(command).await {
            Ok(c) => match kind {
                Kind::Place => place_result(&c),
                Kind::Cancel(order) => cancel_result(&c, order),
                Kind::Amend(order) => amend_result(&c, order),
            },
            Err(e) => Err(e.into()),
        };
        drop(permit);
        let _ = out.send(reply(id.as_ref(), result)).await; // the connection may have closed
    });
    Ok(())
}
