//! Accounts, positions, escrow, collateral, and settlement (SPEC §2.3-§2.4, §4; ADR-0005).
//!
//! Each account has one ledger balance, `cash`. Collateral is a lock on it: `reserved` is the
//! sum over markets of [`reserve_for`], recomputed after every command for every (account,
//! market) the command touched. Every movement of cash is an [`Entry`] whose legs sum to zero.

use std::collections::BTreeMap;

use brier_types::{
    AccountId, Credits, MAX_PRICE_TICKS, MIN_PRICE_TICKS, MarketId, Outcome, PAYOUT_TICKS, Price,
    Side,
};

use crate::{EngineError, Entry, EntryKind, Event, LedgerAccount};

/// Credits granted to every new account: 1,000,000 credit-cents = 10,000 credits (SPEC §1.2).
pub const SIGNUP_GRANT: Credits = Credits::from_cents(1_000_000);
/// Most GTC orders one account may have resting across all markets (SPEC §4.4).
pub const MAX_RESTING_ORDERS: u32 = 1_000;

const LEVELS: usize = 100; // index = price in ticks; 0 unused

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Account {
    pub(crate) cash: Credits,
    pub(crate) reserved: Credits,
    pub(crate) resting: u32,
}

/// One account's state in one market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Holding {
    /// Contracts held: > 0 YES, < 0 NO.
    pub(crate) position: i64,
    /// Resting buy quantity by price.
    pub(crate) buy_qty: [u32; LEVELS],
    /// Resting sell quantity by price.
    pub(crate) sell_qty: [u32; LEVELS],
    /// The reserve last charged for this pair.
    pub(crate) reserve: Credits,
}

impl Holding {
    const fn new() -> Self {
        Self {
            position: 0,
            buy_qty: [0; LEVELS],
            sell_qty: [0; LEVELS],
            reserve: Credits::ZERO,
        }
    }

    fn qty_mut(&mut self, side: Side, price: Price) -> Result<&mut u32, EngineError> {
        let levels = match side {
            Side::Buy => &mut self.buy_qty,
            Side::Sell => &mut self.sell_qty,
        };
        levels
            .get_mut(usize::from(price.ticks()))
            .ok_or(EngineError::Corrupt("holding price index"))
    }
}

fn err(e: brier_types::UnitError) -> EngineError {
    EngineError::Unit(e)
}

fn mul(a: i64, b: i64) -> Result<i64, EngineError> {
    a.checked_mul(b)
        .ok_or(EngineError::Corrupt("credit arithmetic overflow"))
}

fn add(a: i64, b: i64) -> Result<i64, EngineError> {
    a.checked_add(b)
        .ok_or(EngineError::Corrupt("credit arithmetic overflow"))
}

fn sub(a: i64, b: i64) -> Result<i64, EngineError> {
    a.checked_sub(b)
        .ok_or(EngineError::Corrupt("credit arithmetic overflow"))
}

fn neg(a: i64) -> Result<i64, EngineError> {
    a.checked_neg()
        .ok_or(EngineError::Corrupt("credit arithmetic overflow"))
}

/// The worst-case cash a holding's resting orders could still need (ADR-0005): each resting
/// buy unit costs its price, each sell unit costs 100 − price, and the contracts already held
/// (NO for buys, YES for sells) cover the cheapest units on that side for free.
pub(crate) fn reserve_for(h: &Holding) -> Result<Credits, EngineError> {
    let payout = i64::from(PAYOUT_TICKS);
    // Buys: cheapest are the lowest prices; free units come from NO contracts held.
    let mut free = if h.position < 0 {
        h.position.unsigned_abs()
    } else {
        0
    };
    let mut total: i64 = 0;
    for t in MIN_PRICE_TICKS..=MAX_PRICE_TICKS {
        let n = u64::from(
            *h.buy_qty
                .get(usize::from(t))
                .ok_or(EngineError::Corrupt("reserve index"))?,
        );
        let covered = n.min(free);
        free = free.saturating_sub(covered);
        let charged = i64::try_from(n.saturating_sub(covered))
            .map_err(|_| EngineError::Corrupt("reserve units"))?;
        total = add(total, mul(charged, i64::from(t))?)?;
    }
    // Sells: cheapest are the highest prices (cost 100 − p); free units come from YES held.
    let mut free = if h.position > 0 {
        h.position.unsigned_abs()
    } else {
        0
    };
    for t in (MIN_PRICE_TICKS..=MAX_PRICE_TICKS).rev() {
        let n = u64::from(
            *h.sell_qty
                .get(usize::from(t))
                .ok_or(EngineError::Corrupt("reserve index"))?,
        );
        let covered = n.min(free);
        free = free.saturating_sub(covered);
        let charged = i64::try_from(n.saturating_sub(covered))
            .map_err(|_| EngineError::Corrupt("reserve units"))?;
        let unit = payout
            .checked_sub(i64::from(t))
            .ok_or(EngineError::Corrupt("sell unit cost"))?;
        total = add(total, mul(charged, unit)?)?;
    }
    Ok(Credits::from_cents(total))
}

