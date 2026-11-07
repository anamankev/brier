//! The journal: an append-only, checksummed log of sequenced commands, and snapshots of engine
//! state (SPEC §5, ADR-0004, ADR-0006). The journal is the exchange's source of truth: replaying
//! it from empty rebuilds every state the engine ever had.

mod codec;
mod crc;
mod log;
mod snapshot;

pub use codec::{CodecError, FORMAT_VERSION, decode, encode};
pub use crc::{Crc32c, crc32c};
pub use log::{
    JournalError, JournalWriter, MAX_BATCH_BYTES, MAX_PAYLOAD, Recovered, SEGMENT_BYTES,
    Truncation, read_journal, recover, recover_after,
};
pub use snapshot::{KEEP_SNAPSHOTS, LatestSnapshot, SnapshotFile, latest_snapshot, write_snapshot};
