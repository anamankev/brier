//! Domain units for the exchange (SPEC §2.1).
//!
//! Every quantity that reaches the engine or the ledger is one of these types, and each can
//! only be constructed in its valid range. Prices are integer ticks in `[1, 99]`, quantities
//! are whole contracts, and money is a signed integer count of credit-cents with checked
//! arithmetic. There is no floating point anywhere in this crate.

mod digest;
mod error;
mod ids;
mod money;
mod order;

pub use digest::Digest;
pub use error::UnitError;
pub use ids::{AccountId, MarketId, OrderId, Seq};
pub use money::{
    Credits, MAX_ORDER_QTY, MAX_PRICE_TICKS, MIN_PRICE_TICKS, PAYOUT_TICKS, Price, Quantity,
    pair_value,
};
pub use order::{Outcome, Side};
