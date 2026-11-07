//! Market data (SPEC §6): one publisher task turns the core's durable batches into channels.
//!
//! - `book:<market>`: a snapshot of aggregated levels, then one delta per command that changed
//!   the book, each carrying the new quantity of every level it touched (0 = level gone).
//! - `trades:<market>`: every execution, without account identities.
//! - `account`: the subscriber's own order events and fills (authenticated).
//!
//! Every message on a channel carries that channel's sequence number, consecutive from the
//! subscription's first message, and the journal sequence number it reflects. A client that
//! sees a number that is not the previous plus one has missed messages; on the book channel the
//! server then sends a fresh snapshot, which replaces the client's book.
//!
//! The publisher builds its books from the core's [`Query::Feed`] answer (every resting order)
//! and then follows the broadcast, skipping commands the answer already reflects. If it falls
//! behind the broadcast, or its books disagree with an event, it reloads from the core and
//! sends every book subscriber a fresh snapshot. Messages are serialized once per channel, not
//! once per subscriber, and only when someone is subscribed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use axum::extract::ws::Utf8Bytes;
use brier_core::{Answer, Batch, Committed, CoreError, CoreHandle, Query};
use brier_engine::{Command, Event, MarketState, RestingOrder};
use brier_types::{AccountId, MarketId, OrderId, Side};
use brier_wire::{cancel_name, reject_name, side_name, state_name};
use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Messages buffered per channel for subscribers. A subscriber further behind than this has
/// lagged: it is resubscribed, which on a book channel means a fresh snapshot.
pub const CHANNEL_BUFFER: usize = 1_024;
/// Subscription requests waiting for the publisher.
const REQUEST_QUEUE: usize = 4_096;

/// A market-data channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Aggregated book levels of one market.
    Book(MarketId),
    /// Executions in one market.
    Trades(MarketId),
    /// One account's order events and fills.
    Account(AccountId),
}

impl Channel {
    /// Parse a channel name. `account` names the caller's own account, so it needs one.
    ///
    /// # Errors
    /// A stable error code: `unknown_channel`, or `unauthorized` for `account` without one.
    pub fn parse(name: &str, account: Option<AccountId>) -> Result<Self, &'static str> {
        if name == "account" {
            return account.map(Self::Account).ok_or("unauthorized");
        }
        let market = |rest: &str| {
            rest.parse()
                .map(MarketId::new)
                .map_err(|_| "unknown_channel")
        };
        match name.split_once(':') {
            Some(("book", m)) => market(m).map(Self::Book),
            Some(("trades", m)) => market(m).map(Self::Trades),
            _ => Err("unknown_channel"),
        }
    }

    /// The channel's name, as clients write it.
    #[must_use]
    pub fn name(self) -> String {
        match self {
            Self::Book(m) => format!("book:{}", m.get()),
            Self::Trades(m) => format!("trades:{}", m.get()),
            Self::Account(_) => "account".to_owned(),
        }
    }
}

/// A new subscription: the first message to send (a book snapshot, or `subscribed` with the
/// current sequence number), then the channel's messages.
pub struct Subscription {
    /// The first message.
    pub first: Utf8Bytes,
    /// Every later message on the channel.
    pub rx: broadcast::Receiver<Utf8Bytes>,
}

type Reply = oneshot::Sender<Result<Subscription, &'static str>>;

/// A cloneable handle to the publisher.
#[derive(Clone)]
pub struct FeedHandle {
    requests: mpsc::Sender<(Channel, Reply)>,
}

impl FeedHandle {
    /// Subscribe to a channel.
    ///
    /// # Errors
    /// A stable error code: `unknown_market`, or `unavailable` if the publisher stopped.
    pub async fn subscribe(&self, channel: Channel) -> Result<Subscription, &'static str> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send((channel, reply))
            .await
            .map_err(|_| "unavailable")?;
        answer.await.map_err(|_| "unavailable")?
    }
}

/// Start the publisher for a running core. It stops when every [`FeedHandle`] is dropped.
#[must_use]
pub fn spawn(core: CoreHandle) -> FeedHandle {
    let (requests, inbox) = mpsc::channel(REQUEST_QUEUE);
    tokio::spawn(run(core, inbox));
    FeedHandle { requests }
}

fn log(level: &str, msg: &str, detail: &str) {
    eprintln!(
        "{}",
        json!({ "level": level, "msg": msg, "detail": detail })
    );
}

