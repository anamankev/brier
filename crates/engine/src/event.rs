use brier_types::{AccountId, MarketId, OrderId, Price, Quantity, Side};

use crate::MarketState;

/// Why a resting or incoming order stopped being live without filling completely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CancelReason {
    /// The owner cancelled it.
    User,
    /// An IOC order's unfilled remainder.
    IocRemainder,
    /// Matching reached the same account's resting order (`STP = CancelTaker`, SPEC §3.1).
    SelfTrade,
    /// The market closed while the order was resting.
    MarketClosed,
}

/// Why a command was refused. A rejected command changes no state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectReason {
    /// No market with that ID.
    UnknownMarket,
    /// A market with that ID already exists.
    MarketExists,
    /// The market's state does not accept this command.
    MarketNotOpen,
    /// The lifecycle does not allow this state change.
    InvalidTransition,
    /// An invalid combination of order flags (post-only without GTC).
    InvalidOrder,
    /// Price outside 1-99 ticks.
    InvalidPrice,
    /// Quantity outside 1 to `MAX_ORDER_QTY`.
    InvalidQuantity,
    /// A post-only order would have traded on arrival.
    PostOnlyWouldCross,
    /// A fill-or-kill order could not be filled completely.
    FokUnfillable,
    /// No such live order for this account (other accounts' orders are not revealed).
    UnknownOrder,
    /// An amend that does not reduce the remaining quantity.
    InvalidAmend,
}

/// One execution between a resting (maker) and an incoming (taker) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trade {
    /// The market.
    pub market: MarketId,
    /// The resting order.
    pub maker: OrderId,
    /// The incoming order.
    pub taker: OrderId,
    /// The resting order's account.
    pub maker_account: AccountId,
    /// The incoming order's account.
    pub taker_account: AccountId,
    /// The incoming order's side; the maker is on the opposite side.
    pub taker_side: Side,
    /// Execution price: the resting order's price (SPEC §3.1).
    pub price: Price,
    /// Contracts traded.
    pub qty: Quantity,
}

/// Something that happened as the result of one command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A market was listed in `PreOpen`.
    MarketCreated {
        /// The market.
        market: MarketId,
    },
    /// A market changed state.
    MarketStateChanged {
        /// The market.
        market: MarketId,
        /// The new state.
        state: MarketState,
    },
    /// An order passed validation. Its ID is the command's sequence number.
    Accepted {
        /// The order.
        order: OrderId,
    },
    /// An execution.
    Trade(Trade),
    /// The unfilled part of an order now rests in the book.
    Rested {
        /// The order.
        order: OrderId,
        /// Quantity resting.
        remaining: Quantity,
    },
    /// An order (or an incoming order's remainder) was cancelled.
    Cancelled {
        /// The order.
        order: OrderId,
        /// Quantity that was still unfilled.
        remaining: Quantity,
        /// Why.
        reason: CancelReason,
    },
    /// A resting order's quantity was reduced; it kept its priority.
    Amended {
        /// The order.
        order: OrderId,
        /// The new remaining quantity.
        remaining: Quantity,
    },
    /// The command was refused and changed nothing.
    Rejected {
        /// Why.
        reason: RejectReason,
    },
}
