use brier_types::{AccountId, Credits, MarketId, OrderId, Outcome, Price, Quantity, Side};

use crate::{EngineError, MarketState};

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
    /// A kill switch (`cancel_all`) cancelled it (SPEC §4.5).
    KillSwitch,
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
    /// No account with that ID.
    UnknownAccount,
    /// An account with that ID already exists.
    AccountExists,
    /// The account already has `MAX_RESTING_ORDERS` GTC orders resting.
    TooManyOrders,
    /// The order could need more cash than the account has available (ADR-0005).
    InsufficientCollateral,
}

/// A balance in the double-entry ledger (SPEC §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LedgerAccount {
    /// An account's cash.
    Cash(AccountId),
    /// A market's escrow, which backs every open YES/NO pair with 100.
    Escrow(MarketId),
    /// The system issuance account: minus every credit ever granted.
    Issuance,
}

/// The three kinds of ledger entry, and the only ones that exist (SPEC §1.2, §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Issuance to a new account's cash.
    Grant,
    /// Buyer cash, seller cash, and the market's escrow, for one trade.
    Trade,
    /// Escrow to one account's cash at settlement.
    Settle,
}

/// A balanced ledger entry: at most three legs, none zero, summing to zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    kind: EntryKind,
    legs: [(LedgerAccount, Credits); 3],
    len: usize,
}

impl Entry {
    /// Build an entry, dropping zero legs and keeping the given order.
    ///
    /// # Errors
    /// [`EngineError::Corrupt`] if the legs do not sum to zero or more than three are nonzero.
    pub fn new(kind: EntryKind, legs: &[(LedgerAccount, Credits)]) -> Result<Self, EngineError> {
        let mut out = [(LedgerAccount::Issuance, Credits::ZERO); 3];
        let mut len = 0;
        let mut sum = Credits::ZERO;
        for &(account, amount) in legs {
            sum = sum.checked_add(amount)?;
            if amount == Credits::ZERO {
                continue;
            }
            let slot = out
                .get_mut(len)
                .ok_or(EngineError::Corrupt("entry has more than three legs"))?;
            *slot = (account, amount);
            len = len
                .checked_add(1)
                .ok_or(EngineError::Corrupt("entry length"))?;
        }
        if sum != Credits::ZERO {
            return Err(EngineError::Corrupt("entry legs do not sum to zero"));
        }
        Ok(Self {
            kind,
            legs: out,
            len,
        })
    }

    /// The entry's kind.
    #[must_use]
    pub const fn kind(&self) -> EntryKind {
        self.kind
    }

    /// The nonzero legs, in order.
    #[must_use]
    pub fn legs(&self) -> &[(LedgerAccount, Credits)] {
        self.legs.get(..self.len).unwrap_or(&[])
    }
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
    /// A new account received its signup grant.
    AccountOpened {
        /// The account.
        account: AccountId,
        /// Its starting cash.
        cash: Credits,
    },
    /// A ledger entry.
    Entry(Entry),
    /// An account's reserve in one market changed; `amount` is the new reserve.
    Reserved {
        /// The account.
        account: AccountId,
        /// The market.
        market: MarketId,
        /// The new reserve.
        amount: Credits,
    },
    /// A closed market settled; payout entries follow.
    Settled {
        /// The market.
        market: MarketId,
        /// How it settled.
        outcome: Outcome,
    },
}
