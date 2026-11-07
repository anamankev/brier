# G4 plan: gateway, APIs, read models

Reads: SPEC §6, §12, §11 (P3, P4), §13. Exit: fuzzers run 1 hour each with no crash; market-data gap recovery tested; P3 and P4 measured.

## Architecture
```
clients ── HTTPS/WS ──> brier-gateway (tokio, axum)
                          │  auth, rate limits, idempotency keys, request validation
                          ▼
                     CoreHandle ── channel ──> core thread (single writer)
                          ▲                      sequencer → journal append → engine → group commit
                          │                      replies + events published only after the sync
                          │   broadcast (seq, events), durable_seq watch
                          ├── market-data publisher (L2 books, per-market sequence, snapshots)
                          ├── private streams (each account's own order events)
                          └── read-model follower: tails the journal up to durable_seq, replays it in
                              its own engine replica, writes Postgres in one transaction per batch,
                              with its cursor in the same transaction
```
- **The core becomes a library.** `Core::start` runs recovery, then the group-commit loop on a dedicated thread, which owns the engine and the journal. `CoreHandle::submit(command)` returns the command's sequence number and events once its batch is durable. `CoreHandle::query(..)` answers between batches from the engine's state, so a read never sees an unacknowledged command. `brier-core serve` becomes a thin stdin adapter over the same library.
- **Read models follow the journal, not the event broadcast.** The follower reads only records at or below `durable_seq` and replays them through its own `Engine`. Because the engine is deterministic, the follower's events equal the core's. A lagging or restarted follower simply resumes from its cursor, and the read models can be rebuilt from the journal at any time (tested: rebuilt tables equal live tables).
- **Market data** comes from the broadcast. A subscriber that lags gets a fresh snapshot, never a silent gap.

## Engine and protocol change: client order IDs (SPEC §6 idempotency)
- `place` gains an optional `client_order_id` (u64, chosen by the client).
- If the account already has an accepted order with that ID inside its dedupe window, the command emits `duplicate` (order: the original order ID) and changes nothing else. The ID is recorded only when an order is accepted, so a rejected order can be retried with the same ID.
- Window: the last `CLIENT_ID_WINDOW` = 10,000 accepted IDs per account, evicted oldest first. This is engine state, so it is in snapshots and replays deterministically.
- Check position: right after `unknown_account`.
- Journal format version 2 adds the field; readers still decode version 1 (no client ID). Snapshot body version 2 adds the windows; version 1 still restores (empty windows). The reference engine, wire format, protocol document, and generator change together, and the differential tests hold them to it.

## Gateway (`crates/gateway`)
- **Identity (Postgres):** `users` (username, Argon2id hash, account ID, role), `sessions` (SHA-256 of a 32-byte random token, expiry), `api_keys` (Ed25519 public key, account), `audit_log`.
  - Registration opens the engine account (`open_account`) and the user row; a failure on either side leaves no half-made user.
- **Requests:** session bearer tokens for people. Ed25519-signed requests for bots, over `timestamp`, `method`, `path`, and the SHA-256 of the body. Timestamps more than `MAX_CLOCK_SKEW` (5 s) old or in the future are rejected; a signature seen in that window is rejected as a replay.
- **REST (`/v1`):**
  - `register`, `login`, `logout`, `keys`;
  - `markets`, `markets/{id}/book`;
  - `orders` (place, cancel, amend);
  - `portfolio` (cash, reserved, positions, open orders);
  - `fills` and `orders/history` (from Postgres);
  - admin: `markets` (create, set state, settle) and kill switches, with role `admin`, every call audit-logged with actor and reason.
- **WebSocket `/v1/ws`:** subscribe to `book:<market>`, `trades:<market>`, and `account`; order entry with the same commands as REST. Book channels send a snapshot with a market sequence number, then deltas with consecutive numbers. A client that sees a gap resubscribes.
- **Limits:** token buckets per account (orders) and per IP (all requests), body size limit, request timeouts. All limits are named constants with their reasons.
- **Contracts:** `docs/api/openapi.yaml` and `docs/api/asyncapi.yaml`, checked in CI against the routes the server actually registers.
- **Deferred:** TOTP moves to G6, with the web login flow it protects. Prometheus and OpenTelemetry move to G9 (operations); G4 logs structured JSON lines.

## Read models (Postgres 16, Docker Compose)
- Image: the local `postgres:16-alpine`, pinned by digest; port 55442 (twofold uses 55432).
- Migrations: plain SQL files applied in order by the gateway, each in a transaction, recorded in `schema_migrations`.
- Tables: `fills`, `order_history`, `projector_cursor`. One transaction per projected batch updates the cursor too, so projection is exactly once.

## Reference client (`services/.../client.py`)
REST and WebSocket, request signing, and book maintenance with gap detection and resubscription. It is tested by injecting dropped and reordered messages.

## Fuzzing
`cargo-fuzz` targets: wire command parsing, journal record reading, snapshot restore, and the gateway's request decoding and signature verification. This needs the nightly toolchain and `cargo-fuzz` (my approval to install). The exit criterion is 1 hour per target with no crash.

## Performance (registered in `docs/notes/gateway.md` before the gateway exists)
- P3 as in SPEC §11, plus P3b, informed by the G3 measurement that a durable sync costs about 4 ms.
- P4 as in SPEC §11.

## Units
1. G4 plan and registration (this file, `docs/notes/gateway.md`).
2. Core as a library: handle, queries, broadcast, `durable_seq`; `serve` on top; the crash and differential suites still pass.
3. Client order IDs through engine, journal v2, snapshot v2, wire, protocol, reference engine, and generator.
4. Journal follower and Postgres read models, with Docker Compose and migrations; the rebuild-equals-live test.
5. Gateway: identity, signing, REST, limits, audit log; OpenAPI.
6. WebSocket: market data with sequence numbers, snapshots, and gap handling; private streams; order entry; AsyncAPI; the reference client and its gap tests.
7. Fuzzing (after approval), P3/P3b/P4 measurement, gate review.
