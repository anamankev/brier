# G3 plan: journal, snapshots, recovery

Reads: SPEC §5, §3.2, §4.3, §11 (P5). Exit: 1,000 SIGKILLs under load with zero lost acknowledged commands, and every recovered state hash equal to a clean replay's hash; P5 measured.

## Design
- **What is journaled:** sequenced commands, not events. The engine is deterministic (SPEC §3.2), so commands reproduce every event, and they are smaller.
- **Record format** (ADR-0006): `len: u32 | crc32c: u32 | seq: u64 | payload`, little-endian. The CRC covers `seq` and `payload`. The payload is a hand-written canonical binary encoding of `Command`: a tag byte, then fixed-width fields. It has a version byte in the segment header, so the format can evolve without ambiguity. It does not use serde: the bytes on disk must be stable across library upgrades.
- **Segments:** `journal-<first seq, 20 digits>.log`, rotated at `SEGMENT_BYTES` (64 MiB), each with a header of magic, format version, and first seq.
- **Durability** (ADR-0004): commands are appended in batches and synced once per batch (group commit). Nothing is acknowledged before its batch is synced. On macOS, `File::sync_data`/`sync_all` must issue `fcntl(F_FULLFSYNC)`, because plain `fsync` does not flush the drive cache. The plan is to rely on Rust std doing this on Apple targets, verified two ways: from the std source for the pinned toolchain, and by measurement (`F_FULLFSYNC` costs milliseconds; a plain fsync costs microseconds). If either check fails, the fallback is a single audited `unsafe` call through `libc`, as a documented exception to `unsafe_code = forbid`.
- **Torn tails:** on open, a record that is short or fails its CRC at the very end of the last segment is truncated, and the truncation is logged with its offset and length. A bad record anywhere else is corruption: recovery stops with an error. It never skips records.
- **Snapshots:** `Engine::snapshot()` exports the full state as a canonical structure, and `Engine::restore()` rebuilds it, checking every invariant.
  - File `snapshot-<seq>.bin`: header (magic, version, seq, state hash, length, CRC), then the body.
  - Written atomically: write a temporary file, sync it, rename it, sync the directory.
  - Recovery loads the newest valid snapshot, checks that its recomputed state hash equals the recorded one, then replays later journal records.
- **Core process** (`brier-core`): reads commands, assigns sequence numbers, journals with group commit, applies each command to the engine, and prints `ack <seq>` only after the sync, followed by the events. G4's gateway replaces stdin with its own channel; the core stays the same.

## Units
1. ADR-0004 (durability, group commit, F_FULLFSYNC) and ADR-0006 (journal and snapshot formats). Register P5 and an fsync-cost observation in `docs/notes/durability.md` before measuring.
2. `crates/journal`:
   - command codec, with a round-trip property test for every command kind;
   - segment writer with group commit, and reader with CRC and torn-tail handling;
   - corruption property tests: random truncations and flipped bytes must give either a clean prefix (at the tail) or an error (in the middle), never wrong commands.
3. `Engine::snapshot`/`restore` with property tests: snapshot then restore at random points of random streams gives an identical state hash and identical subsequent events.
4. `brier-core` binary: sequencer, journal, engine, acks; recovery on start (snapshot + replay); a snapshot every `SNAPSHOT_INTERVAL` commands.
5. Crash harness (`services/.../crash.py`): drives load into `brier-core`, sends SIGKILL at random moments 1,000 times, and restarts each time. After every restart it checks three things: every acknowledged command is in the recovered journal; the full invariant check passes; and the recovered state hash equals a clean replay of the journal from empty.
6. P5: replay 10 million journaled commands from empty; target ≤ 30 s.
7. Gate review.

## Out of scope here, with reasons
- `cargo-fuzz` needs a nightly toolchain, which would be a new tool install (my approval first). G3 uses proptest corruption tests on stable instead; real fuzzing of the journal reader and the G4 protocol parsers is proposed for G4.
- Replication to a second machine is not in SPEC; single-node durability only.
