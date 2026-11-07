# G3 review: journal, snapshots, recovery

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| 1,000 SIGKILLs with zero lost acknowledged commands and matching state hashes | MET | Attempt 3 (`cbdf678`): 1,000 cycles, highest acknowledged 6,058,014 = journal end after the final restart; 1,001 of 1,001 recovered hashes equal to a clean replay; 314 torn writes injected, all truncated (`docs/notes/durability.md`) |
| P5 measured | MET | 10 M commands replayed from empty in a median of 7.74 s against a bound of 30 s (`docs/notes/durability.md`) |

## What was built
- `brier-journal`:
  - canonical command codec, pinned byte for byte, with strict decoding;
  - CRC-32C, implemented in house and checked against the standard check value;
  - segment writer with group commit and bounded batches;
  - recovery with torn-tail truncation (at most one batch) and corruption errors;
  - a read-only scan, and a scan that skips segments covered by a snapshot;
  - atomic snapshot files with fallback past damaged ones.
- `Engine::snapshot`/`restore`: canonical state bytes; restore rebuilds derived state and checks every invariant. Property-tested at random points of random streams.
- `brier-wire`: the JSON wire format, shared by the replay tool and the core.
- `brier-core`:
  - `serve`: sequencer, journal, engine, acknowledgment only after the sync, snapshots;
  - `verify`: clean read-only replay with invariant checks;
  - `sync-bench`.
- The crash harness, with injected torn writes and a final restart check; a 10-cycle version runs in CI.
- ADR-0004 (group commit, F_FULLFSYNC, with a source and timing check) and ADR-0006 (formats and reading rules).

## Found during G3, and fixed
1. **Kill switches got slower with every market ever listed**, and closed books kept their storage forever. Found when the crash study slowed to about 10 s per cycle. `cancel_all` now walks the live-order index, and closed books free their storage. Replay went from 510 k to about 2 M commands/s on the crash journal.
2. **Restarts re-read the whole journal** even with a snapshot. They now skip segments the snapshot covers. A bug in that change (an empty newest segment hid the journal's end) was caught by the 10-cycle crash test minutes later, and fixed.
3. **A crash during segment creation, followed by a torn write, made recovery refuse to start** (crash study attempt 2, cycle 183). An unreadable header on the newest segment, with at most one batch of bytes, is now an interrupted creation (ADR-0006).

## Measured
- A durable sync (`F_FULLFSYNC`) of 4 KiB: p50 3.95 ms, p99 4.44 ms, about 120 times a plain `fsync`. This is the floor under order acknowledgment in G4.
- Replay from disk: about 1.29 M commands/s.

## Deviations
- `cargo-fuzz` needs a nightly toolchain (a new install). It was replaced by proptest corruption tests on stable; fuzzing is proposed for G4 with the API parsers.
- The crash study's clean replay is `brier-core verify`, not `brier-replay` (same engine, no JSON export step).

## Exam questions
1. The core applies each command to the engine before the batch is synced, but prints acknowledgments and events only after. Construct the failure a client could observe if events were printed before the sync, and say why applying early is still safe.
2. Why can a torn tail be at most one batch long? What would break, and what would you have to change, if the core started a second batch before the first one's sync returned?
3. A snapshot at sequence S is restored, then the journal after S is replayed. List every way this could produce a different state than replaying from 1, and the test or check that rules each one out.
4. SIGKILL never tears a write, so the harness injects tears by appending garbage. What real power-loss behavior does that not cover, and is the recovery rule still correct for it?