#[derive(Default)]
pub(crate) struct Ledger {
    pub(crate) accounts: BTreeMap<AccountId, Account>,
    pub(crate) holdings: BTreeMap<(MarketId, AccountId), Holding>,
    pub(crate) escrow: BTreeMap<MarketId, Credits>,
    pub(crate) issuance: Credits,
    /// (account, market) pairs whose reserve may have changed during the current command.
    touched: Vec<(AccountId, MarketId)>,
}

impl Ledger {
    pub(crate) fn has_account(&self, a: AccountId) -> bool {
        self.accounts.contains_key(&a)
    }

    pub(crate) fn add_market(&mut self, m: MarketId) {
        self.escrow.insert(m, Credits::ZERO);
    }

    /// Open an account with the signup grant. `None` if it already exists.
    pub(crate) fn open_account(&mut self, a: AccountId) -> Result<Option<Entry>, EngineError> {
        if self.accounts.contains_key(&a) {
            return Ok(None);
        }
        self.issuance = self.issuance.checked_sub(SIGNUP_GRANT).map_err(err)?;
        self.accounts.insert(
            a,
            Account {
                cash: SIGNUP_GRANT,
                reserved: Credits::ZERO,
                resting: 0,
            },
        );
        let entry = Entry::new(
            EntryKind::Grant,
            &[
                (
                    LedgerAccount::Issuance,
                    SIGNUP_GRANT.checked_neg().map_err(err)?,
                ),
                (LedgerAccount::Cash(a), SIGNUP_GRANT),
            ],
        )?;
        Ok(Some(entry))
    }

    fn account(&self, a: AccountId) -> Result<&Account, EngineError> {
        self.accounts
            .get(&a)
            .ok_or(EngineError::Corrupt("unknown account in ledger"))
    }

    fn account_mut(&mut self, a: AccountId) -> Result<&mut Account, EngineError> {
        self.accounts
            .get_mut(&a)
            .ok_or(EngineError::Corrupt("unknown account in ledger"))
    }

    fn holding_mut(&mut self, m: MarketId, a: AccountId) -> &mut Holding {
        self.holdings.entry((m, a)).or_insert_with(Holding::new)
    }

    pub(crate) fn resting_count(&self, a: AccountId) -> Result<u32, EngineError> {
        Ok(self.account(a)?.resting)
    }

    /// Whether `a` can afford an order of `qty` at `price` on `side` in `m`, counting it as if
    /// it rested in full (ADR-0005).
    pub(crate) fn can_afford(
        &self,
        a: AccountId,
        m: MarketId,
        side: Side,
        price: Price,
        qty: u32,
    ) -> Result<bool, EngineError> {
        let account = self.account(a)?;
        let mut h = self
            .holdings
            .get(&(m, a))
            .cloned()
            .unwrap_or_else(Holding::new);
        let cell = h.qty_mut(side, price)?;
        *cell = cell
            .checked_add(qty)
            .ok_or(EngineError::Corrupt("resting quantity overflow"))?;
        let increase = reserve_for(&h)?.checked_sub(h.reserve).map_err(err)?;
        let available = account.cash.checked_sub(account.reserved).map_err(err)?;
        Ok(increase <= available)
    }

    /// An order of `qty` now rests in the book.
    pub(crate) fn rest(
        &mut self,
        a: AccountId,
        m: MarketId,
        side: Side,
        price: Price,
        qty: u32,
    ) -> Result<(), EngineError> {
        let cell = self.holding_mut(m, a).qty_mut(side, price)?;
        *cell = cell
            .checked_add(qty)
            .ok_or(EngineError::Corrupt("resting quantity overflow"))?;
        let account = self.account_mut(a)?;
        account.resting = account
            .resting
            .checked_add(1)
            .ok_or(EngineError::Corrupt("resting count overflow"))?;
        self.touched.push((a, m));
        Ok(())
    }

