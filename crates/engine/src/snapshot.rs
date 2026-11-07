//! Engine snapshots: the whole state as canonical bytes, and back (SPEC §5, ADR-0006).
//!
//! Only state that cannot be derived is stored: each market's state and escrow, its resting
//! orders in time priority, each account's cash, each holding's position and reserve, and
//! issuance. Everything else (the order-ID map, per-price resting totals, resting counts,
//! reserved totals) is rebuilt on restore, and then every invariant is checked. A snapshot
//! that restores to a state failing any invariant is rejected, never patched.

use std::fmt;

use brier_types::{AccountId, Credits, MarketId, OrderId, Price, Seq, Side};

use crate::book::Book;
use crate::client_ids::ClientIds;
use crate::engine::Engine;
use crate::ledger::{Account, Holding, Ledger};
use crate::{EngineError, MarketState};

/// Version of the snapshot body encoding. Bump on any change; keep decoding old versions.
pub const SNAPSHOT_BODY_VERSION: u8 = 2;
/// The first version, without client order ID windows; still restorable.
const SNAPSHOT_BODY_V1: u8 = 1;

/// Why a snapshot could not be restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The body ended early, had trailing bytes, or held an invalid code. Names the problem.
    Malformed(&'static str),
    /// The body decoded, but the state it describes breaks an invariant.
    Invalid(EngineError),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(what) => write!(f, "snapshot is malformed: {what}"),
            Self::Invalid(e) => write!(f, "snapshot describes an invalid state: {e}"),
        }
    }
}

impl std::error::Error for RestoreError {}

struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn len(&mut self, n: usize) {
        self.u64(n as u64);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], RestoreError> {
        let (head, tail) = self
            .0
            .split_at_checked(N)
            .ok_or(RestoreError::Malformed("body is truncated"))?;
        self.0 = tail;
        head.try_into()
            .map_err(|_| RestoreError::Malformed("body is truncated"))
    }
    fn u8(&mut self) -> Result<u8, RestoreError> {
        Ok(u8::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, RestoreError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, RestoreError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn i64(&mut self) -> Result<i64, RestoreError> {
        Ok(i64::from_le_bytes(self.take()?))
    }
    /// A count, bounded by the bytes left so a damaged count cannot trigger a huge allocation.
    fn len(&mut self, min_item_bytes: usize) -> Result<usize, RestoreError> {
        let n =
            usize::try_from(self.u64()?).map_err(|_| RestoreError::Malformed("count too large"))?;
        if n.checked_mul(min_item_bytes)
            .is_none_or(|bytes| bytes > self.0.len())
        {
            return Err(RestoreError::Malformed("count exceeds the remaining body"));
        }
        Ok(n)
    }
}

fn state_from(code: u8) -> Result<MarketState, RestoreError> {
    match code {
        0 => Ok(MarketState::PreOpen),
        1 => Ok(MarketState::Open),
        2 => Ok(MarketState::Halted),
        3 => Ok(MarketState::Closed),
        4 => Ok(MarketState::Resolved),
        5 => Ok(MarketState::Voided),
        _ => Err(RestoreError::Malformed("market state code")),
    }
}

const ORDER_BYTES: usize = 8 + 8 + 1 + 4;
const MARKET_MIN_BYTES: usize = 4 + 1 + 8 + 8 + 8;
const ACCOUNT_BYTES: usize = 8 + 8;
const HOLDING_BYTES: usize = 4 + 8 + 8 + 8;
const CLIENT_WINDOW_MIN_BYTES: usize = 8 + 8;
const CLIENT_ENTRY_BYTES: usize = 8 + 8;

impl Engine {
    /// The whole engine state as canonical bytes, valid as of the last applied command
    /// (`next_seq − 1`).
    ///
    /// # Errors
    /// [`EngineError::Halted`] if the engine has halted: a halted state is not trustworthy and
    /// must never be snapshotted. [`EngineError::Corrupt`] if a book's links are broken.
    pub fn snapshot(&self) -> Result<Vec<u8>, EngineError> {
        if let Some((at, _)) = self.halted {
            return Err(EngineError::Halted { at });
        }
        let mut w = Writer(Vec::new());
        w.u8(SNAPSHOT_BODY_VERSION);
        w.u64(self.next_seq.get());
        w.i64(self.ledger.issuance.cents());
        w.len(self.markets.len());
        for (market, book) in &self.markets {
            w.u32(market.get());
            w.u8(book.state.code());
            w.i64(
                self.ledger
                    .escrow
                    .get(market)
                    .copied()
                    .unwrap_or(Credits::ZERO)
                    .cents(),
            );
            for side in [Side::Buy, Side::Sell] {
                let resting = book.resting(side)?;
                w.len(resting.len());
                for s in resting {
                    w.u64(s.id.get());
                    w.u64(s.account.get());
                    w.u8(s.price.ticks());
                    w.u32(s.remaining);
                }
            }
        }
        w.len(self.ledger.accounts.len());
        for (account, a) in &self.ledger.accounts {
            w.u64(account.get());
            w.i64(a.cash.cents());
        }
        w.len(self.ledger.holdings.len());
        for (&(market, account), h) in &self.ledger.holdings {
            w.u32(market.get());
            w.u64(account.get());
            w.i64(h.position);
            w.i64(h.reserve.cents());
        }
        w.len(self.client_ids.len());
        for (account, window) in &self.client_ids {
            w.u64(account.get());
            w.len(window.len());
            for (client_id, order) in window.entries() {
                w.u64(client_id);
                w.u64(order.get());
            }
        }
        Ok(w.0)
    }

    /// Rebuild an engine from [`Engine::snapshot`] bytes, then check every invariant.
    ///
    /// # Errors
    /// [`RestoreError::Malformed`] if the bytes are not a snapshot body;
    /// [`RestoreError::Invalid`] if they describe a state that breaks an invariant.
    pub fn restore(body: &[u8]) -> Result<Self, RestoreError> {
        let mut r = Reader(body);
        let version = r.u8()?;
        if version != SNAPSHOT_BODY_VERSION && version != SNAPSHOT_BODY_V1 {
            return Err(RestoreError::Malformed("unsupported snapshot version"));
        }
        let invalid = RestoreError::Invalid;
        let mut engine = Self::new();
        engine.next_seq = Seq::from_raw(r.u64()?);
        if engine.next_seq.get() == 0 {
            return Err(RestoreError::Malformed("next sequence number is zero"));
        }
        let mut ledger = Ledger::default();
        ledger.issuance = Credits::from_cents(r.i64()?);
        let mut resting: Vec<(MarketId, OrderId, AccountId, Side, Price, u32)> = Vec::new();
        for _ in 0..r.len(MARKET_MIN_BYTES)? {
            let market = MarketId::new(r.u32()?);
            let mut book = Book::new();
            book.state = state_from(r.u8()?)?;
            ledger.escrow.insert(market, Credits::from_cents(r.i64()?));
            for side in [Side::Buy, Side::Sell] {
                for _ in 0..r.len(ORDER_BYTES)? {
                    let id = OrderId::new(r.u64()?);
                    let account = AccountId::new(r.u64()?);
                    let price =
                        Price::new(r.u8()?).map_err(|_| RestoreError::Malformed("order price"))?;
                    let remaining = r.u32()?;
                    let h = book
                        .insert(id, account, side, price, remaining)
                        .map_err(invalid)?;
                    if engine.orders.insert(id, (market, h)).is_some() {
                        return Err(RestoreError::Malformed("duplicate order ID"));
                    }
                    resting.push((market, id, account, side, price, remaining));
                }
            }
            if engine.markets.insert(market, book).is_some() {
                return Err(RestoreError::Malformed("duplicate market"));
            }
        }
        for _ in 0..r.len(ACCOUNT_BYTES)? {
            let account = AccountId::new(r.u64()?);
            let cash = Credits::from_cents(r.i64()?);
            let fresh = Account {
                cash,
                reserved: Credits::ZERO,
                resting: 0,
            };
            if ledger.accounts.insert(account, fresh).is_some() {
                return Err(RestoreError::Malformed("duplicate account"));
            }
        }
        for _ in 0..r.len(HOLDING_BYTES)? {
            let market = MarketId::new(r.u32()?);
            let account = AccountId::new(r.u64()?);
            let mut h = Holding::new();
            h.position = r.i64()?;
            h.reserve = Credits::from_cents(r.i64()?);
            let reserved = &mut ledger
                .accounts
                .get_mut(&account)
                .ok_or(RestoreError::Malformed("holding of an unknown account"))?
                .reserved;
            *reserved = reserved
                .checked_add(h.reserve)
                .map_err(|_| RestoreError::Malformed("reserve overflow"))?;
            if ledger.holdings.insert((market, account), h).is_some() {
                return Err(RestoreError::Malformed("duplicate holding"));
            }
        }
        if version >= 2 {
            for _ in 0..r.len(CLIENT_WINDOW_MIN_BYTES)? {
                let account = AccountId::new(r.u64()?);
                let mut window = ClientIds::default();
                for _ in 0..r.len(CLIENT_ENTRY_BYTES)? {
                    let client_id = r.u64()?;
                    window.record(client_id, OrderId::new(r.u64()?));
                }
                if engine.client_ids.insert(account, window).is_some() {
                    return Err(RestoreError::Malformed("duplicate client ID window"));
                }
            }
        }
        if !r.0.is_empty() {
            return Err(RestoreError::Malformed("body has trailing bytes"));
        }
        for (market, _, account, side, price, remaining) in resting {
            if !ledger.has_account(account) {
                return Err(RestoreError::Malformed("order of an unknown account"));
            }
            ledger
                .rest(account, market, side, price, remaining)
                .map_err(invalid)?;
        }
        ledger.clear_touched();
        engine.ledger = ledger;
        engine.check_invariants().map_err(invalid)?;
        Ok(engine)
    }
}
