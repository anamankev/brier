//! Order sides and market outcomes (SPEC §2.2, §2.4).

/// The side of an order in the market's single book, which is quoted in YES terms
/// (SPEC §2.2). "Buy NO at q" enters the book as `Sell` at `100 − q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Bids for YES contracts.
    Buy,
    /// Offers of YES contracts.
    Sell,
}

impl Side {
    /// The side an incoming order matches against.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Buy => Self::Sell,
            Self::Sell => Self::Buy,
        }
    }
}

/// How a market settles (SPEC §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Outcome {
    /// YES contracts pay 100; NO contracts pay 0.
    Yes,
    /// NO contracts pay 100; YES contracts pay 0.
    No,
    /// The rulebook's void conditions were met: every contract on both sides pays 50.
    Void,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_is_an_involution() {
        for side in [Side::Buy, Side::Sell] {
            assert_ne!(side.opposite(), side);
            assert_eq!(side.opposite().opposite(), side);
        }
    }
}
