//! Deterministic matching engine for binary contracts (SPEC §3).
//!
//! The engine is a pure state machine: [`Engine::apply`] takes one sequenced [`Command`] and
//! appends the resulting [`Event`]s. It reads no clock, draws no random numbers, does no I/O,
//! and never iterates an unordered collection, so replaying the same commands reproduces the
//! same events and the same [`Engine::state_hash`] (SPEC §3.2).
//!
//! The exact semantics (event order, rejection order, order types, self-trade prevention) are
//! fixed in `docs/gates/G1-plan.md`, which the Python reference engine also implements.

mod book;
mod command;
mod engine;
mod event;

pub use command::{Command, MarketState, NewOrder, Sequenced, TimeInForce};
pub use engine::{Engine, EngineError, RestingOrder};
pub use event::{CancelReason, Event, RejectReason, Trade};
