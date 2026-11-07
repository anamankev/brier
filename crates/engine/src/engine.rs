use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use brier_types::{AccountId, MarketId, OrderId, Price, Quantity, Seq, Side, UnitError};

use crate::book::{Book, Handle};
use crate::{
    CancelReason, Command, Event, MarketState, NewOrder, RejectReason, Sequenced, TimeInForce,
    Trade,
};

/// A failure that halts the engine. Unlike a [`RejectReason`], it means the input stream or
/// the engine's own state is broken, and continuing could corrupt the book (SPEC §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineError {
    /// A command arrived out of sequence.
    SequenceGap {
        /// The sequence number the engine expected.
        expected: Seq,
        /// The sequence number it received.
        got: Seq,
    },
    /// An internal invariant does not hold. The message names it.
    Corrupt(&'static str),
    /// A unit conversion failed where the engine's own invariants say it cannot.
    Unit(UnitError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SequenceGap { expected, got } => write!(
                f,
                "sequence gap: expected {}, got {}",
                expected.get(),
                got.get()
            ),
            Self::Corrupt(what) => write!(f, "engine invariant violated: {what}"),
            Self::Unit(e) => write!(f, "engine unit error: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<UnitError> for EngineError {
    fn from(e: UnitError) -> Self {
        Self::Unit(e)
    }
}

/// A resting order as seen from outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestingOrder {
    /// The order.
    pub id: OrderId,
    /// Its account.
    pub account: AccountId,
    /// Its limit price.
    pub price: Price,
    /// Unfilled contracts.
    pub remaining: Quantity,
}

/// The matching engine for every market (SPEC §3).
pub struct Engine {
    next_seq: Seq,
    markets: BTreeMap<MarketId, Book>,
    /// Lookup only; never iterated, so its order cannot affect output (SPEC §3.2).
    orders: HashMap<OrderId, (MarketId, Handle)>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An engine with no markets, expecting sequence number 1 next.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_seq: Seq::FIRST,
            markets: BTreeMap::new(),
            orders: HashMap::new(),
        }
    }

    /// Apply one command, appending its events to `out` (which the caller may reuse).
    ///
    /// # Errors
    /// [`EngineError::SequenceGap`] if `cmd.seq` is not the next sequence number, or
    /// [`EngineError::Corrupt`] if an internal invariant fails. Either halts the engine.
    pub fn apply(&mut self, cmd: &Sequenced, out: &mut Vec<Event>) -> Result<(), EngineError> {
        if cmd.seq != self.next_seq {
            return Err(EngineError::SequenceGap {
                expected: self.next_seq,
                got: cmd.seq,
            });
        }
        self.next_seq = cmd.seq.next()?;
        match cmd.command {
            Command::CreateMarket { market } => {
                self.create_market(market, out);
                Ok(())
            }
            Command::SetState { market, state } => self.set_state(market, state, out),
            Command::Place(order) => self.place(cmd.seq, order, out),
            Command::Cancel { account, order } => self.cancel(account, order, out),
            Command::Amend {
                account,
                order,
                qty,
            } => self.amend(account, order, qty, out),
        }
    }

    fn create_market(&mut self, market: MarketId, out: &mut Vec<Event>) {
        match self.markets.entry(market) {
            Entry::Occupied(_) => out.push(Event::Rejected {
                reason: RejectReason::MarketExists,
            }),
            Entry::Vacant(slot) => {
                slot.insert(Book::new());
                out.push(Event::MarketCreated { market });
            }
        }
    }

    fn set_state(
        &mut self,
        market: MarketId,
        state: MarketState,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        let Some(book) = self.markets.get_mut(&market) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownMarket,
            });
            return Ok(());
        };
        if !book.state.can_transition_to(state) {
            out.push(Event::Rejected {
                reason: RejectReason::InvalidTransition,
            });
            return Ok(());
        }
        book.state = state;
        out.push(Event::MarketStateChanged { market, state });
        if state == MarketState::Closed {
            for h in book.handles_by_id() {
                let slot = book.remove(h)?;
                self.orders.remove(&slot.id);
                out.push(Event::Cancelled {
                    order: slot.id,
                    remaining: Quantity::new(slot.remaining)?,
                    reason: CancelReason::MarketClosed,
                });
            }
        }
        Ok(())
    }

    fn place(&mut self, seq: Seq, o: NewOrder, out: &mut Vec<Event>) -> Result<(), EngineError> {
        let Self {
            markets, orders, ..
        } = self;
        let Some(book) = markets.get_mut(&o.market) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownMarket,
            });
            return Ok(());
        };
        let (price, qty) = match validate_place(book, &o)? {
            Ok(valid) => valid,
            Err(reason) => {
                out.push(Event::Rejected { reason });
                return Ok(());
            }
        };

        let id = OrderId::new(seq.get());
        out.push(Event::Accepted { order: id });
        let maker_side = o.side.opposite();
        let mut remaining = qty.contracts();
        while remaining > 0 {
            let Some(best) = book.best(maker_side) else {
                break;
            };
            if !book.crosses(o.side, price) {
                break;
            }
            let h = book
                .front(maker_side, best)?
                .ok_or(EngineError::Corrupt("best level is empty"))?;
            let maker = *book.slot(h)?;
            if maker.account == o.account {
                // Self-trade prevention, CancelTaker (SPEC §3.1): trades so far stand.
                out.push(Event::Cancelled {
                    order: id,
                    remaining: Quantity::new(remaining)?,
                    reason: CancelReason::SelfTrade,
                });
                return Ok(());
            }
            let fill = remaining.min(maker.remaining);
            out.push(Event::Trade(Trade {
                market: o.market,
                maker: maker.id,
                taker: id,
                maker_account: maker.account,
                taker_account: o.account,
                taker_side: o.side,
                price: best,
                qty: Quantity::new(fill)?,
            }));
            remaining = remaining
                .checked_sub(fill)
                .ok_or(EngineError::Corrupt("taker fill exceeds remaining"))?;
            let maker_left = maker
                .remaining
                .checked_sub(fill)
                .ok_or(EngineError::Corrupt("maker fill exceeds remaining"))?;
            if maker_left == 0 {
                book.remove(h)?;
                orders.remove(&maker.id);
            } else {
                book.slot_mut(h)?.remaining = maker_left;
            }
        }
        if remaining == 0 {
            return Ok(());
        }
        match o.tif {
            TimeInForce::Gtc => {
                let h = book.insert(id, o.account, o.side, price, remaining)?;
                orders.insert(id, (o.market, h));
                out.push(Event::Rested {
                    order: id,
                    remaining: Quantity::new(remaining)?,
                });
            }
            TimeInForce::Ioc => out.push(Event::Cancelled {
                order: id,
                remaining: Quantity::new(remaining)?,
                reason: CancelReason::IocRemainder,
            }),
            TimeInForce::Fok => {
                return Err(EngineError::Corrupt(
                    "fill-or-kill passed its check but did not fill",
                ));
            }
        }
        Ok(())
    }

    /// The account's live order, or `None` if it does not exist or belongs to someone else.
    fn own_order(&self, account: AccountId, order: OrderId) -> Option<(MarketId, Handle)> {
        let &(market, h) = self.orders.get(&order)?;
        let owner = self.markets.get(&market)?.slot(h).ok()?.account;
        (owner == account).then_some((market, h))
    }

    fn cancel(
        &mut self,
        account: AccountId,
        order: OrderId,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        let Some((market, h)) = self.own_order(account, order) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownOrder,
            });
            return Ok(());
        };
        let book = self
            .markets
            .get_mut(&market)
            .ok_or(EngineError::Corrupt("order map points to a missing market"))?;
        let slot = book.remove(h)?;
        self.orders.remove(&order);
        out.push(Event::Cancelled {
            order,
            remaining: Quantity::new(slot.remaining)?,
            reason: CancelReason::User,
        });
        Ok(())
    }

    fn amend(
        &mut self,
        account: AccountId,
        order: OrderId,
        qty: u32,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        let Some((market, h)) = self.own_order(account, order) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownOrder,
            });
            return Ok(());
        };
        let book = self
            .markets
            .get_mut(&market)
            .ok_or(EngineError::Corrupt("order map points to a missing market"))?;
        if book.state != MarketState::Open {
            out.push(Event::Rejected {
                reason: RejectReason::MarketNotOpen,
            });
            return Ok(());
        }
        let Ok(new_qty) = Quantity::new(qty) else {
            out.push(Event::Rejected {
                reason: RejectReason::InvalidQuantity,
            });
            return Ok(());
        };
        let slot = book.slot_mut(h)?;
        if new_qty.contracts() >= slot.remaining {
            out.push(Event::Rejected {
                reason: RejectReason::InvalidAmend,
            });
            return Ok(());
        }
        slot.remaining = new_qty.contracts();
        out.push(Event::Amended {
            order,
            remaining: new_qty,
        });
        Ok(())
    }

    /// The market's state, if it exists.
    #[must_use]
    pub fn market_state(&self, market: MarketId) -> Option<MarketState> {
        self.markets.get(&market).map(|b| b.state)
    }

    /// Every market ID, ascending.
    pub fn markets(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.markets.keys().copied()
    }

    /// Resting orders on one side of a market in time priority (best price first, FIFO).
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] if the book's links are broken.
    pub fn resting(&self, market: MarketId, side: Side) -> Result<Vec<RestingOrder>, EngineError> {
        let Some(book) = self.markets.get(&market) else {
            return Ok(Vec::new());
        };
        book.resting(side)?
            .into_iter()
            .map(|s| {
                Ok(RestingOrder {
                    id: s.id,
                    account: s.account,
                    price: s.price,
                    remaining: Quantity::new(s.remaining)?,
                })
            })
            .collect()
    }

    /// Check every invariant the engine maintains: each book's structure (I5) and the
    /// agreement of the order-ID map with the books.
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] naming the first invariant that fails.
    pub fn check_invariants(&self) -> Result<(), EngineError> {
        let mut total: usize = 0;
        for book in self.markets.values() {
            book.check()?;
            total = total
                .checked_add(book.live())
                .ok_or(EngineError::Corrupt("live order count overflow"))?;
        }
        if total != self.orders.len() {
            return Err(EngineError::Corrupt(
                "order map size disagrees with the books",
            ));
        }
        // Checked in ascending order ID so the first failure reported is deterministic.
        let mut entries: Vec<_> = self.orders.iter().collect();
        entries.sort_unstable_by_key(|&(&id, _)| id);
        for (&id, &(market, h)) in entries {
            let book = self
                .markets
                .get(&market)
                .ok_or(EngineError::Corrupt("order map points to a missing market"))?;
            if book.slot(h)?.id != id {
                return Err(EngineError::Corrupt("order map points to the wrong order"));
            }
        }
        Ok(())
    }

    /// BLAKE3 hash of the complete engine state in a canonical byte order: the next sequence
    /// number, then each market in ascending ID with its state and both sides in time
    /// priority. Replay must reproduce it exactly (SPEC §3.2).
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] if a book's links are broken.
    pub fn state_hash(&self) -> Result<[u8; 32], EngineError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"brier-engine-state-v1");
        hasher.update(&self.next_seq.get().to_le_bytes());
        hasher.update(&(self.markets.len() as u64).to_le_bytes());
        for (market, book) in &self.markets {
            hasher.update(&market.get().to_le_bytes());
            hasher.update(&[book.state.code()]);
            for side in [Side::Buy, Side::Sell] {
                let resting = book.resting(side)?;
                hasher.update(&(resting.len() as u64).to_le_bytes());
                for s in resting {
                    hasher.update(&[s.price.ticks()]);
                    hasher.update(&s.id.get().to_le_bytes());
                    hasher.update(&s.account.get().to_le_bytes());
                    hasher.update(&s.remaining.to_le_bytes());
                }
            }
        }
        Ok(*hasher.finalize().as_bytes())
    }
}

