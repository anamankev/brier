# ADR 0004: Durability by group commit, with a full flush on macOS

Date: 2026-11-06  Status: Accepted

## Context
SPEC §5 promises that an acknowledged command survives a crash. A command counts as durable only once the storage device itself has it: data still in the operating system's page cache, or in the drive's own write cache, is lost on power failure. On macOS, `fsync(2)` asks the drive to write but does not force its cache to flush. Apple's fsync(2) man page directs applications that need durability to `fcntl(F_FULLFSYNC)`, which costs on the order of milliseconds on Apple SSDs. Linux's `fdatasync` flushes the device cache on drives that honor flush commands. The engine handles millions of commands per second (P1), so syncing once per command would cap the exchange at a few hundred commands per second.

## Options considered
1. **Sync every command.** Simplest, and each ack waits for exactly its own record. Throughput is capped at about 1 / sync latency: hundreds per second with `F_FULLFSYNC`.
2. **Acknowledge before syncing (asynchronous durability).** Fast, but breaks the promise: a crash loses acknowledged commands. Rejected by SPEC §5.
3. **Group commit.** Commands arriving while a sync is in flight join the next batch. One sync covers the whole batch, and every command in it is acknowledged only after that sync returns. Throughput scales with batch size; latency is about one sync interval plus queueing.

## Decision
Option 3. The writer appends a batch of records, syncs once (`sync_data`, which must issue `F_FULLFSYNC` on Apple targets; see G3 plan, "Durability"), then acknowledges every command in the batch. The engine applies commands in sequence order, as each batch is appended. Their events are published only after the batch's sync, so no client can observe the effects of a command that might not survive a crash.

## Consequences
- An acknowledgment means "on the device". The crash harness checks this with SIGKILL. SIGKILL does not cut power, so it proves the ordering (no ack before the sync returns), not the device's honesty about flushing. That limit is stated in the durability note.
- Latency under load is bounded below by `F_FULLFSYNC`, so P3 (G4) is set against it. The durability note measures its distribution first.
- Wrong-choice signal: if measured `F_FULLFSYNC` latency makes P3 unreachable at the target rate even with large batches, revisit with a dedicated log device, or by relaxing to `fsync` with the risk stated. Never by acknowledging early.
