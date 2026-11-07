//! Identifiers and the global sequence number (SPEC §3.2).

use crate::UnitError;

/// A user or system account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountId(u64);

impl AccountId {
    /// The account with numeric id `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The numeric id.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// A listed market.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarketId(u32);

impl MarketId {
    /// The market with numeric id `id`.
    #[must_use]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// The numeric id.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// An order, as numbered by the exchange (clients use their own client order IDs, SPEC §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderId(u64);

impl OrderId {
    /// The order with numeric id `id`.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// The numeric id.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The sequencer's global, gap-free command number. The first command is number 1.
///
/// Time priority in the book is sequence order (SPEC §3.1), and replay reproduces state by
/// applying commands in this order (SPEC §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seq(u64);

impl Seq {
    /// The number of the first command.
    pub const FIRST: Self = Self(1);

    /// The sequence number `n`, as read back from a journal.
    #[must_use]
    pub const fn from_raw(n: u64) -> Self {
        Self(n)
    }

    /// The numeric value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The next number, or an error once the counter is exhausted. Never wraps: a wrapped
    /// sequence would break time priority and replay.
    ///
    /// # Errors
    /// [`UnitError::SequenceExhausted`] if `self` is `u64::MAX`.
    pub const fn next(self) -> Result<Self, UnitError> {
        match self.0.checked_add(1) {
            Some(n) => Ok(Self(n)),
            None => Err(UnitError::SequenceExhausted),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_advances_by_one_and_never_wraps() {
        assert_eq!(Seq::FIRST.next(), Ok(Seq::from_raw(2)));
        assert_eq!(
            Seq::from_raw(u64::MAX).next(),
            Err(UnitError::SequenceExhausted)
        );
    }

    #[test]
    fn ids_round_trip() {
        assert_eq!(AccountId::new(7).get(), 7);
        assert_eq!(MarketId::new(u32::MAX).get(), u32::MAX);
        assert_eq!(OrderId::new(u64::MAX).get(), u64::MAX);
    }
}
