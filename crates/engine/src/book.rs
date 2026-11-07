//! One market's order book (SPEC §3.3).
//!
//! Prices are 1-99 ticks, so each side is a fixed array of 100 levels indexed directly by
//! price (index 0 is never used). Each level is a FIFO queue threaded through an order arena
//! as a doubly linked list, so inserting at the tail, removing any order (cancel), and taking
//! the front (match) are all O(1). Finding the next best level after one empties scans at
//! most 98 levels, a small constant. ADR-0003 records this choice against a tree of levels.

use brier_types::{AccountId, MAX_PRICE_TICKS, MIN_PRICE_TICKS, OrderId, Price, Side};

use crate::{EngineError, MarketState};

/// Marks "no order" in level and arena links.
const NIL: u32 = u32::MAX;
/// Level array length: index = price in ticks, so 0..=99.
const LEVEL_SLOTS: usize = 100;

/// A resting order in the arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Slot {
    pub(crate) id: OrderId,
    pub(crate) account: AccountId,
    pub(crate) side: Side,
    pub(crate) price: Price,
    /// Unfilled contracts; always > 0 while the order rests (invariant I5).
    pub(crate) remaining: u32,
    prev: u32,
    next: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Level {
    head: u32,
    tail: u32,
}

impl Level {
    const EMPTY: Self = Self {
        head: NIL,
        tail: NIL,
    };
}

/// Arena index of a resting order.
pub(crate) type Handle = u32;

const fn ix(h: Handle) -> usize {
    h as usize
}

pub(crate) struct Book {
    pub(crate) state: MarketState,
    bids: [Level; LEVEL_SLOTS],
    asks: [Level; LEVEL_SLOTS],
    best_bid: Option<Price>,
    best_ask: Option<Price>,
    slots: Vec<Option<Slot>>,
    free: Vec<Handle>,
    live: usize,
}

impl Book {
    pub(crate) fn new() -> Self {
        Self {
            state: MarketState::PreOpen,
            bids: [Level::EMPTY; LEVEL_SLOTS],
            asks: [Level::EMPTY; LEVEL_SLOTS],
            best_bid: None,
            best_ask: None,
            slots: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }

    pub(crate) const fn best(&self, side: Side) -> Option<Price> {
        match side {
            Side::Buy => self.best_bid,
            Side::Sell => self.best_ask,
        }
    }

    /// Whether an incoming order on `side` at `price` would trade against the book now.
    pub(crate) fn crosses(&self, side: Side, price: Price) -> bool {
        match side {
            Side::Buy => self.best_ask.is_some_and(|ask| ask <= price),
            Side::Sell => self.best_bid.is_some_and(|bid| bid >= price),
        }
    }

    const fn levels(&self, side: Side) -> &[Level; LEVEL_SLOTS] {
        match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        }
    }

    fn level(&self, side: Side, price: Price) -> Result<Level, EngineError> {
        self.levels(side)
            .get(usize::from(price.ticks()))
            .copied()
            .ok_or(EngineError::Corrupt("price level index out of range"))
    }

