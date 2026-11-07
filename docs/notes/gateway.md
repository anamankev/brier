# Gateway latency and fan-out (P3, P3b, P4)

Status: COMPLETE. Registered before any gateway code (`49aed1a`); measured at `0f9504f` plus the power and swap checks.

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

### Attempt 2: the registered run
Command: `make gateway-bench`. macOS 26.6, rustc 1.99.1, release build, AC power at the start and end of every run (checked by the harness), PostgreSQL 16.15 in Docker with the follower running. Other applications held about 9.2 GB of swap throughout (9.36 GB before run 1, 9.19 GB after run 3), which was not ideal but steady.

| Run | Order to ack p50 | p90 | p99 | p99.9 | max | Schedule lag p99 | Fan-out p50 | p99 | p99.9 |
|---|---|---|---|---|---|---|---|---|---|
| 1 | 4.75 ms | 6.06 ms | 7.18 ms | 21.0 ms | 36.0 ms | 1.89 ms | 5.67 ms | 13.1 ms | 25.4 ms |
| 2 | 4.73 ms | 5.79 ms | 17.4 ms | 297 ms | 353 ms | 1.99 ms | 7.38 ms | 20.1 ms | 51.2 ms |
| 3 | 4.74 ms | 6.08 ms | 8.18 ms | 122 ms | 176 ms | 1.92 ms | 8.01 ms | 36.9 ms | 87.1 ms |
| **Pooled** | 4.74 ms | 6.06 ms | **9.14 ms** | 179 ms | 353 ms | | 6.88 ms | **22.9 ms** | 68.2 ms |

- Orders: 600,000 per run in the measured window, exactly the scheduled 10,000 per second. Outcomes per run: about 241,500 resting, 195,500 cancelled, 17,600 filled by IOC, 130,000 rejected for insufficient collateral (P1's mix rests faster than it cancels, so accounts run out of free credits), and 15,000 cancels of orders that had already filled (`unknown_order`). Every outcome is a sequenced, durable command and is in the sample.
- Fan-out: about 33.4 million deltas received per run across 1,000 subscribers (about 557,000 per second).
- The schedule lag stayed under 2 ms at p99 in every run, so the client kept to the open-loop schedule.

## 5. Verdicts
- **P3 (p99 ≤ 5 ms): refuted.** Pooled p99 9.14 ms. As predicted at registration: the median alone (4.74 ms) is above one durable sync (3.95 ms), because an order waits for the batch in progress before its own.
- **P3b (p99 ≤ 9.9 ms): supported**, narrowly (9.14 ms). The per-run p99s were 7.2, 17.4, and 8.2 ms; run 2's tail (p99.9 297 ms) is a stall outside the order path, discussed below.
- **P4 (p99 ≤ 10 ms): refuted.** Pooled p99 22.9 ms, and even the median (6.9 ms) is far above what the publisher's own work takes.

### Diagnosis of the P4 miss (after the verdicts; not registered)
`brier-gateway-bench --diagnose N`: one run, 5 s warm-up, 20 s measured, the same order load.

| Subscribers | Fan-out p50 | p99 | p99.9 | Deltas received |
|---|---|---|---|---|
| 1 | 0.07 ms | 0.14 ms | 0.15 ms | 7,879 |
| 100 | 0.45 ms | 0.88 ms | 1.01 ms | 776,600 |
| 300 | 1.29 ms | 2.89 ms | 4.80 ms | 2,354,700 |
| 1,000 (registered run, pooled) | 6.88 ms | 22.9 ms | 68.2 ms | 100,339,134 |

- The median grows linearly with the number of subscribers, about 4.3 µs per subscriber, so each update reaches its last subscriber about 1,000 × 4.3 µs after its first. Below 300 subscribers P4's bound holds easily.
- A 5 s `sample` of the process at 1,000 subscribers (which itself disturbed that run, so its latencies are not used) puts the busy time in system calls: `sendto` 7,306 samples, `recvfrom` 2,356, buffer zeroing 2,844, against a few hundred for the gateway's own code (JSON, routing, the book). The gateway writes every message to every subscriber with its own `send`, which is one system call each: about 557,000 a second here, and the clients, on the same machine, make as many reads.
- Two other findings from the same data: order latency rises while fan-out runs (p99 7.2 ms alone, 23.8 to 66.2 ms during fan-out in the registered runs), because orders and fan-out share the gateway's 4 worker threads; and the registered runs had stalls of 100-350 ms in the order path (run 2's p99.9) that the fan-out load makes worse.

### Follow-up hypothesis (registered here before the change is made)
- **P4-coalesced**: if each connection writes all its queued messages and then flushes once, instead of one system call per message, then P4's pooled p99 over 3 registered-length runs is ≤ 10 ms. Refuted if it exceeds 10 ms. Order latency during fan-out is reported beside it.

### Follow-up result: P4-coalesced
Code `7058161`, same command, conditions, and checks (AC power at every run's start and end).

| Run | Fan-out p50 | p99 | p99.9 | Order to ack p99 (alone) | Order to ack p99 (during fan-out) |
|---|---|---|---|---|---|
| 1 | 3.35 ms | 8.00 ms | 13.2 ms | 7.02 ms | 20.3 ms |
| 2 | 3.79 ms | 37.5 ms | 224 ms | 6.97 ms | 86.6 ms |
| 3 | 3.98 ms | 15.3 ms | 28.3 ms | 18.1 ms | 43.3 ms |
| **Pooled** | 3.69 ms | **16.4 ms** | 76.3 ms | 7.97 ms | |

- **P4-coalesced: refuted** (pooled p99 16.4 ms > 10 ms). Coalescing halved the median (6.88 to 3.69 ms) and cut the pooled p99 from 22.9 to 16.4 ms; run 1 alone met the bound.
- What is left is not the steady per-message cost but stalls of 100-400 ms that hit some runs and not others, in the order path and the fan-out at the same time (run 2 here, run 2 of the registered run). P3 and P3b were re-measured on the way: pooled p99 7.97 ms, again P3 refuted and P3b supported.

## 6. Interpretation
- Order acknowledgment costs one to two durable syncs, as G3 predicted; the median is 4.7 ms, and a single sync is 3.95 ms. Getting P3's 5 ms at p99 would need a cheaper durability point (a battery-backed or replicated log), not a faster gateway.
- Fan-out of 1,000 subscribers on one machine is bounded by system calls: about 557,000 messages a second written by the gateway and read by the clients on the same cores. Coalescing writes removed much of the steady cost.
- The remaining tail is intermittent stalls that strike orders and fan-out together and vary from run to run under the same code. With 9 GB of swap in use by other applications, a 4-core gateway runtime competing with 4 client threads, Docker, and the core's sync thread on one laptop, the stalls are most likely the machine (paging or scheduling), but this was not proven.

## 7. Limitations and threats to validity
- Load client, gateway, and subscribers share one 10-core laptop, so they compete for CPU. Measured latency is an upper bound on the gateway's own cost.
- Localhost only: no network, TLS, or proxy cost.

## 8. Next questions
- Where do the 100-400 ms stalls come from? Repeat with swap cleared, then with the clients on a second machine; if they persist, trace scheduling (`sample` the core and gateway threads during one).
- Conflation for slow or very wide fan-out: send a book's latest state at most every N ms per subscriber instead of every delta, as large exchanges do for retail feeds.
