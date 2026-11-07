use core::fmt;

/// A value outside the range its unit allows, or arithmetic that would overflow.
///
/// Invalid values are rejected, never clipped into range (SPEC §4.3, hard rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitError {
    /// A price outside `[MIN_PRICE_TICKS, MAX_PRICE_TICKS]`.
    PriceOutOfRange {
        /// The rejected value, in ticks.
        ticks: u8,
    },
    /// A quantity outside `[1, MAX_ORDER_QTY]`.
    QuantityOutOfRange {
        /// The rejected value, in contracts.
        contracts: u32,
    },
    /// Credit arithmetic whose exact result does not fit in an `i64`.
    CreditsOverflow,
    /// The sequence counter reached `u64::MAX` and has no successor.
    SequenceExhausted,
}

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PriceOutOfRange { ticks } => write!(
                f,
                "price {ticks} is outside [{}, {}] ticks",
                crate::MIN_PRICE_TICKS,
                crate::MAX_PRICE_TICKS
            ),
            Self::QuantityOutOfRange { contracts } => write!(
                f,
                "quantity {contracts} is outside [1, {}] contracts",
                crate::MAX_ORDER_QTY
            ),
            Self::CreditsOverflow => f.write_str("credit arithmetic overflowed i64"),
            Self::SequenceExhausted => f.write_str("sequence number space is exhausted"),
        }
    }
}

impl std::error::Error for UnitError {}
