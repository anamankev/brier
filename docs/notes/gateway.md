# Gateway latency and fan-out (P3, P3b, P4)

Status: REGISTERED (hypotheses committed before any gateway code exists)

## 1. Questions
1. How long does an order take from leaving the client to the client receiving its acknowledgment, through the real gateway, core, journal, and a full device flush per batch?
2. How quickly do market-data updates reach many subscribers?

## 2. Hypotheses
- P3 (SPEC §11, registered before G3): order-to-acknowledgment p99 ≤ 5 ms at 10,000 orders/s on localhost. Refuted if the pooled p99 over 3 runs exceeds 5 ms.
  - Expectation stated before measuring: G3 measured a durable sync at p50 3.95 ms and p99 4.44 ms. An order arriving just after a batch begins its sync waits for that sync, then for its own batch's sync, so its latency approaches two syncs. P3 is therefore expected to be refuted on this hardware. It is kept as registered, not loosened.
- P3b (new, informed by G3): order-to-acknowledgment p99 ≤ 2 × (sync p99 4.44 ms) + 1 ms = 9.9 ms at 10,000 orders/s. Refuted if the pooled p99 over 3 runs exceeds 9.9 ms.
- P4 (SPEC §11): with 1,000 WebSocket subscribers on one market's book channel, the time from the core publishing a durable batch to each subscriber receiving the update has p99 ≤ 10 ms. Refuted if the pooled p99 over 3 runs exceeds 10 ms.

## 3. Design
- Load: a Rust load client (in `crates/bench`). WebSocket order entry from 100 accounts, open-loop at 10,000 orders/s for 60 s per run, after a 10 s warm-up. Same 60/35/5 mix as P1, using client order IDs.
- Latency: client send to receipt of the acknowledgment, measured by the client's monotonic clock into an HDR histogram.
- Fan-out: 1,000 subscriber connections from the same machine. The publisher stamps each update with the core's publish time (monotonic, same machine). Subscribers record receipt minus publish time.
- Environment: release builds, journal on the internal SSD, Postgres follower running, AC power, heavy apps closed; macOS version and rustc recorded.
- Command: `make gateway-bench` (added in G4).

## 4. Results

### Deviations from the registered design
Decided while building the harness, before any registered run:
1. **One process, separate runtimes.** The gateway runs in the benchmark process on its own 4-thread Tokio runtime, with the load client and subscribers on a second 4-thread runtime, the core on its own thread, and the read-model follower writing to PostgreSQL in Docker. Requests still cross real localhost TCP and WebSocket framing. In-process setup lets the benchmark raise the per-address limits, which would otherwise refuse 1,100 connections from one address (the per-account order limit stays as in production).
2. **The fan-out stamp is the publisher's wall clock**, taken when it publishes each message (`ts`, in microseconds), not the core's monotonic clock: the stamp has to cross the wire, and both ends read the same machine's clock. It starts after the core's broadcast hop (microseconds), so it slightly understates the registered interval.
3. **Order latency runs from the actual send**, not the scheduled time. The schedule's lag is reported beside it; a run whose sends fell behind schedule shows it there.
4. **P4's updates come from the P3 load** running at the same time (10,000 orders/s over 10 markets, so `book:1` gets about a tenth of them). The registration did not say where updates come from.
5. The registered window is 10 s warm-up plus 60 s measured, per phase, per run.
6. **Power.** The first attempt started on battery power (the machine was unplugged between the pre-run check and the start). Its numbers are kept below as a non-result. The harness now refuses to start on battery and marks a run invalid if the power source reads battery at its start or end.

### Attempt 1: on battery power (not a result)
Code `62da8ff`, macOS 26.6, rustc 1.99.1, about 10 GB of swap already in use by other apps. The power source read battery when the run started, so the registered environment was not met and no verdict is drawn from it. Kept because it shows what the next run must explain.

| Run | Order to ack p50 | p99 | Schedule lag p99 | Fan-out p50 | p99 |
|---|---|---|---|---|---|
| 1 | 4.83 ms | 8.88 ms | 1.82 ms | 5.25 ms | 16.7 ms |
| 2 | 4.74 ms | 23.7 ms | 2.01 ms | 5.89 ms | 115 ms |
| 3 | 4.83 ms | 8.10 ms | 1.77 ms | 5.68 ms | 18.3 ms |
| Pooled | | 11.6 ms | | | 60.3 ms |

- Each run sent 600,000 orders in the measured window (10,000 per second, as scheduled). About 22% were rejected for insufficient collateral: P1's mix rests orders faster than it cancels them, so accounts run out of free credits. Rejections are sequenced and acknowledged like any command, so they are part of the latency sample.
- Run 2 was disturbed by something outside the benchmark (its p99 tripled while the schedule lag stayed normal). With swap that heavy, paging is the first suspect; the registered run will record swap before and after.

## 5. Verdicts

## 6. Interpretation

## 7. Limitations and threats to validity
- Load client, gateway, and subscribers share one 10-core laptop, so they compete for CPU. Measured latency is an upper bound on the gateway's own cost.
- Localhost only: no network, TLS, or proxy cost.

## 8. Next questions
