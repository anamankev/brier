# G4 review: gateway, APIs, read models

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| Fuzzers run 1 hour each with no crash | MET after one fix | Campaign at `458d606`, 1 hour per target: 5 of 6 clean; `snapshot_restore` found a non-canonical encoding (below). Fixed; that target then ran another hour clean on the fixed code (`d734e54`): 491,263,462 executions, no crash, and the original crashing input refused. Executions in "Measured" |
| Market-data gap recovery tested | MET | `crates/gateway/tests/ws.rs`: a subscriber forced 40,000 changes behind is resubscribed with a fresh snapshot, and its book equals the core's with zero sequence gaps. `services/tests/test_client.py`: Hypothesis drops, duplicates and reorders messages; the client tracker always refetches or ignores and never applies a gap. `test_client_live.py`: the reference client against a real gateway process |
| P3 and P4 measured | MET | Registered 3-run benchmark (`docs/notes/gateway.md`): P3 refuted (p99 9.14 ms > 5 ms), P3b supported (≤ 9.9 ms), P4 refuted (p99 22.9 ms > 10 ms); the registered follow-up P4-coalesced also refuted (16.4 ms) |

## What was built
- **`brier-core` as a library.** `CoreHandle::submit` returns once the command's batch is durable. `query` answers between batches, so a read never sees an unacknowledged command. `subscribe` and `durable` complete the interface.
- **`brier-gateway`** (axum, tokio):
  - 23 REST endpoints from one table, which also feeds the routing test and the OpenAPI contract test.
  - Session tokens (Argon2id passwords, SHA-256-stored tokens) and Ed25519-signed requests. Signatures cover the timestamp, method, path with query, and body hash, with a 5 s skew window and a replay cache.
  - Token-bucket limits per account, per address, and on the auth endpoints. Bodies are capped at 8 KiB and requests time out after 10 s.
  - Admin actions are audit-logged before they run.
- **WebSocket API.**
  - Book, trade and private account channels, each with its own sequence number, plus the journal sequence and a timestamp.
  - A book channel starts with a snapshot, then sends deltas.
  - A lagging subscriber gets a new snapshot, never a silent gap.
  - Order entry is bounded per connection (256 in flight), writes are coalesced, and clean close codes are sent (1001 on shutdown).
- **Read models in PostgreSQL.** A follower tails the journal up to the durable point, replays it in its own engine, and writes fills and order events in one transaction per batch, with its cursor in the same transaction. Tested: the live tables equal a rebuild from the journal.
- **Client order IDs** in the engine, the journal (format version 2) and the snapshot (body version 2), for idempotent retries. The reference engine, wire format and protocol changed with them.
- **Contracts held to the code:** OpenAPI 3.1 and AsyncAPI 3.0. Tests in `make check` fail if an endpoint, error code, operation or message type drifts.
- **Python reference client:** REST, Ed25519 signing, and a WebSocket transport that resubscribes itself, tested live in CI.
- **Supply chain.** Every locked Rust and Python dependency was at least 14 days old when chosen (`scripts/dependency_age.py`, enforced in CI on lockfile changes).
- **Six fuzz targets** and `make fuzz-campaign`.
- **The G4 threat model** (`docs/threat-model.md`).

## Found during G4, and fixed
1. **Snapshot restore accepted non-canonical bytes** (fuzz campaign, `snapshot_restore`). It was found after 1 hour by an 81-byte input that listed two accounts in decreasing ID order.
   - What went wrong: the input restored to a valid state, but snapshotting that state produced different bytes.
   - Why it matters: no state was ever wrong, but "one state, one encoding" is what makes snapshot bytes comparable across machines and runs.
   - The fix: restore now requires strictly increasing keys in every keyed section (markets, accounts, holdings, client ID windows). It also refuses empty client ID windows and windows over the 10,000 limit, which the fuzzer could not reach at its 4 KiB input size.
   - The regression test is `non_canonical_snapshots_are_refused`.
2. **Market-data fan-out made one write system call per message per subscriber.** About 557,000 a second at 1,000 subscribers, found by profiling the P4 miss. Each connection now writes everything queued and flushes once (up to 64 messages), which halved the median.
3. **The gateway never answered a client's close frame** (RFC 6455), found by the live Python client test. After a client close, it now keeps reading for up to 1 s so the library sends the reply.
4. **The 14-day dependency rule was being broken unnoticed.**
   - Writing the checker turned up 16 crates and 5 Python dev tools that were newer than 14 days.
   - They were rolled back, and uv resolution is pinned to a cutoff date (`exclude-newer`).
   - CI now runs the check on every lockfile change.
5. **The benchmark's teardown deadlocked**: dropping the test schema while the follower was still writing. Runtimes now shut down first, and the schema is dropped from a fresh connection.
6. **The first registered benchmark ran on battery.** It was discarded, not reported. The harness now refuses to start on battery and invalidates a run that ends on battery.

## Measured
- Order to acknowledgment, 10,000 orders/s from 100 open-loop traders: p50 4.74 ms, pooled p99 9.14 ms. A re-measurement gave 7.97 ms.
- Fan-out to 1,000 subscribers, about 33.4 M deltas per run:
  - before coalescing: p50 6.88 ms, p99 22.9 ms;
  - after: p50 3.69 ms, p99 16.4 ms.
- Fuzzing, 1 hour per target, about 2.16 billion executions in all:

  | Target | Executions |
  |---|---|
  | `journal_decode` | 1,427,907,876 |
  | `snapshot_restore` (re-run after the fix) | 491,263,462 |
  | `wire_parse` | 192,308,865 |
  | `gateway_request` | 40,158,075 |
  | `signature_verify` | 9,127,704 |
  | `journal_recover` | 805,423 |

## Deviations
- P3 and P4 are refuted, and their bounds were kept, not loosened. P3's 5 ms is below two durable syncs on this hardware (predicted at registration). P4's remaining tail is intermittent 100–400 ms stalls that hit orders and fan-out at the same time. The likely cause is the machine (about 9 GB of swap, one laptop running clients, gateway, Docker and the sync thread), but this is not proven. `docs/notes/gateway.md` §8 lists the experiments that would separate the causes.
- The benchmark runs clients and gateway on one machine over localhost, with no TLS. The numbers are an upper bound on the gateway's own cost and say nothing about network latency.
- The snapshot fix landed after the campaign. Only `snapshot_restore` was re-run for an hour; the other five targets never call snapshot code.

## Exam questions
1. The read-model follower replays the journal in its own engine instead of consuming the core's event broadcast. Give a failure that the broadcast design would allow and this one rules out. What does it cost, in CPU and in staleness?
2. A signed request carries a timestamp, and the gateway keeps a replay cache. Why are both needed? What does an attacker who captures one signed request get if the cache is lost on restart, and how long is that window?
3. A book subscriber that falls behind is resubscribed with a snapshot instead of being disconnected or sent its backlog. What does this guarantee the client, and what does it give up? When is disconnecting the better rule?
4. The fuzz finding never produced a wrong state. Argue whether it was a bug, naming what in this system depends on snapshots being canonical, and say why proptest round-trips from the engine's own snapshots could never have found it.
