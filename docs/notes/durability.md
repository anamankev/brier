# Durability: crash recovery, sync cost, and replay speed (G3)

Status: REGISTERED (hypotheses committed before any journal code exists)

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

## 5. Verdicts

## 6. Interpretation

## 7. Limitations and threats to validity
- SIGKILL stops the process, not the machine. It proves that nothing is acknowledged before its sync returns, and that recovery is correct. It cannot prove that the SSD honors cache flushes on power loss; that would need power-cut testing, which is out of scope.
- One machine, one SSD, APFS.

## 8. Next questions
