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

## 5. Verdicts

## 6. Interpretation

## 7. Limitations and threats to validity
- Load client, gateway, and subscribers share one 10-core laptop, so they compete for CPU. Measured latency is an upper bound on the gateway's own cost.
- Localhost only: no network, TLS, or proxy cost.

## 8. Next questions