    fn level_mut(&mut self, side: Side, price: Price) -> Result<&mut Level, EngineError> {
        let levels = match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        };
        levels
            .get_mut(usize::from(price.ticks()))
            .ok_or(EngineError::Corrupt("price level index out of range"))
    }

    pub(crate) fn slot(&self, h: Handle) -> Result<&Slot, EngineError> {
        self.slots
            .get(ix(h))
            .and_then(Option::as_ref)
            .ok_or(EngineError::Corrupt(
                "handle does not refer to a resting order",
            ))
    }

    pub(crate) fn slot_mut(&mut self, h: Handle) -> Result<&mut Slot, EngineError> {
        self.slots
            .get_mut(ix(h))
            .and_then(Option::as_mut)
            .ok_or(EngineError::Corrupt(
                "handle does not refer to a resting order",
            ))
    }

    /// The order first in time priority at `price` on `side`, if any.
    pub(crate) fn front(&self, side: Side, price: Price) -> Result<Option<Handle>, EngineError> {
        let level = self.level(side, price)?;
        Ok((level.head != NIL).then_some(level.head))
    }

    /// The order after `h` in its level's queue, if any.
    pub(crate) fn next_in_level(&self, h: Handle) -> Result<Option<Handle>, EngineError> {
        let next = self.slot(h)?.next;
        Ok((next != NIL).then_some(next))
    }

    /// Add an order at the back of its price level's queue.
    pub(crate) fn insert(
        &mut self,
        id: OrderId,
        account: AccountId,
        side: Side,
        price: Price,
        remaining: u32,
    ) -> Result<Handle, EngineError> {
        if remaining == 0 {
            return Err(EngineError::Corrupt(
                "resting an order with nothing remaining",
            ));
        }
        let tail = self.level(side, price)?.tail;
        let slot = Slot {
            id,
            account,
            side,
            price,
            remaining,
            prev: tail,
            next: NIL,
        };
        let h = if let Some(h) = self.free.pop() {
            let cell = self
                .slots
                .get_mut(ix(h))
                .ok_or(EngineError::Corrupt("free list handle out of range"))?;
            if cell.is_some() {
                return Err(EngineError::Corrupt("free list handle is occupied"));
            }
            *cell = Some(slot);
            h
        } else {
            let h = Handle::try_from(self.slots.len())
                .ok()
                .filter(|&h| h != NIL)
                .ok_or(EngineError::Corrupt("order arena is full"))?;
            self.slots.push(Some(slot));
            h
        };
        if tail == NIL {
            self.level_mut(side, price)?.head = h;
        } else {
            self.slot_mut(tail)?.next = h;
        }
        self.level_mut(side, price)?.tail = h;
        match side {
            Side::Buy => self.best_bid = Some(self.best_bid.map_or(price, |b| b.max(price))),
            Side::Sell => self.best_ask = Some(self.best_ask.map_or(price, |a| a.min(price))),
        }
        self.live = self
            .live
            .checked_add(1)
            .ok_or(EngineError::Corrupt("live order count overflow"))?;
        Ok(h)
    }

    /// Remove any resting order, wherever it sits in its queue.
    pub(crate) fn remove(&mut self, h: Handle) -> Result<Slot, EngineError> {
        let slot = self
            .slots
            .get_mut(ix(h))
            .and_then(Option::take)
            .ok_or(EngineError::Corrupt(
                "removing an order that is not resting",
            ))?;
        if slot.prev == NIL {
            self.level_mut(slot.side, slot.price)?.head = slot.next;
        } else {
            self.slot_mut(slot.prev)?.next = slot.next;
        }
        if slot.next == NIL {
            self.level_mut(slot.side, slot.price)?.tail = slot.prev;
        } else {
            self.slot_mut(slot.next)?.prev = slot.prev;
        }
        if self.level(slot.side, slot.price)?.head == NIL
            && self.best(slot.side) == Some(slot.price)
        {
            self.recompute_best(slot.side, slot.price)?;
        }
        self.free.push(h);
        self.live = self
            .live
            .checked_sub(1)
            .ok_or(EngineError::Corrupt("live order count underflow"))?;
        Ok(slot)
    }

    /// After the best level on `side` (at `from`) empties, find the next non-empty level.
    fn recompute_best(&mut self, side: Side, from: Price) -> Result<(), EngineError> {
        let mut best = None;
        match side {
            Side::Buy => {
                for t in (MIN_PRICE_TICKS..=from.ticks()).rev() {
                    let p = Price::new(t)?;
                    if self.level(side, p)?.head != NIL {
                        best = Some(p);
                        break;
                    }
                }
                self.best_bid = best;
            }
            Side::Sell => {
                for t in from.ticks()..=MAX_PRICE_TICKS {
                    let p = Price::new(t)?;
                    if self.level(side, p)?.head != NIL {
                        best = Some(p);
                        break;
                    }
                }
                self.best_ask = best;
            }
        }
        Ok(())
    }

    /// Prices on `side` from best to worst: bids high to low, asks low to high.
    fn prices_best_first(side: Side) -> impl Iterator<Item = u8> {
        let ascending = MIN_PRICE_TICKS..=MAX_PRICE_TICKS;
        let (up, down) = match side {
            Side::Buy => (None, Some(ascending.rev())),
            Side::Sell => (Some(ascending), None),
        };
        up.into_iter().flatten().chain(down.into_iter().flatten())
    }

    /// Every resting order on `side` in time priority: best price first, FIFO within a level.
    pub(crate) fn resting(&self, side: Side) -> Result<Vec<Slot>, EngineError> {
        let mut out = Vec::new();
        for t in Self::prices_best_first(side) {
            let mut cursor = self.front(side, Price::new(t)?)?;
            while let Some(h) = cursor {
                out.push(*self.slot(h)?);
                cursor = self.next_in_level(h)?;
            }
        }
        Ok(out)
    }

    /// Handles of every resting order, in ascending order ID (the order closing cancels in).
    pub(crate) fn handles_by_id(&self) -> Vec<Handle> {
        let mut found: Vec<(OrderId, Handle)> = self
            .slots
            .iter()
            .zip(0..)
            .filter_map(|(cell, h)| cell.map(|s| (s.id, h)))
            .collect();
        found.sort_unstable_by_key(|&(id, _)| id);
        found.into_iter().map(|(_, h)| h).collect()
    }

    pub(crate) const fn live(&self) -> usize {
        self.live
    }

    /// Check the book's structural invariants (I5, SPEC §4.3): every level's links are
    /// consistent, every resting order sits at its own side and price with quantity > 0, the
    /// counts agree, the best prices are right, and the book is not crossed.
    pub(crate) fn check(&self) -> Result<(), EngineError> {
        let mut counted: usize = 0;
        for side in [Side::Buy, Side::Sell] {
            if self.levels(side).first() != Some(&Level::EMPTY) {
                return Err(EngineError::Corrupt("level 0 is in use"));
            }
            let mut best_seen = None;
            for t in Self::prices_best_first(side) {
                let price = Price::new(t)?;
                let level = self.level(side, price)?;
                let mut prev = NIL;
                let mut cursor = level.head;
                while cursor != NIL {
                    let slot = self.slot(cursor)?;
                    if slot.side != side || slot.price != price {
                        return Err(EngineError::Corrupt(
                            "order sits at the wrong side or price",
                        ));
                    }
                    if slot.remaining == 0 {
                        return Err(EngineError::Corrupt("resting order with zero quantity"));
                    }
                    if slot.prev != prev {
                        return Err(EngineError::Corrupt("broken backward link"));
                    }
                    counted = counted
                        .checked_add(1)
                        .filter(|&c| c <= self.slots.len())
                        .ok_or(EngineError::Corrupt("cycle in a level queue"))?;
                    prev = cursor;
                    cursor = slot.next;
                }
                if level.tail != prev {
                    return Err(EngineError::Corrupt("level tail is not the last order"));
                }
                if best_seen.is_none() && level.head != NIL {
                    best_seen = Some(price);
                }
            }
            if self.best(side) != best_seen {
                return Err(EngineError::Corrupt("best price is stale"));
            }
        }
        let occupied = self.slots.iter().filter(|cell| cell.is_some()).count();
        if counted != self.live || occupied != self.live {
            return Err(EngineError::Corrupt(
                "live order count disagrees with the book",
            ));
        }
        if self.free.len().checked_add(self.live) != Some(self.slots.len()) {
            return Err(EngineError::Corrupt(
                "free list and live orders do not cover the arena",
            ));
        }
        if let (Some(bid), Some(ask)) = (self.best_bid, self.best_ask)
            && bid >= ask
        {
            return Err(EngineError::Corrupt("book is crossed"));
        }
        Ok(())
    }
}