async fn run(core: CoreHandle, mut inbox: mpsc::Receiver<(Channel, Reply)>) {
    let mut batches = core.subscribe();
    let mut feed = Feed::default();
    if let Err(e) = feed.reload(&core).await {
        log("error", "market data could not load", &e.to_string());
        return;
    }
    loop {
        tokio::select! {
            batch = batches.recv() => {
                let trouble = match batch {
                    Ok(batch) => feed.apply_batch(&batch).err().map(|e| e.0.to_owned()),
                    Err(RecvError::Lagged(n)) => Some(format!("fell {n} batches behind the core")),
                    Err(RecvError::Closed) => return,
                };
                if let Some(why) = trouble {
                    log("warn", "market data reloading from the core", &why);
                    if let Err(e) = feed.reload(&core).await {
                        log("error", "market data could not reload", &e.to_string());
                        return;
                    }
                }
            }
            request = inbox.recv() => match request {
                Some((channel, reply)) => {
                    let _ = reply.send(feed.subscribe(channel)); // the requester may have left
                }
                None => return,
            },
        }
    }
}

/// The publisher's books disagree with an event; it reloads from the core.
#[derive(Debug)]
struct Inconsistent(&'static str);

/// One channel's sequence numbers and subscribers.
struct Stream {
    seq: u64,
    tx: broadcast::Sender<Utf8Bytes>,
}

impl Stream {
    fn new() -> Self {
        Self {
            seq: 0,
            tx: broadcast::channel(CHANNEL_BUFFER).0,
        }
    }

    /// Publish the message `build` makes with the next sequence number; it is built only when
    /// someone is subscribed.
    fn publish(&mut self, build: impl FnOnce(u64) -> Value) {
        self.seq = self.seq.saturating_add(1); // 2^64 messages: unreachable
        if self.tx.receiver_count() > 0 {
            let _ = self.tx.send(Utf8Bytes::from(build(self.seq).to_string())); // no receiver: fine
        }
    }
}

struct Market {
    state: MarketState,
    bids: BTreeMap<u8, u64>,
    asks: BTreeMap<u8, u64>,
    book: Stream,
    trades: Stream,
}

impl Market {
    fn new(state: MarketState) -> Self {
        Self {
            state,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            book: Stream::new(),
            trades: Stream::new(),
        }
    }

    fn side(&mut self, side: Side) -> &mut BTreeMap<u8, u64> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    fn add(&mut self, side: Side, price: u8, qty: u32) -> Result<(), Inconsistent> {
        let level = self.side(side).entry(price).or_insert(0);
        *level = level
            .checked_add(u64::from(qty))
            .ok_or(Inconsistent("level overflow"))?;
        Ok(())
    }

    fn remove(&mut self, side: Side, price: u8, qty: u32) -> Result<(), Inconsistent> {
        let levels = self.side(side);
        let level = levels
            .get_mut(&price)
            .ok_or(Inconsistent("no such level"))?;
        *level = level
            .checked_sub(u64::from(qty))
            .ok_or(Inconsistent("level below zero"))?;
        if *level == 0 {
            levels.remove(&price);
        }
        Ok(())
    }

    fn level(&self, side: Side, price: u8) -> u64 {
        let levels = match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        };
        levels.get(&price).copied().unwrap_or(0)
    }

    fn snapshot(&self, market: MarketId, seq: u64, journal_seq: u64) -> Value {
        snapshot(market, &self.bids, &self.asks, self.state, seq, journal_seq)
    }
}

/// A book snapshot: levels best first (bids descending, asks ascending).
fn snapshot(
    market: MarketId,
    bids: &BTreeMap<u8, u64>,
    asks: &BTreeMap<u8, u64>,
    state: MarketState,
    seq: u64,
    journal_seq: u64,
) -> Value {
    let bids: Vec<Value> = bids.iter().rev().map(|(p, q)| json!([p, q])).collect();
    let asks: Vec<Value> = asks.iter().map(|(p, q)| json!([p, q])).collect();
    json!({
        "type": "snapshot", "channel": Channel::Book(market).name(), "seq": seq, "journal_seq": journal_seq,
        "state": state_name(state), "bids": bids, "asks": asks,
    })
}

