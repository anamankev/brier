use brier_types::{AccountId, MarketId, OrderId, Outcome, Seq, Side};

/// How long an order may rest (SPEC §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeInForce {
    /// Good till cancelled: any unfilled remainder rests in the book.
    Gtc,
    /// Immediate or cancel: fill what crosses now, cancel the rest.
    Ioc,
    /// Fill or kill: fill the whole quantity now, or reject without trading.
    Fok,
}

/// A market's lifecycle state (SPEC §3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarketState {
    /// Listed, not yet trading. Cancels only.
    PreOpen,
    /// Trading.
    Open,
    /// Trading paused. Cancels only.
    Halted,
    /// No orders; every resting order was cancelled on entry.
    Closed,
    /// Settled YES or NO (settlement itself is G2).
    Resolved,
    /// Settled as void (SPEC §2.4).
    Voided,
}

impl MarketState {
    /// Whether `set_state` may move from `self` to `next`. A transition to the current state
    /// is not allowed, and Resolved and Voided are reached only through settlement.
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::PreOpen | Self::Halted => matches!(next, Self::Open | Self::Closed),
            Self::Open => matches!(next, Self::Halted | Self::Closed),
            // Resolved and Voided are reached only by `Command::Settle` (G2 plan).
            Self::Closed | Self::Resolved | Self::Voided => false,
        }
    }

    /// Stable one-byte code, used in the state hash.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::PreOpen => 0,
            Self::Open => 1,
            Self::Halted => 2,
            Self::Closed => 3,
            Self::Resolved => 4,
            Self::Voided => 5,
        }
    }
}

/// An order as submitted. Price and quantity are raw wire values; the engine validates them
/// and rejects out-of-range values with a typed reason, never clipping them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewOrder {
    /// The submitting account.
    pub account: AccountId,
    /// The market to trade.
    pub market: MarketId,
    /// Buy or sell YES (SPEC §2.2).
    pub side: Side,
    /// Limit price in ticks; valid range 1-99.
    pub price: u8,
    /// Quantity in contracts; valid range 1 to `MAX_ORDER_QTY`.
    pub qty: u32,
    /// Time in force.
    pub tif: TimeInForce,
    /// Reject instead of trading if the order would cross on arrival. Only valid with GTC.
    pub post_only: bool,
}

/// One input to the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// List a new market in `PreOpen`.
    CreateMarket {
        /// The new market's ID.
        market: MarketId,
    },
    /// Move a market through its lifecycle.
    SetState {
        /// The market.
        market: MarketId,
        /// The requested state.
        state: MarketState,
    },
    /// Submit an order.
    Place(NewOrder),
    /// Cancel a resting order.
    Cancel {
        /// The account that owns the order.
        account: AccountId,
        /// The order.
        order: OrderId,
    },
    /// Open an account with the signup grant (SPEC §1.2).
    OpenAccount {
        /// The new account's ID.
        account: AccountId,
    },
    /// Settle a closed market (SPEC §2.4).
    Settle {
        /// The market.
        market: MarketId,
        /// YES, NO, or void.
        outcome: Outcome,
    },
    /// Kill switch: cancel every resting order matching the filters; neither means all.
    CancelAll {
        /// Only this account's orders.
        account: Option<AccountId>,
        /// Only this market's orders.
        market: Option<MarketId>,
    },
    /// Kill switch: halt every open market.
    HaltAll,
    /// Reduce a resting order's quantity, keeping its time priority.
    Amend {
        /// The account that owns the order.
        account: AccountId,
        /// The order.
        order: OrderId,
        /// The new remaining quantity (raw wire value).
        qty: u32,
    },
}

/// A command with the sequence number the sequencer assigned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequenced {
    /// Gap-free sequence number; the first command is 1.
    pub seq: Seq,
    /// The command.
    pub command: Command,
}

#[cfg(test)]
mod tests {
    use super::MarketState::{Closed, Halted, Open, PreOpen, Resolved, Voided};

    #[test]
    fn lifecycle_allows_exactly_the_listed_transitions() {
        // docs/gates/G1-plan.md and G2-plan.md: six set_state transitions; settlement reaches
        // Resolved and Voided.
        let all = [PreOpen, Open, Halted, Closed, Resolved, Voided];
        let allowed = [
            (PreOpen, Open),
            (PreOpen, Closed),
            (Open, Halted),
            (Open, Closed),
            (Halted, Open),
            (Halted, Closed),
        ];
        for from in all {
            for to in all {
                assert_eq!(
                    from.can_transition_to(to),
                    allowed.contains(&(from, to)),
                    "{from:?} -> {to:?}"
                );
            }
        }
    }
}
