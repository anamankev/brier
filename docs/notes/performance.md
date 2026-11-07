# Engine performance (P1, P2)

Status: REGISTERED (hypotheses committed before any engine code exists)

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
- Command: `make bench` (added in G1).

## 4. Results
- Commit hash of the code that produced the results:
- Environment:
- Tables:

## 5. Verdicts

## 6. Interpretation

## 7. Limitations and threats to validity
- A fanless laptop throttles under sustained load. Runs are short, but the power state and the order of runs are recorded.
- The workload is synthetic. Real order flow may have deeper books or more cancels.

## 8. Next questions
