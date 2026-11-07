# Engine performance (P1, P2)

Status: COMPLETE. Registered at `05abbb4`, before the first engine commit (`d9e9f68`). Results from `3db43b7`.

## 1. Question
How fast is the matching engine on one core, in process, for a realistic mix of commands? Is it fast enough that the order path's cost will be dominated by the journal and the network (G3, G4), not by matching?

## 2. Hypotheses (SPEC §11)
- P1: throughput ≥ 1,000,000 commands per second on one core for the mixed workload below. Refuted if the measured median over 5 runs is below 1,000,000.
- P2: per-command latency p99 ≤ 5 µs and p99.9 ≤ 20 µs on the same workload. Refuted if either percentile, pooled over the 5 runs, exceeds its bound.

## 3. Design
- Engine: release build (`lto = "thin"`, `codegen-units = 1`), single thread, no I/O, event buffer reused across commands.
- Workload, seeded and generated before timing starts:
  - 10 open markets and 100 accounts.
  - 60% passive limit orders: GTC, price uniform within ±10 ticks of 50, quantity 1-100.
  - 35% cancels of a randomly chosen live order.
  - 5% aggressive IOC orders that cross the spread.
- Warm-up: 1,000,000 commands. Measured: 10,000,000 commands per run; 5 runs with seeds 1-5.
- Throughput: commands divided by the wall time of the measured section, timed without per-command instrumentation.
- Latency: a separate pass timing each command with `std::time::Instant`, recorded into an HDR histogram (3 significant digits). The timer's own overhead is measured with empty timing pairs and reported.
- Hardware: 16 GB M5 MacBook Air, fanless. Record whether it was on power, the macOS version, and the rustc version.
- Command: `make bench`.
- Implementation detail settled while building the workload: "passive" means buys at 40-49 and sells at 51-60, so they never cross on arrival. "Aggressive" means buys at 60 or sells at 40. Both lie within ±10 ticks of 50, as registered.

## 4. Results
- Code: `3db43b7`. Command: `make bench`. Wall time 24.3 s for all five seeds, including workload generation; peak memory 1.06 GB.
- Environment: Apple M5 (4 performance and 6 efficiency cores), 16 GB, macOS 26.6, on AC power; rustc 1.99.1.

Throughput, commands per second (no per-command instrumentation):

| Seed | 1 | 2 | 3 | 4 | 5 | Median |
|---|---|---|---|---|---|---|
| Commands/s | 10,774,874 | 12,338,298 | 12,396,025 | 12,270,375 | 12,118,212 | **12,270,375** |

Latency per command, pooled over 50,000,000 timed commands:

| p50 | p90 | p99 | p99.9 | max |
|---|---|---|---|---|
| 42 ns | 250 ns | **334 ns** | **500 ns** | 12.6 ms |

Timer overhead (1,000,000 empty timing pairs): p50 below one tick, p99 42 ns, max 34 µs.

## 5. Verdicts
- P1: SUPPORTED. The median of 12.27 million commands per second is 12 times the threshold. The lowest run (seed 1, the first) is 10.8 million.
- P2: SUPPORTED. p99 is 334 ns against a 5 µs bound, and p99.9 is 500 ns against 20 µs.

## 6. Interpretation
- At about 82 ns per command on average, matching is not the bottleneck. A single `F_FULLFSYNC` on an Apple SSD takes on the order of milliseconds (to be measured in G3), and a localhost network round trip takes tens of microseconds. Group commit and the gateway will set the order-to-acknowledgment latency (P3), as the architecture assumed.
- The constant-time book (ADR-0003) is why the tail is tight. No command does work proportional to book depth except the rescan of at most 98 levels after a best level empties, and fill-or-kill's liquidity walk, which this workload does not use.
- The 12.6 ms maximum is one command among 50 million. A gap that long is the operating system descheduling the thread, not engine work. Pinning the engine thread is out of reach on macOS; on Linux it would be core isolation plus `SCHED_FIFO`.

## 7. Limitations and threats to validity
- **Timer resolution.** `Instant` on macOS reads `mach_absolute_time`, which ticks every 41.67 ns. The latency percentiles are therefore quantized to multiples of about 42 ns: the median command finishes within one tick, and p99 is 8 ticks. The percentiles are upper bounds at that resolution. Hardware cycle counters would resolve them further.
- **First run lower.** Seed 1 ran first and was 12% slower than the rest, consistent with the CPU ramping up. The median is robust to it.
- **Fanless laptop.** Sustained load throttles; these runs lasted about 5 seconds of timed work each.
- **Synthetic workload.** No fill-or-kill orders, a shallow book (passive orders stay within 10 ticks of the touch), and 10 markets. Deeper books mostly add level rescans, which are bounded. More markets do not change per-command cost (one BTreeMap lookup).
- **In process only.** No serialization, journal, or network: those are P3 and P4 in G3 and G4.

## 8. Next questions
- How much of the 82 ns is the order-ID HashMap (SipHash)? A faster deterministic hasher might matter at G4 rates.
- How large is the tail with a 1,000-level-deep book and frequent level emptying?


## Addendum: rerun after G2 (observation, not a registered hypothesis)

The same `make bench` workload at `496af0f`, with the ledger in the order path: every place is checked for collateral, every fill moves cash, and every touched (account, market) has its reserve recomputed. The workload now opens its 100 accounts first. Apple M5, macOS 26.6, AC power.

| Measure | G1 (matching only) | G2 (with ledger) |
|---|---|---|
| Throughput, median of 5 | 12.27 M commands/s | 2.51 M commands/s |
| p99 latency | 334 ns | 1,417 ns |
| p99.9 latency | 500 ns | 2,543 ns |

P1 (≥ 1 M/s) and P2 (p99 ≤ 5 µs, p99.9 ≤ 20 µs) still hold with the ledger included. The ledger costs about 4.9× in throughput.

Likely sources, to profile before optimizing anything:
- `can_afford` copies an 800-byte holding (two 100-level arrays) on every place.
- Each touched pair's reserve walks 2 × 99 levels.
- Holdings sit in a `BTreeMap` keyed by (market, account).

Limitation: with collateral enforced, part of this workload is rejected (cash locked by accumulated passive orders), so it is not the same mix of work as the G1 run. Measuring the rejection share is the first next question.