    /// A resting order shrank by `by` (a fill or an amend); `removed` if it left the book.
    pub(crate) fn shrink(
        &mut self,
        a: AccountId,
        m: MarketId,
        side: Side,
        price: Price,
        by: u32,
        removed: bool,
    ) -> Result<(), EngineError> {
        let cell = self.holding_mut(m, a).qty_mut(side, price)?;
        *cell = cell
            .checked_sub(by)
            .ok_or(EngineError::Corrupt("resting quantity underflow"))?;
        if removed {
            let account = self.account_mut(a)?;
            account.resting = account
                .resting
                .checked_sub(1)
                .ok_or(EngineError::Corrupt("resting count underflow"))?;
        }
        self.touched.push((a, m));
        Ok(())
    }

    /// Move cash and positions for one trade (G2 plan, "Positions and trade cash").
    pub(crate) fn trade(
        &mut self,
        market: MarketId,
        buyer: AccountId,
        seller: AccountId,
        price: Price,
        qty: u32,
    ) -> Result<Entry, EngineError> {
        let filled = i64::from(qty);
        let ticks = i64::from(price.ticks());
        let payout = i64::from(PAYOUT_TICKS);
        let buyer_position = self.holding_mut(market, buyer).position;
        let seller_position = self.holding_mut(market, seller).position;
        // Buyer closes NO contracts it holds; seller opens NO beyond the YES it holds.
        let buyer_closing = filled.min(neg(buyer_position.min(0))?);
        let seller_opening = sub(filled, filled.min(seller_position.max(0)))?;
        let buyer_cash = sub(mul(payout, buyer_closing)?, mul(ticks, filled)?)?;
        let seller_cash = sub(mul(ticks, filled)?, mul(payout, seller_opening)?)?;
        let escrow_change = sub(mul(payout, seller_opening)?, mul(payout, buyer_closing)?)?;

        let buyer_account = self.account_mut(buyer)?;
        buyer_account.cash = buyer_account
            .cash
            .checked_add(Credits::from_cents(buyer_cash))
            .map_err(err)?;
        let seller_account = self.account_mut(seller)?;
        seller_account.cash = seller_account
            .cash
            .checked_add(Credits::from_cents(seller_cash))
            .map_err(err)?;
        let escrow = self
            .escrow
            .get_mut(&market)
            .ok_or(EngineError::Corrupt("trade in a market without escrow"))?;
        *escrow = escrow
            .checked_add(Credits::from_cents(escrow_change))
            .map_err(err)?;
        let buyer_holding = self.holding_mut(market, buyer);
        buyer_holding.position = add(buyer_holding.position, filled)?;
        let seller_holding = self.holding_mut(market, seller);
        seller_holding.position = sub(seller_holding.position, filled)?;
        self.touched.push((buyer, market));
        self.touched.push((seller, market));
        Entry::new(
            EntryKind::Trade,
            &[
                (LedgerAccount::Cash(buyer), Credits::from_cents(buyer_cash)),
                (
                    LedgerAccount::Cash(seller),
                    Credits::from_cents(seller_cash),
                ),
                (
                    LedgerAccount::Escrow(market),
                    Credits::from_cents(escrow_change),
                ),
            ],
        )
    }

