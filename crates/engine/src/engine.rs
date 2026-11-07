use std::collections::btree_map::Entry as MapEntry;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

use brier_types::{
    AccountId, Credits, MarketId, OrderId, Outcome, Price, Quantity, Seq, Side, UnitError,
};

use crate::book::{Book, Handle};
use crate::ledger::{Ledger, MAX_RESTING_ORDERS};
use crate::{
    CancelReason, Command, Event, MarketState, NewOrder, RejectReason, Sequenced, TimeInForce,
    Trade,
};

/// A failure that halts the engine. Unlike a [`RejectReason`], it means the input stream or
/// the engine's own state is broken, and continuing could corrupt the book or the ledger
/// (SPEC §4.3).
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
    /// The engine halted earlier, at command `at`, and accepts nothing more. The original
    /// failure is available from [`Engine::halted`]. Recovery is a restart from the journal
    /// (G3), never a retry on the same in-memory state.
    Halted {
        /// The sequence number of the command that halted the engine.
        at: Seq,
    },
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
            Self::Halted { at } => write!(f, "engine halted at command {}", at.get()),
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

/// An account's balances and positions as seen from outside the engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    /// The account.
    pub account: AccountId,
    /// Ledger balance.
    pub cash: Credits,
    /// Collateral locked by resting orders.
    pub reserved: Credits,
    /// Nonzero positions by ascending market: > 0 YES, < 0 NO.
    pub positions: Vec<(MarketId, i64)>,
}

