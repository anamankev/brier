# Durability: crash recovery, sync cost, and replay speed (G3)

Status: COMPLETE. Registered at `4eb5c41`, before any journal code.

## 1. Questions
1. Does the core ever lose an acknowledged command when killed at an arbitrary moment, and does it always recover to exactly the state a clean replay produces?
2. What does a durable sync (`F_FULLFSYNC`) cost on this machine? That sets the floor under order acknowledgment latency (P3, G4).
3. How fast does recovery replay the journal (P5)?

## 2. Hypotheses
- D1 (crash safety): across 1,000 SIGKILLs at random moments under load, zero acknowledged commands are missing after restart. Refuted by any one missing.
- D2 (exact recovery): after every restart, the recovered state hash equals the hash from a clean replay of the recovered journal from empty, and the full invariant check passes. Refuted by any one mismatch or failure.
- P5 (SPEC §11): replaying 10 million journaled commands from empty takes ≤ 30 s. Refuted if the median of 3 runs exceeds 30 s.
- Observation, no threshold: the latency distribution of `sync_data` on a journal segment (p50, p99, max over 1,000 syncs of 4 KiB appends), and whether it matches `F_FULLFSYNC` (milliseconds) rather than plain `fsync` (microseconds).

## 3. Design
- Crash harness: `brier-core` fed by a load generator at full speed. A SIGKILL at a uniformly random delay of 0-200 ms after each start; 1,000 cycles. The driver records every `ack <seq>` it receives before each kill.
- Recovery check after each restart: the journal's last sequence number is at least the highest acknowledged sequence; the core's reported state hash equals `brier-replay` run over the journal's commands from empty; `check_invariants` passes.
- Replay timing: a 10-million-command journal from the state-aware differential stream (seed 1), three runs, release build, AC power.
- Sync timing: 1,000 append-and-sync cycles on a fresh segment, timed with `Instant`.

## 4. Results

### Crash study (D1, D2)
Command: `make crash SEED=1`. Deviation from the registered design: the clean replay is `brier-core verify`, which reads the journal directly with the same engine, rather than exporting the journal to JSON for `brier-replay`. This removes a conversion step; the comparison is the same. Settings: 64 KiB segments, so kills land during rotation; a snapshot every 2,000 commands; SIGKILL uniformly 0-200 ms after the core reports ready; after 30% of kills, 1-4,096 random bytes appended to the newest segment to imitate a power cut mid-batch.

| Attempt | Code | Outcome |
|---|---|---|
| 1 | `7af90b9` | Stopped by hand after about 300 cycles, with no failure reported. Each cycle had grown to about 10 s, because every restart re-read the whole journal and replay ran at 510 k commands/s. Profiling found `cancel_all` walking every market ever listed. Fixed in `8840643` (kill switches walk the live-order index; closed books free their storage; restarts skip segments a snapshot covers). Replay rose to about 2 M commands/s. |
| 2 | `8840643` | **Failed at cycle 183.** The core was killed while creating a segment, before its header was written; the injected torn write then filled that empty file with garbage, and recovery reported "bad segment magic" and refused to start. No acknowledged command was lost: the core stopped rather than guess. Nothing in that segment could have been acknowledged, because a header is synced before any record is appended. Fixed in `cbdf678`: an unreadable header on the newest segment, with at most one batch of bytes, is an interrupted creation and is removed (ADR-0006). |
| 3 | `cbdf678` | **Passed**: 1,000 cycles, 0 failures, 1,921 s. |

Attempt 3 in detail:

| Measure | Value |
|---|---|
| Kill-and-restart cycles | 1,000 |
| Highest acknowledged sequence number | 6,058,014 |
| Journal end after the final restart | 6,058,014 (nothing acknowledged is missing) |
| Recovered-hash vs clean-replay mismatches | 0 in 1,001 checks (every restart plus the final one) |
| Torn writes injected / torn tails truncated | 314 / 316 (two more than injected; consistent with kills during segment creation, which leave an empty newest segment, but not traced individually) |
| Restarts that recovered from a snapshot | 998 |
| Damaged snapshots skipped | 0 |

### Sync cost
`brier-core sync-bench --count 1000`, 4 KiB appended per sync, at `8992dbc`; Apple M5 SSD, APFS, macOS 26.6, on battery power. A plain `fsync(2)` of 4 KiB from Python on the same volume serves as the reference.

| Operation | p50 | p90 | p99 | max |
|---|---|---|---|---|
| `File::sync_data` (journal commit) | 3,945 µs | 4,022 µs | 4,443 µs | 8,971 µs |
| Plain `fsync(2)` | 33 µs | 45 µs | 58 µs | 166 µs |

### Replay speed (P5)
Journal: 10,000,000 commands from the state-aware stream (seed 1), written through `brier-core serve`; 324 MB in 6 segments. Replay: `brier-core verify`, from empty, read-only, with the full invariant check at the end (not included in the time). Code `8e7275d`, AC power.

| Run | 1 | 2 | 3 | Median |
|---|---|---|---|---|
| Replay seconds | 7.95 | 7.74 | 7.29 | **7.74** |

All three runs ended at the same state hash (`49d32c5f…`) and passed the invariant check. 7.74 s is about 1.29 million commands per second from disk.

## 5. Verdicts
- D1: SUPPORTED on attempt 3. Attempt 2 is reported above: it lost nothing, but failed to restart, which a production system must also not do.
- D2: SUPPORTED. 1,001 of 1,001 recovered states matched a clean replay, with full invariant checks.
- Sync observation: `sync_data` costs about 120 times a plain `fsync`, matching a full device flush (`F_FULLFSYNC`). That agrees with the Rust 1.99.1 source (ADR-0004). It sets the floor for order acknowledgment at about 4 ms per batch, so G4's P3 target (p99 ≤ 5 ms at 10,000 orders/s) depends on batching, not on matching speed.
- P5: SUPPORTED. The median of 7.74 s is well under the 30 s bound. Recovery after a crash replays only the commands since the last snapshot, so in practice a restart takes a fraction of this.

## 6. Interpretation
- Group commit is what makes durability affordable. One 4 ms flush covers a whole batch: at 10,000 orders per second, a batch holds about 40 orders, so durability costs about 0.1 ms of device time per order instead of 4.
- The two incidents are why this study exists. The first was a scaling bug (cost growing with history) that ordinary tests never reach. The second was an edge case in recovery: a crash in the narrow window of segment creation, combined with a torn write. Both were found by repeated adversarial restarts, not by reasoning about the code.

## 7. Limitations and threats to validity
- SIGKILL stops the process, not the machine. It proves that nothing is acknowledged before its sync returns, and that recovery is correct. It cannot prove that the SSD honors cache flushes on power loss; that would need power-cut testing, which is out of scope.
- One machine, one SSD, APFS.
- The injected tears append garbage after the last synced byte. A real power cut can also lose or reorder unsynced writes inside the last batch. Recovery handles both the same way (truncate within one batch), but only appending was exercised.

## 8. Next questions
- Replay runs at about 1.3 M commands/s against the engine's 2.7 M/s in memory. How much of the gap is reading and decoding the journal, and does it matter for P3 in G4?
- Can power-loss behavior be tested on this hardware at all (an external drive one can unplug), or is it accepted as a stated limit?
