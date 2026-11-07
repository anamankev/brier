//! The journal: an append-only, checksummed log of sequenced commands, and snapshots of engine
//! state (SPEC §5, ADR-0004, ADR-0006). The journal is the exchange's source of truth: replaying
//! it from empty rebuilds every state the engine ever had.

mod codec;
mod crc;

pub use codec::{CodecError, decode, encode};
pub use crc::{Crc32c, crc32c};