    /// Pay out every position in `m` and zero its escrow (SPEC §2.4, ADR-0002).
    pub(crate) fn settle(
        &mut self,
        market: MarketId,
        outcome: Outcome,
        out: &mut Vec<Event>,
    ) -> Result<(), EngineError> {
        let payout = i64::from(PAYOUT_TICKS);
        let half = payout
            .checked_div(2)
            .ok_or(EngineError::Corrupt("half payout"))?;
        let keys: Vec<(MarketId, AccountId)> = self
            .holdings
            .range((market, AccountId::new(0))..=(market, AccountId::new(u64::MAX)))
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            let account = key.1;
            let holding = self
                .holdings
                .get_mut(&key)
                .ok_or(EngineError::Corrupt("holding vanished"))?;
            let position = holding.position;
            let units = i64::try_from(position.unsigned_abs())
                .map_err(|_| EngineError::Corrupt("position size"))?;
            let amount = match outcome {
                Outcome::Yes if position > 0 => mul(payout, units)?,
                Outcome::No if position < 0 => mul(payout, units)?,
                Outcome::Void => mul(half, units)?,
                Outcome::Yes | Outcome::No => 0,
            };
            holding.position = 0;
            if amount == 0 {
                continue;
            }
            let pay = Credits::from_cents(amount);
            let escrow = self
                .escrow
                .get_mut(&market)
                .ok_or(EngineError::Corrupt("settling a market without escrow"))?;
            *escrow = escrow.checked_sub(pay).map_err(err)?;
            let paid = self.account_mut(account)?;
            paid.cash = paid.cash.checked_add(pay).map_err(err)?;
            out.push(Event::Entry(Entry::new(
                EntryKind::Settle,
                &[
                    (
                        LedgerAccount::Escrow(market),
                        pay.checked_neg().map_err(err)?,
                    ),
                    (LedgerAccount::Cash(account), pay),
                ],
            )?));
        }
        if self.escrow.get(&market) != Some(&Credits::ZERO) {
            return Err(EngineError::Corrupt("escrow is not zero after settlement"));
        }
        Ok(())
    }

    /// Recompute the reserve of every pair this command touched, emit `Reserved` for each
    /// change (sorted by account, then market), and check cash ≥ reserved for those accounts.
    pub(crate) fn finish_command(&mut self, out: &mut Vec<Event>) -> Result<(), EngineError> {
        let mut touched = std::mem::take(&mut self.touched);
        touched.sort_unstable();
        touched.dedup();
        for &(a, m) in &touched {
            let h = self
                .holdings
                .get_mut(&(m, a))
                .ok_or(EngineError::Corrupt("touched pair has no holding"))?;
            let new = reserve_for(h)?;
            let old = h.reserve;
            if new == old {
                continue;
            }
            h.reserve = new;
            let acct = self
                .accounts
                .get_mut(&a)
                .ok_or(EngineError::Corrupt("unknown account in ledger"))?;
            acct.reserved = acct
                .reserved
                .checked_add(new.checked_sub(old).map_err(err)?)
                .map_err(err)?;
            out.push(Event::Reserved {
                account: a,
                market: m,
                amount: new,
            });
        }
        for &(a, _) in &touched {
            let acct = self.account(a)?;
            if acct.cash < acct.reserved || acct.reserved < Credits::ZERO {
                return Err(EngineError::Corrupt("I3: cash below reserved"));
            }
        }
        self.touched = touched;
        self.touched.clear();
        Ok(())
    }

    /// I1-I4 (SPEC §4.3). The engine separately checks that the resting quantities here match
    /// the books.
    pub(crate) fn check(&self) -> Result<(), EngineError> {
        let mut total = self.issuance;
        for acct in self.accounts.values() {
            total = total.checked_add(acct.cash).map_err(err)?;
            if acct.cash < acct.reserved || acct.reserved < Credits::ZERO {
                return Err(EngineError::Corrupt("I3: cash below reserved"));
            }
        }
        for e in self.escrow.values() {
            total = total.checked_add(*e).map_err(err)?;
        }
        if total != Credits::ZERO {
            return Err(EngineError::Corrupt("I1: credits not conserved"));
        }
        let mut reserved_sum: BTreeMap<AccountId, Credits> = BTreeMap::new();
        let mut long: BTreeMap<MarketId, i64> = BTreeMap::new();
        let mut short: BTreeMap<MarketId, i64> = BTreeMap::new();
        for (&(m, a), h) in &self.holdings {
            if reserve_for(h)? != h.reserve {
                return Err(EngineError::Corrupt("I4: stale reserve"));
            }
            let r = reserved_sum.entry(a).or_insert(Credits::ZERO);
            *r = r.checked_add(h.reserve).map_err(err)?;
            let side = if h.position > 0 {
                long.entry(m).or_insert(0)
            } else {
                short.entry(m).or_insert(0)
            };
            *side = add(
                *side,
                i64::try_from(h.position.unsigned_abs())
                    .map_err(|_| EngineError::Corrupt("position size"))?,
            )?;
        }
        for (a, acct) in &self.accounts {
            if reserved_sum.get(a).copied().unwrap_or(Credits::ZERO) != acct.reserved {
                return Err(EngineError::Corrupt(
                    "I4: reserved total disagrees with holdings",
                ));
            }
        }
        for (m, e) in &self.escrow {
            let oi = long.get(m).copied().unwrap_or(0);
            if oi != short.get(m).copied().unwrap_or(0) {
                return Err(EngineError::Corrupt(
                    "I2: long and short open interest differ",
                ));
            }
            if e.cents() != mul(i64::from(PAYOUT_TICKS), oi)? {
                return Err(EngineError::Corrupt(
                    "I2: escrow is not 100 x open interest",
                ));
            }
        }
        Ok(())
    }
}