/// The matching engine and ledger for every market (SPEC §3, §4).
pub struct Engine {
    next_seq: Seq,
    markets: BTreeMap<MarketId, Book>,
    /// Lookup only; never iterated without sorting, so its order cannot affect output.
    orders: HashMap<OrderId, (MarketId, Handle)>,
    ledger: Ledger,
    /// Set by the first failed command; every later command is refused.
    halted: Option<(Seq, EngineError)>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// An engine with no markets or accounts, expecting sequence number 1 next.
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_seq: Seq::FIRST,
            markets: BTreeMap::new(),
            orders: HashMap::new(),
            ledger: Ledger::default(),
            halted: None,
        }
    }

    /// The command that halted the engine and why, if it has halted.
    #[must_use]
    pub const fn halted(&self) -> Option<(Seq, EngineError)> {
        self.halted
    }

    /// Apply one command, appending its events to `out` (which the caller may reuse).
    ///
    /// # Errors
    /// [`EngineError::SequenceGap`] if `cmd.seq` is not the next sequence number, or
    /// [`EngineError::Corrupt`] if an internal invariant fails. Either halts the engine for
    /// good: a command that fails may have changed state part-way, so every later call
    /// returns [`EngineError::Halted`] (SPEC §4.3).
    pub fn apply(&mut self, cmd: &Sequenced, out: &mut Vec<Event>) -> Result<(), EngineError> {
        if let Some((at, _)) = self.halted {
            return Err(EngineError::Halted { at });
        }
        let result = self.apply_unchecked(cmd, out);
        if let Err(e) = result {
            self.halted = Some((cmd.seq, e));
        }
        result
    }

    fn apply_unchecked(
        &mut self,
        cmd: &Sequenced,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        if cmd.seq != self.next_seq {
            return Err(EngineError::SequenceGap {
                expected: self.next_seq,
                got: cmd.seq,
            });
        }
        self.next_seq = cmd.seq.next()?;
        match cmd.command {
            Command::CreateMarket { market } => self.create_market(market, out),
            Command::SetState { market, state } => self.set_state(market, state, out)?,
            Command::Place(order) => self.place(cmd.seq, order, out)?,
            Command::Cancel { account, order } => self.cancel(account, order, out)?,
            Command::Amend {
                account,
                order,
                qty,
            } => self.amend(account, order, qty, out)?,
            Command::OpenAccount { account } => self.open_account(account, out)?,
            Command::Settle { market, outcome } => self.settle(market, outcome, out)?,
            Command::CancelAll { account, market } => self.cancel_all(account, market, out)?,
            Command::HaltAll => self.halt_all(out),
        }
        self.ledger.finish_command(out)
    }

    fn create_market(&mut self, market: MarketId, out: &mut Vec<Event>) {
        match self.markets.entry(market) {
            MapEntry::Occupied(_) => out.push(Event::Rejected {
                reason: RejectReason::MarketExists,
            }),
            MapEntry::Vacant(slot) => {
                slot.insert(Book::new());
                self.ledger.add_market(market);
                out.push(Event::MarketCreated { market });
            }
        }
    }

    fn open_account(
        &mut self,
        account: AccountId,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        match self.ledger.open_account(account)? {
            Some(entry) => {
                out.push(Event::AccountOpened {
                    account,
                    cash: crate::SIGNUP_GRANT,
                });
                out.push(Event::Entry(entry));
            }
            None => out.push(Event::Rejected {
                reason: RejectReason::AccountExists,
            }),
        }
        Ok(())
    }

    /// Remove a resting order from its book, the order map, and the ledger's resting totals.
    fn remove_resting(
        &mut self,
        market: MarketId,
        h: Handle,
    ) -> Result<crate::book::Slot, EngineError> {
        let book = self
            .markets
            .get_mut(&market)
            .ok_or(EngineError::Corrupt("order in a missing market"))?;
        let slot = book.remove(h)?;
        self.orders.remove(&slot.id);
        self.ledger.shrink(
            slot.account,
            market,
            slot.side,
            slot.price,
            slot.remaining,
            true,
        )?;
        Ok(slot)
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
                let slot = self.remove_resting(market, h)?;
                out.push(Event::Cancelled {
                    order: slot.id,
                    remaining: Quantity::new(slot.remaining)?,
                    reason: CancelReason::MarketClosed,
                });
            }
        }
        Ok(())
    }

    fn settle(
        &mut self,
        market: MarketId,
        outcome: Outcome,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        let Some(book) = self.markets.get_mut(&market) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownMarket,
            });
            return Ok(());
        };
        if book.state != MarketState::Closed {
            out.push(Event::Rejected {
                reason: RejectReason::InvalidTransition,
            });
            return Ok(());
        }
        book.state = match outcome {
            Outcome::Yes | Outcome::No => MarketState::Resolved,
            Outcome::Void => MarketState::Voided,
        };
        out.push(Event::Settled { market, outcome });
        self.ledger.settle(market, outcome, out)
    }

    fn cancel_all(
        &mut self,
        account: Option<AccountId>,
        market: Option<MarketId>,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        if account.is_some_and(|a| !self.ledger.has_account(a)) {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownAccount,
            });
            return Ok(());
        }
        if market.is_some_and(|m| !self.markets.contains_key(&m)) {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownMarket,
            });
            return Ok(());
        }
        let mut targets: Vec<(OrderId, MarketId, Handle)> = Vec::new();
        for (&m, book) in &self.markets {
            if market.is_some_and(|only| only != m) {
                continue;
            }
            for h in book.handles_by_id() {
                let slot = book.slot(h)?;
                if account.is_none_or(|a| a == slot.account) {
                    targets.push((slot.id, m, h));
                }
            }
        }
        targets.sort_unstable_by_key(|&(id, _, _)| id);
        for (_, m, h) in targets {
            let slot = self.remove_resting(m, h)?;
            out.push(Event::Cancelled {
                order: slot.id,
                remaining: Quantity::new(slot.remaining)?,
                reason: CancelReason::KillSwitch,
            });
        }
        Ok(())
    }

    fn halt_all(&mut self, out: &mut Vec<Event>) {
        for (&market, book) in &mut self.markets {
            if book.state == MarketState::Open {
                book.state = MarketState::Halted;
                out.push(Event::MarketStateChanged {
                    market,
                    state: MarketState::Halted,
                });
            }
        }
    }

    fn place(&mut self, seq: Seq, o: NewOrder, out: &mut Vec<Event>) -> Result<(), EngineError> {
        let Some(book) = self.markets.get(&o.market) else {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownMarket,
            });
            return Ok(());
        };
        if !self.ledger.has_account(o.account) {
            out.push(Event::Rejected {
                reason: RejectReason::UnknownAccount,
            });
            return Ok(());
        }
        let (price, qty) = match validate_place(book, &self.ledger, &o)? {
            Ok(valid) => valid,
            Err(reason) => {
                out.push(Event::Rejected { reason });
                return Ok(());
            }
        };
        let id = OrderId::new(seq.get());
        out.push(Event::Accepted { order: id });
        let Some(remaining) = self.match_incoming(id, &o, price, qty.contracts(), out)? else {
            return Ok(()); // self-trade prevention cancelled the remainder
        };
        if remaining == 0 {
            return Ok(());
        }
        match o.tif {
            TimeInForce::Gtc => {
                let book = self
                    .markets
                    .get_mut(&o.market)
                    .ok_or(EngineError::Corrupt("market vanished"))?;
                let h = book.insert(id, o.account, o.side, price, remaining)?;
                self.orders.insert(id, (o.market, h));
                self.ledger
                    .rest(o.account, o.market, o.side, price, remaining)?;
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

    /// Match an incoming order against the book. Returns the unfilled quantity, or `None` if
    /// self-trade prevention cancelled the remainder (its event is already emitted).
    fn match_incoming(
        &mut self,
        id: OrderId,
        o: &NewOrder,
        price: Price,
        qty: u32,
        out: &mut Vec<Event>,
    ) -> Result<Option<u32>, EngineError> {
        let Self {
            markets,
            orders,
            ledger,
            ..
        } = self;
        let book = markets
            .get_mut(&o.market)
            .ok_or(EngineError::Corrupt("market vanished"))?;
        let maker_side = o.side.opposite();
        let mut remaining = qty;
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
                return Ok(None);
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
            let (buyer, seller) = match o.side {
                Side::Buy => (o.account, maker.account),
                Side::Sell => (maker.account, o.account),
            };
            out.push(Event::Entry(
                ledger.trade(o.market, buyer, seller, best, fill)?,
            ));
            remaining = remaining
                .checked_sub(fill)
                .ok_or(EngineError::Corrupt("taker fill exceeds remaining"))?;
            let maker_left = maker
                .remaining
                .checked_sub(fill)
                .ok_or(EngineError::Corrupt("maker fill exceeds remaining"))?;
            ledger.shrink(
                maker.account,
                o.market,
                maker_side,
                best,
                fill,
                maker_left == 0,
            )?;
            if maker_left == 0 {
                book.remove(h)?;
                orders.remove(&maker.id);
            } else {
                book.slot_mut(h)?.remaining = maker_left;
            }
        }
        Ok(Some(remaining))
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
        let slot = self.remove_resting(market, h)?;
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
        let by = slot
            .remaining
            .checked_sub(new_qty.contracts())
            .ok_or(EngineError::Corrupt("amend size"))?;
        slot.remaining = new_qty.contracts();
        let (side, price) = (slot.side, slot.price);
        self.ledger
            .shrink(account, market, side, price, by, false)?;
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

    /// A market's escrow, if it exists.
    #[must_use]
    pub fn escrow(&self, market: MarketId) -> Option<Credits> {
        self.ledger.escrow.get(&market).copied()
    }

    /// Every account with its balances and nonzero positions, ascending by account.
    #[must_use]
    pub fn accounts(&self) -> Vec<AccountView> {
        let mut views: BTreeMap<AccountId, AccountView> = self
            .ledger
            .accounts
            .iter()
            .map(|(&account, a)| {
                (
                    account,
                    AccountView {
                        account,
                        cash: a.cash,
                        reserved: a.reserved,
                        positions: Vec::new(),
                    },
                )
            })
            .collect();
        for (&(m, a), h) in &self.ledger.holdings {
            if h.position != 0
                && let Some(v) = views.get_mut(&a)
            {
                v.positions.push((m, h.position));
            }
        }
        views.into_values().collect()
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

    /// Check every invariant the engine maintains: each book's structure (I5), the order map,
    /// the ledger (I1-I4), and the agreement of the ledger's resting totals with the books.
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
        self.ledger.check()?;
        self.check_resting_totals()
    }

    /// The ledger's per-price resting quantities and per-account resting counts must equal
    /// what the books hold (I4's inputs).
    fn check_resting_totals(&self) -> Result<(), EngineError> {
        let mut qty: BTreeMap<(MarketId, AccountId, bool, u8), u64> = BTreeMap::new();
        let mut counts: BTreeMap<AccountId, u32> = BTreeMap::new();
        for (&m, book) in &self.markets {
            for side in [Side::Buy, Side::Sell] {
                for s in book.resting(side)? {
                    let cell = qty
                        .entry((m, s.account, side == Side::Buy, s.price.ticks()))
                        .or_insert(0);
                    *cell = cell
                        .checked_add(u64::from(s.remaining))
                        .ok_or(EngineError::Corrupt("count overflow"))?;
                    let c = counts.entry(s.account).or_insert(0);
                    *c = c
                        .checked_add(1)
                        .ok_or(EngineError::Corrupt("count overflow"))?;
                }
            }
        }
        for (&(m, a), h) in &self.ledger.holdings {
            for (is_buy, levels) in [(true, &h.buy_qty), (false, &h.sell_qty)] {
                for (t, &n) in levels.iter().enumerate() {
                    let t = u8::try_from(t).map_err(|_| EngineError::Corrupt("price index"))?;
                    if u64::from(n) != qty.get(&(m, a, is_buy, t)).copied().unwrap_or(0) {
                        return Err(EngineError::Corrupt(
                            "I4: ledger resting quantity disagrees with the book",
                        ));
                    }
                }
            }
        }
        let tracked: u64 = qty.values().sum();
        let in_ledger: u64 = self
            .ledger
            .holdings
            .values()
            .flat_map(|h| h.buy_qty.iter().chain(h.sell_qty.iter()))
            .map(|&n| u64::from(n))
            .sum();
        if tracked != in_ledger {
            return Err(EngineError::Corrupt(
                "I4: resting quantity missing from the ledger",
            ));
        }
        for (a, acct) in &self.ledger.accounts {
            if counts.get(a).copied().unwrap_or(0) != acct.resting {
                return Err(EngineError::Corrupt(
                    "resting order count disagrees with the books",
                ));
            }
        }
        Ok(())
    }

    /// BLAKE3 hash of the complete engine state in a canonical byte order: the next sequence
    /// number; each market in ascending ID with its state, escrow, and both sides in time
    /// priority; then every account and every holding in ascending order. Replay must
    /// reproduce it exactly (SPEC §3.2).
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] if a book's links are broken.
    pub fn state_hash(&self) -> Result<[u8; 32], EngineError> {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"brier-engine-state-v2");
        hasher.update(&self.next_seq.get().to_le_bytes());
        hasher.update(&(self.markets.len() as u64).to_le_bytes());
        for (market, book) in &self.markets {
            hasher.update(&market.get().to_le_bytes());
            hasher.update(&[book.state.code()]);
            hasher.update(
                &self
                    .ledger
                    .escrow
                    .get(market)
                    .copied()
                    .unwrap_or(Credits::ZERO)
                    .cents()
                    .to_le_bytes(),
            );
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
        hasher.update(&self.ledger.issuance.cents().to_le_bytes());
        hasher.update(&(self.ledger.accounts.len() as u64).to_le_bytes());
        for (a, acct) in &self.ledger.accounts {
            hasher.update(&a.get().to_le_bytes());
            hasher.update(&acct.cash.cents().to_le_bytes());
            hasher.update(&acct.reserved.cents().to_le_bytes());
            hasher.update(&acct.resting.to_le_bytes());
        }
        hasher.update(&(self.ledger.holdings.len() as u64).to_le_bytes());
        for (&(m, a), h) in &self.ledger.holdings {
            hasher.update(&m.get().to_le_bytes());
            hasher.update(&a.get().to_le_bytes());
            hasher.update(&h.position.to_le_bytes());
            hasher.update(&h.reserve.cents().to_le_bytes());
        }
        Ok(*hasher.finalize().as_bytes())
    }
}

/// The place-order checks after the market and account lookups, in their fixed order
/// (docs/gates/G2-plan.md): the first failing check names the rejection.
fn validate_place(
    book: &Book,
    ledger: &Ledger,
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
    if o.tif == TimeInForce::Gtc && ledger.resting_count(o.account)? >= MAX_RESTING_ORDERS {
        return Ok(Err(RejectReason::TooManyOrders));
    }
    if !ledger.can_afford(o.account, o.market, o.side, price, qty.contracts())? {
        return Ok(Err(RejectReason::InsufficientCollateral));
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