struct Resting {
    market: MarketId,
    side: Side,
    price: u8,
    remaining: u32,
    account: AccountId,
}

#[derive(Default)]
struct Feed {
    /// The last journal sequence number reflected.
    as_of: u64,
    markets: BTreeMap<MarketId, Market>,
    orders: HashMap<OrderId, Resting>,
    /// Streams of accounts with a subscriber (dropped once nobody listens).
    accounts: HashMap<AccountId, Stream>,
}

/// Publish to an account's stream if anyone is subscribed to it.
fn to_account(
    accounts: &mut HashMap<AccountId, Stream>,
    account: AccountId,
    journal_seq: u64,
    body: Value,
) {
    let Some(stream) = accounts.get_mut(&account) else {
        return;
    };
    if stream.tx.receiver_count() == 0 {
        accounts.remove(&account);
        return;
    }
    stream.publish(|seq| {
        let mut message = json!({ "channel": "account", "seq": seq, "journal_seq": journal_seq });
        if let (Some(m), Value::Object(fields)) = (message.as_object_mut(), body) {
            m.extend(fields);
        }
        message
    });
}

impl Feed {
    async fn reload(&mut self, core: &CoreHandle) -> Result<(), CoreError> {
        let Answer::Feed {
            as_of,
            markets,
            orders,
        } = core.query(Query::Feed).await?
        else {
            return Err(CoreError::Engine(
                "the feed query answered another shape".to_owned(),
            ));
        };
        self.reset(as_of, &markets, &orders).map_err(|e| {
            CoreError::Engine(format!(
                "the core's own resting orders are inconsistent: {}",
                e.0
            ))
        })
    }

    /// Rebuild every book from the core's answer, keeping the streams (and so the
    /// subscribers), and tell every subscriber where it stands.
    fn reset(
        &mut self,
        as_of: u64,
        markets: &[(MarketId, MarketState)],
        orders: &[(MarketId, Side, RestingOrder)],
    ) -> Result<(), Inconsistent> {
        self.as_of = as_of;
        self.orders.clear();
        for &(id, state) in markets {
            let market = self.markets.entry(id).or_insert_with(|| Market::new(state));
            market.state = state;
            market.bids.clear();
            market.asks.clear();
        }
        for &(id, side, o) in orders {
            let market = self
                .markets
                .get_mut(&id)
                .ok_or(Inconsistent("order in an unknown market"))?;
            market.add(side, o.price.ticks(), o.remaining.contracts())?;
            self.orders.insert(
                o.id,
                Resting {
                    market: id,
                    side,
                    price: o.price.ticks(),
                    remaining: o.remaining.contracts(),
                    account: o.account,
                },
            );
        }
        for (&id, market) in &mut self.markets {
            let (bids, asks, state) = (&market.bids, &market.asks, market.state);
            market
                .book
                .publish(|seq| snapshot(id, bids, asks, state, seq, as_of));
            market.trades.publish(|seq| json!({ "type": "resync", "channel": Channel::Trades(id).name(), "seq": seq, "journal_seq": as_of }));
        }
        let accounts: Vec<AccountId> = self.accounts.keys().copied().collect();
        for account in accounts {
            to_account(
                &mut self.accounts,
                account,
                as_of,
                json!({ "type": "resync" }),
            );
        }
        Ok(())
    }