/// The place-order checks in their fixed order (docs/gates/G1-plan.md), after the market
/// lookup: the first failing check names the rejection.
fn validate_place(
    book: &Book,
    o: &NewOrder,
) -> Result<Result<(Price, Quantity), RejectReason>, EngineError> {
    if book.state != MarketState::Open {
        return Ok(Err(RejectReason::MarketNotOpen));
    }
    let Ok(price) = Price::new(o.price) else {
        return Ok(Err(RejectReason::InvalidPrice));
    };
    let Ok(qty) = Quantity::new(o.qty) else {
        return Ok(Err(RejectReason::InvalidQuantity));
    };
    if o.post_only && o.tif != TimeInForce::Gtc {
        return Ok(Err(RejectReason::InvalidOrder));
    }
    if o.post_only && book.crosses(o.side, price) {
        return Ok(Err(RejectReason::PostOnlyWouldCross));
    }
    if o.tif == TimeInForce::Fok
        && fillable(book, o.side, price, o.account, qty.contracts())? < qty.contracts()
    {
        return Ok(Err(RejectReason::FokUnfillable));
    }
    Ok(Ok((price, qty)))
}

/// How much of an order on `side` up to `limit` would fill now, stopping at `need`, at the
/// limit price, or at the first resting order owned by `account` (where self-trade
/// prevention would stop matching). Used by fill-or-kill before any trade happens.
fn fillable(
    book: &Book,
    side: Side,
    limit: Price,
    account: AccountId,
    need: u32,
) -> Result<u32, EngineError> {
    let maker_side = side.opposite();
    let mut sum: u32 = 0;
    for maker in book.resting(maker_side)? {
        let crosses = match side {
            Side::Buy => maker.price <= limit,
            Side::Sell => maker.price >= limit,
        };
        if !crosses || maker.account == account {
            break;
        }
        sum = sum.saturating_add(maker.remaining);
        if sum >= need {
            break;
        }
    }
    Ok(sum)
}