    fn subscribe(&mut self, channel: Channel) -> Result<Subscription, &'static str> {
        let as_of = self.as_of;
        let (first, rx) = match channel {
            Channel::Book(id) => {
                let market = self.markets.get(&id).ok_or("unknown_market")?;
                (
                    market.snapshot(id, market.book.seq, as_of),
                    market.book.tx.subscribe(),
                )
            }
            Channel::Trades(id) => {
                let market = self.markets.get(&id).ok_or("unknown_market")?;
                let first = json!({ "type": "subscribed", "channel": channel.name(), "seq": market.trades.seq, "journal_seq": as_of });
                (first, market.trades.tx.subscribe())
            }
            Channel::Account(account) => {
                let stream = self.accounts.entry(account).or_insert_with(Stream::new);
                let first = json!({ "type": "subscribed", "channel": "account", "seq": stream.seq, "journal_seq": as_of });
                (first, stream.tx.subscribe())
            }
        };
        Ok(Subscription {
            first: Utf8Bytes::from(first.to_string()),
            rx,
        })
    }

    fn apply_batch(&mut self, batch: &Batch) -> Result<(), Inconsistent> {
        for c in batch.iter() {
            if c.seq.get() > self.as_of {
                self.apply(c)?;
                self.as_of = c.seq.get();
            }
        }
        Ok(())
    }

    /// The account a rejected command came from, and the order it named, if any.
    fn rejected_by(command: &Command) -> Option<(AccountId, &'static str, Option<OrderId>)> {
        match *command {
            Command::Place(n) => Some((n.account, "place", None)),
            Command::Cancel { account, order } => Some((account, "cancel", Some(order))),
            Command::Amend { account, order, .. } => Some((account, "amend", Some(order))),
            _ => None,
        }
    }

    #[allow(clippy::too_many_lines)] // one arm per event kind; splitting it hides the shape
    fn apply(&mut self, c: &Committed) -> Result<(), Inconsistent> {
        let journal_seq = c.seq.get();
        let placed = if let Command::Place(n) = c.command {
            Some(n)
        } else {
            None
        };
        let mut taker: Option<OrderId> = None;
        let mut touched: BTreeMap<MarketId, BTreeSet<(bool, u8)>> = BTreeMap::new();
        let accounts = &mut self.accounts;
        for e in &c.events {
            match *e {
                Event::MarketCreated { market } => {
                    self.markets
                        .entry(market)
                        .or_insert_with(|| Market::new(MarketState::PreOpen));
                    touched.entry(market).or_default();
                }
                Event::MarketStateChanged { market, state } => {
                    self.markets
                        .get_mut(&market)
                        .ok_or(Inconsistent("state of an unknown market"))?
                        .state = state;
                    touched.entry(market).or_default();
                }
                Event::Accepted { order } => {
                    taker = Some(order);
                    if let Some(n) = placed {
                        to_account(
                            accounts,
                            n.account,
                            journal_seq,
                            json!({
                                "type": "order", "event": "accepted", "order": order.get(), "market": n.market.get(),
                                "side": side_name(n.side), "price": n.price, "qty": n.qty, "client_order_id": n.client_order_id,
                            }),
                        );
                    }
                }
                Event::Rested { order, remaining } => {
                    let n = placed
                        .filter(|_| taker == Some(order))
                        .ok_or(Inconsistent("rested without a place"))?;
                    let market = self
                        .markets
                        .get_mut(&n.market)
                        .ok_or(Inconsistent("rested in an unknown market"))?;
                    market.add(n.side, n.price, remaining.contracts())?;
                    self.orders.insert(
                        order,
                        Resting {
                            market: n.market,
                            side: n.side,
                            price: n.price,
                            remaining: remaining.contracts(),
                            account: n.account,
                        },
                    );
                    touched
                        .entry(n.market)
                        .or_default()
                        .insert((n.side == Side::Buy, n.price));
                    to_account(
                        accounts,
                        n.account,
                        journal_seq,
                        json!({ "type": "order", "event": "rested", "order": order.get(), "remaining": remaining.contracts() }),
                    );
                }
                Event::Trade(t) => {
                    let (price, qty) = (t.price.ticks(), t.qty.contracts());
                    let maker_side = t.taker_side.opposite();
                    let maker = self
                        .orders
                        .get_mut(&t.maker)
                        .ok_or(Inconsistent("trade against an unknown order"))?;
                    maker.remaining = maker
                        .remaining
                        .checked_sub(qty)
                        .ok_or(Inconsistent("maker filled beyond its size"))?;
                    if maker.remaining == 0 {
                        self.orders.remove(&t.maker);
                    }
                    let market = self
                        .markets
                        .get_mut(&t.market)
                        .ok_or(Inconsistent("trade in an unknown market"))?;
                    market.remove(maker_side, price, qty)?;
                    touched
                        .entry(t.market)
                        .or_default()
                        .insert((maker_side == Side::Buy, price));
                    market.trades.publish(|seq| {
                        json!({ "type": "trade", "channel": Channel::Trades(t.market).name(), "seq": seq, "journal_seq": journal_seq,
                                "price": price, "qty": qty, "taker_side": side_name(t.taker_side) })
                    });
                    for (account, order, side, liquidity) in [
                        (t.maker_account, t.maker, maker_side, "maker"),
                        (t.taker_account, t.taker, t.taker_side, "taker"),
                    ] {
                        to_account(
                            accounts,
                            account,
                            journal_seq,
                            json!({
                                "type": "fill", "order": order.get(), "market": t.market.get(), "side": side_name(side),
                                "price": price, "qty": qty, "liquidity": liquidity,
                            }),
                        );
                    }
                }
                Event::Cancelled {
                    order,
                    remaining,
                    reason,
                } => {
                    let owner = if let Some(r) = self.orders.remove(&order) {
                        if r.remaining != remaining.contracts() {
                            return Err(Inconsistent("cancelled quantity differs from the book"));
                        }
                        let market = self
                            .markets
                            .get_mut(&r.market)
                            .ok_or(Inconsistent("cancel in an unknown market"))?;
                        market.remove(r.side, r.price, r.remaining)?;
                        touched
                            .entry(r.market)
                            .or_default()
                            .insert((r.side == Side::Buy, r.price));
                        Some(r.account)
                    } else {
                        // The incoming order's unrested remainder (IOC, self-trade prevention).
                        placed.filter(|_| taker == Some(order)).map(|n| n.account)
                    };
                    if let Some(account) = owner {
                        to_account(
                            accounts,
                            account,
                            journal_seq,
                            json!({
                                "type": "order", "event": "cancelled", "order": order.get(), "remaining": remaining.contracts(), "reason": cancel_name(reason),
                            }),
                        );
                    }
                }
                Event::Amended { order, remaining } => {
                    let r = self
                        .orders
                        .get_mut(&order)
                        .ok_or(Inconsistent("amend of an unknown order"))?;
                    let (old, new) = (r.remaining, remaining.contracts());
                    r.remaining = new;
                    let (id, side, price, account) = (r.market, r.side, r.price, r.account);
                    let market = self
                        .markets
                        .get_mut(&id)
                        .ok_or(Inconsistent("amend in an unknown market"))?;
                    market.remove(side, price, old)?;
                    market.add(side, price, new)?;
                    touched
                        .entry(id)
                        .or_default()
                        .insert((side == Side::Buy, price));
                    to_account(
                        accounts,
                        account,
                        journal_seq,
                        json!({ "type": "order", "event": "amended", "order": order.get(), "remaining": new }),
                    );
                }
                Event::Duplicate { original } => {
                    if let Some(n) = placed {
                        to_account(
                            accounts,
                            n.account,
                            journal_seq,
                            json!({ "type": "order", "event": "duplicate", "order": original.get(), "client_order_id": n.client_order_id }),
                        );
                    }
                }
                Event::Rejected { reason } => {
                    if let Some((account, command, order)) = Self::rejected_by(&c.command) {
                        to_account(
                            accounts,
                            account,
                            journal_seq,
                            json!({
                                "type": "order", "event": "rejected", "command": command, "order": order.map(OrderId::get), "reason": reject_name(reason),
                            }),
                        );
                    }
                }
                Event::AccountOpened { .. }
                | Event::Entry(_)
                | Event::Reserved { .. }
                | Event::Settled { .. } => {}
            }
        }
        for (id, levels) in touched {
            let Some(market) = self.markets.get_mut(&id) else {
                continue;
            };
            let changes = |side: Side| -> Vec<Value> {
                levels
                    .iter()
                    .filter(|&&(bid, _)| bid == (side == Side::Buy))
                    .map(|&(_, p)| json!([p, market.level(side, p)]))
                    .collect()
            };
            let (bids, asks) = (changes(Side::Buy), changes(Side::Sell));
            let state = state_name(market.state);
            market.book.publish(|seq| {
                json!({ "type": "delta", "channel": Channel::Book(id).name(), "seq": seq, "journal_seq": journal_seq,
                        "state": state, "bids": bids, "asks": asks })
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_names_round_trip_and_account_needs_one() {
        for name in ["book:7", "trades:0"] {
            assert_eq!(Channel::parse(name, None).unwrap().name(), name);
        }
        assert_eq!(Channel::parse("account", None), Err("unauthorized"));
        assert_eq!(
            Channel::parse("account", Some(AccountId::new(3))),
            Ok(Channel::Account(AccountId::new(3)))
        );
        for bad in ["book", "book:", "book:-1", "book:x", "candles:1", ""] {
            assert_eq!(Channel::parse(bad, None), Err("unknown_channel"), "{bad}");
        }
    }
}
