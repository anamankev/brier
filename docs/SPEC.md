# Brier: Product and Engineering Specification

Written 6 November 2026. This is the complete standard for the product. It is built once, completely, and released when every item in §0 is met. Section numbers are stable; cite them in commits, ADRs, and notes.

---

## 0. Mandate and definition of done

Build a prediction-market exchange whose core would pass review at a regulated venue: every order is matched by a deterministic engine, every credit is accounted for by a double-entry ledger, every acknowledged order survives a crash, every market resolves from recorded public evidence, and manipulation is detected and measured. It runs on play money (§1.2), so it can be built, run, and shown at $0 and within the law.

**Done means all of the following, each with evidence:**
1. **Matching engine:** price-time priority central limit order book for binary contracts, with the order types in §3. Tested by property tests and by differential testing against an independent reference engine. Throughput and latency measured and published (§11).
2. **Ledger and risk:** fully collateralized binary contracts, double-entry accounting, pre-trade collateral reservation. The invariants (§4.3) are checked after every command in the test suites and periodically in long differential runs, over millions of random commands.
3. **Durability:** an append-only journal with checksums, snapshots, and replay. A crash test (kill -9 under load, repeatedly) loses no acknowledged order and recovers to the exact replayed state hash.
4. **APIs:** REST and WebSocket order entry with idempotent client order IDs and Ed25519-signed requests; WebSocket market data with per-market sequence numbers, snapshots, and gap recovery. Protocol parsers fuzzed.
5. **Markets that resolve correctly:** every market has a written rulebook entry (§7). Automated resolvers fetch public sources (FRED, BLS, NOAA), record evidence, and settle after a two-step confirmation.
6. **Liquidity:** a reference market maker trading through the public API only, quoting around a model fair value (twofold's options-implied probabilities for index markets) with inventory control (§8).
7. **Surveillance:** detection of wash trading, spoofing and layering, marking the close, and insider-like trading. Evaluated by a pre-registered study on simulated markets with injected manipulators, reporting precision and recall per typology (§9).
8. **Web app:** market list, order-book ladder, price chart, trade ticket, positions and P&L, market rules with resolution evidence, forecaster scorecard and leaderboards (§7), admin console, surveillance case view. Accessible (WCAG 2.2 AA).
9. **Operations:** one-command local stack (Docker Compose), structured logs, Prometheus metrics, OpenTelemetry traces, runbooks for halt, recovery, and resolution disputes; threat model.
10. **Release:** public repo, README with architecture, invariants, benchmark results, and an honest comparison with industry leaders (§16).

The exact engine behavior (commands, check order, events, wire format) lives in `docs/protocol.md`, which the code and both engines' tests are held to.

## 1. Scope and boundaries

### 1.1 In scope
Binary (yes/no) event contracts on economics, weather, science, rates, and sports events (sports added 8 November 2026, G10); the components in §0; an agent-based simulation harness for load testing and surveillance evaluation (§10).

### 1.2 Play-money boundary (enforced in code and terms)
- Each account receives a one-time grant of credits (`SIGNUP_GRANT` = 1,000,000 credit-cents, i.e. 10,000 credits). Admins may run documented, equal-for-all re-grants; no individual top-ups.
- Credits cannot be bought, sold, withdrawn, redeemed, or transferred between users. They have no cash value and there are no prizes. This keeps the product outside gambling, sweepstakes, and money-transmission rules. Real-money operation for US persons would require CFTC designation as a contract market and a clearing arrangement; that is out of scope (§17).
- The code contains no deposit, withdrawal, payment, or wallet path. A test asserts that no ledger entry type exists that moves credits between two user accounts outside matched trades and settlement.

### 1.3 Out of scope
Real money; KYC; scalar and multi-outcome markets (each outcome of a multi-outcome event is listed as its own binary market); margin or leverage; FIX protocol gateway (REST and WebSocket only; see §16 for the gap); mobile apps; users under 18.

## 2. Contract model

### 2.1 Units
- Price: integer ticks, 1 to 99, meaning 1 to 99 credit-cents per contract. Tick size 1.
- Payout: a YES contract pays 100 credit-cents if the market resolves YES, else 0. A NO contract pays the reverse.
- Quantity: integer contracts, 1 ≤ q ≤ `MAX_ORDER_QTY` (25,000; a fat-finger limit).
- Money: integer credit-cents (`i64`). No floating point in the engine, ledger, risk, or settlement.

### 2.2 One book per market
Every market has a single book quoted in YES terms. "Buy NO at q" is the same economic order as "sell YES at 100 − q", so both enter the same book. The API accepts either form and normalizes on entry. The book is never crossed after matching: best bid < best ask, or one side is empty.

### 2.3 Positions and netting
An account's position in a market is a signed integer x: x > 0 means x YES contracts, x < 0 means |x| NO contracts. A trade that moves x toward zero closes contracts; one that moves it away from zero opens contracts.
- Open a YES at price p: pay p. Open a NO at YES-price p: pay 100 − p.
- A trade between two opening sides creates a pair: 1 YES + 1 NO, backed by 100 credit-cents in market escrow.
- A trade between two closing sides destroys a pair and releases 100 from escrow: the YES seller receives p, the NO holder receives 100 − p.
- A trade between an opening and a closing side transfers an existing contract; escrow is unchanged and the closer receives the opener's payment.

### 2.4 Settlement
- YES: each YES contract receives 100 from escrow; NO receives 0. NO resolution is the reverse.
- Void (the rulebook's stated conditions, e.g. the source never publishes): every contract on both sides receives 50. This pays out exactly the escrow, requires no clawback of realized gains, and treats both sides symmetrically. The trade-off is recorded in ADR-0002.
- After settlement, escrow for the market is exactly zero (tested).

## 3. Matching engine

### 3.1 Semantics
- Price-time priority: better price first; at equal price, earlier sequence number first.
- Trades execute at the resting order's price.
- Order types: limit GTC; IOC (fill what crosses, cancel the rest); FOK (fill completely or not at all); post-only (rejected if it would cross); market orders are IOC with a mandatory protection price.
- Cancel; amend down (quantity decrease keeps priority); any price change or quantity increase is cancel-and-replace and loses priority.
- Self-trade prevention: an incoming order that would match the same account's resting order cancels the incoming remainder (`STP = CancelTaker`). Wash-trade surveillance (§9) covers linked accounts.
- Market states: `PreOpen` (accepts cancels only), `Open`, `Halted` (cancels only), `Closed` (no orders), `Resolved`, `Voided`. Transitions are commands in the log.

### 3.2 Determinism
The engine is a pure function: `(state, command) → (state', events)`. A sequencer assigns a global, gap-free sequence number to each command before it reaches the engine. The engine reads no clock (the sequencer will stamp commands with time in G3/G4, and the engine only echoes it), uses no randomness, and never lets the iteration order of an unordered collection reach its output. `Engine::state_hash()` is a BLAKE3 hash over a canonical serialization of the whole state. Snapshots (G3) record it, and replaying the journal from empty must reproduce it exactly. Replay determinism over in-memory command streams is property-tested now.

### 3.3 Data structures (performance design, measured in §11)
Per market: two price-indexed arrays of 99 levels each (index = tick), each level a FIFO of order handles; best-bid and best-ask indexes; an order arena with free-list reuse; an order-ID map. With only 99 ticks, every price lookup is O(1) and no tree is needed. ADR-0003 records this choice against a BTreeMap book.

### 3.4 Reference engine (oracle)
A second, deliberately simple engine (Python, list-based, O(n) per operation) implements §2-§3 from this text alone. Differential tests feed both engines identical random command streams and require identical event streams. Neither engine's code may be derived from the other.

## 4. Ledger and pre-trade risk

### 4.1 Accounts
Per user: `cash`, the only user ledger balance. Collateral is a lock on cash, not a ledger account: `reserved(a)` = Σ over markets of `reserve(a, m)` (§4.4), and `available(a)` = cash − reserved (ADR-0005). Per market: `escrow`. System: `issuance` (negative balance equal to all credits ever granted).

### 4.2 Double-entry
Every movement is a journal entry whose legs sum to zero. Entry types: `Grant` (issuance → cash), `Trade` (buyer cash, seller cash, escrow; one entry covers pair creation, transfer, and pair destruction), `Settle` (escrow → cash, for YES, NO, and void payouts alike). No other type exists (§1.2 test). Reservations are lock changes, reported as `reserved` events, not ledger entries.

### 4.3 Invariants
Where they are checked:
- **Tests:** `Engine::check_invariants` runs all five after every command in the rule and property tests.
- **The engine itself, after every command:** I3 for every account the command touched, the balance of every ledger entry as it is built, and zero escrow after every settlement.
- **Full check elsewhere:** at every snapshot (G3), and every N commands in replay runs (`brier-replay --check-every N`).
- I1 conservation: Σ cash + Σ escrow + issuance = 0 (equivalently Σ available + Σ reserved + Σ escrow + issuance = 0).
- I2 escrow: escrow(m) = 100 × open interest(m), where open interest = Σ positive positions = Σ |negative positions|.
- I3 no negative balances: cash ≥ reserved ≥ 0 for every user, so available ≥ 0.
- I4 reservations: an account's reserved total equals Σ over markets of `reserve(a, m)`, recomputed from its resting orders and position by the worst-case rule in §4.4.
- I5 book: never crossed; every resting order has quantity > 0; order count matches the ID map.
A violated invariant halts the engine with `EngineError::Corrupt` naming it. `Engine::halted()` reports the sequence number of the failing command, and every later command is refused with `EngineError::Halted`, since the failing command may have changed state part-way. Recovery is a restart from the journal. Nothing ever adjusts a balance to restore an invariant.

### 4.4 Pre-trade checks (in order; each rejection has a typed reason)
Market state; price in [1, 99]; quantity in [1, `MAX_ORDER_QTY`]; per-account resting-order limit; per-account message rate limit (enforced at the gateway); collateral: counting the incoming order as if it rests in full, `reserve(a, m)` may grow by at most `available(a)`. `reserve(a, m)` charges the price of each resting buy unit and (100 − price) of each resting sell unit, except that the account's held contracts (NO for buys, YES for sells) cover the cheapest units on that side, since closing needs no cash. The exact rule and the order of checks are in `docs/protocol.md` §3 and §5; the proof of sufficiency is in ADR-0005.

### 4.5 Kill switches
Cancel-all per account, per market, or everywhere, and a global halt of every open market. Each is a sequenced command; the gateway (G4) audit-logs it with actor and reason.

## 5. Journal, snapshots, recovery
- Journal: append-only segments. Each record = length, CRC32C, sequence number, command bytes. A torn or corrupt tail is detected on open and truncated to the last valid record, and the truncation is logged.
- Group commit: commands are batched and synced before any is acknowledged. On macOS, durability requires `fcntl(F_FULLFSYNC)`; plain `fsync` does not flush the drive cache (Apple fsync(2) man page). On Linux, `fdatasync`. ADR-0004.
- Snapshots: every `SNAPSHOT_INTERVAL` events, the full state is serialized with its sequence number and state hash. Recovery = latest valid snapshot + replay of later records.
- Crash test (G3 gate): a harness drives load, kills the engine process with SIGKILL at random points (at least 1,000 kills), restarts, and checks: every acknowledged order is present, invariants hold, and the recovered state hash equals a clean replay's hash.

## 6. Gateway and APIs
- REST: accounts, markets, orders (place, cancel, amend), positions, fills, history.
- WebSocket order entry: same commands, lower overhead.
- WebSocket market data: per-market channels (book snapshot, then incremental level updates, trades, ticker), each message carrying the market's sequence number. A client that sees a gap requests a fresh snapshot. A public reference client implements this and is tested against injected gaps.
- Authentication: browser sessions (Argon2id password hashing, optional TOTP); bot access by Ed25519 key pairs. The client signs (timestamp, method, path, body hash); the server stores only the public key and rejects stale timestamps (`MAX_CLOCK_SKEW`) and replayed signatures.
- Idempotency: every order carries a client order ID, unique per account; a retry returns the original result.
- Rate limits: token buckets per account and per IP, with stated limits.
- Every endpoint, message, and error code is documented in an OpenAPI file and an AsyncAPI file kept in the repo.

## 7. Markets, rulebook, resolution
- Each market has a rulebook entry before listing: question; exact YES condition; resolution source (dataset, series ID, URL); release or observation time and timezone; revision policy (first release counts, later revisions ignored, unless stated); void conditions; close time.
- Sources must be free, public, and authoritative: FRED (St. Louis Fed; free API key), BLS public API, NOAA climate data. Terms for each source are recorded in `docs/data-licenses.md` before use (O3).
- Resolution: a resolver job fetches the source after the release time and stores evidence (request, raw response hash, parsed value, retrieval time). It proposes an outcome, and an admin confirms it in a second step; settlement then runs as a sequenced command. A disputed resolution can be re-run within `DISPUTE_WINDOW` before settlement.
- **Forecaster scores** (added 7 November 2026; shown in G6). Play money cannot make trading matter; being measurably right can. Any user may record a probability (1 to 99 percent) on any open market and change it until close; trading is not required.
  - When the market resolves YES or NO, each forecast is scored with the Brier score, (forecast − outcome)², and so is the market's own price (the last trade, or the midpoint before the first trade).
  - Skill is the market's score minus the user's, averaged over the market's whole open period. Time before a user's first forecast counts as zero skill, so forecasting late, with more information, earns nothing extra; forecasting early and right earns the most. The open period is sampled once a minute (G6 plan: journal commands carry no timestamps). A minute in which the market has no price at all (no trade yet and an empty book) is not sampled; until 8 November 2026 such minutes were recorded at 50%, an invented price (G9).
  - Voided markets are not scored. Integer arithmetic, in units of 1/10,000.
  - Leaderboards rank by average skill over at least `MIN_SCORED_MARKETS` (10) resolved markets, and separately by settled trading profit.
  - Forecasts are timestamped by the server, cannot be backdated, and are frozen at close.
- Launch catalogue examples: monthly CPI year-on-year above a threshold; the FOMC target upper bound after a meeting; the unemployment rate; the 10-year Treasury yield above a level on a date (replacing the S&P 500 close, whose FRED series is licensed from S&P Dow Jones Indices; `docs/data-licenses.md`, O6); the daily maximum temperature at a named station.

## 8. Reference market maker
- A Python service that trades only through the public API, with the same limits as any user.
- Fair value: for S&P 500 markets, the options-implied probability from twofold's European pipeline (XSP options; ADR-0010 there), consumed as a pinned dependency. For other markets, a documented, simple model or no quotes at all.
- Quoting: inventory-skewed quotes after Avellaneda and Stoikov (2008), with a spread floor, position limits, and quotes pulled when fair value is stale or the market is near close.
- Reported: time quoting, average spread, inventory path, P&L by market. Its performance is measured, not assumed.

## 9. Surveillance
- Input: the engine's event stream (orders, cancels, trades, resolutions) plus account metadata (creation time, linked-account signals such as shared devices or IPs).
- Typologies and first-pass detectors:
  - Wash trading: trades between the same owner or linked accounts.
  - Spoofing and layering: large orders near the touch, cancelled before execution, around the account's own fills on the other side (order-to-trade ratio, time-to-cancel, size relative to depth).
  - Marking the close: aggressive trades in the final window before close or resolution that move the price.
  - Insider-like trading: large directional positions shortly before a source release that turn out correct, especially from new or linked accounts.
- Alerts open cases with an evidence timeline (orders, book state, fills, P&L) in the admin UI.
- Evaluation study (pre-registered in `docs/notes/surveillance.md` before running): simulated markets (§10) with injected manipulator agents as ground truth. Precision and recall per typology at stated alert thresholds, with confidence intervals over simulation seeds. Thresholds are chosen on a tuning set of seeds and reported on a separate set.
- Legal background: spoofing and wash trading are prohibited on US futures markets (Commodity Exchange Act §4c), and in 2026 the CFTC brought its first insider-trading enforcement actions in prediction markets. Here the detectors are product features, not legal determinations.

## 10. Simulation harness
Agent-based markets for load and surveillance testing: noise traders, informed traders (with signal quality as a parameter), the reference market maker, and manipulators (one agent type per typology). Fully seeded and reproducible. Simulated activity runs only in the harness and benchmarks, never mixed with real users' markets.

## 11. Performance targets (hypotheses, measured on the 16 GB M5 MacBook Air)
Registered in `docs/notes/performance.md` before measuring; results committed with hardware and OS notes.
- P1 engine, in-process, single core, mixed workload (60% place, 35% cancel, 5% aggressive): ≥ 1,000,000 commands/s.
- P2 engine in-process latency per command: p99 ≤ 5 µs, p99.9 ≤ 20 µs.
- P3 order-to-acknowledgment through the gateway on localhost, with group-commit F_FULLFSYNC: p99 ≤ 5 ms at 10,000 orders/s.
- P4 market-data fan-out: 1,000 WebSocket subscribers on one market, update p99 ≤ 10 ms from engine event.
- P5 recovery: replay of 10 million events in ≤ 30 s.
A missed target is reported as missed, with the profile that explains it.

## 12. Architecture and stack
- Processes: gateway (Rust, axum/tokio) → sequencer and engine (Rust, single-threaded per shard; all markets on one shard initially) → journal; event bus to projectors (PostgreSQL read models), market-data publisher, surveillance, resolvers.
- Languages: Rust for everything on the order path (no garbage-collection pauses, memory safety without a runtime). Python for resolvers, market maker, surveillance analytics, simulation, and the reference engine. TypeScript/Next.js for the web. ADR-0001.
- Storage: the journal is the source of truth. PostgreSQL 16 holds read models (positions, fills, history), reference data (markets, rulebook, users), and surveillance cases. Read models can be rebuilt from the journal (tested).
- Observability: structured JSON logs, Prometheus metrics, OpenTelemetry traces (same approach as twofold ADR-0003).

## 13. Security
Threat model per component (`docs/threat-model.md`): order forgery (signature verification), replay (timestamps and nonces), API abuse (rate limits), account takeover (Argon2id, TOTP), multi-accounting (surveillance linkage; credits have no value, which limits the incentive), admin misuse (two-step resolution, audit log), supply chain (locked dependencies, `cargo audit`, `pip-audit`, `npm audit` in CI), secrets (environment only; gitleaks in pre-commit and CI).

## 14. Testing
- Rust: unit tests; proptest for invariants I1-I5 over random command streams; differential tests against the reference engine (§3.4); cargo-fuzz targets for the journal reader and every protocol parser; criterion benchmarks.
- Python: pytest, hypothesis, mypy --strict.
- Integration: the gateway against a real journal and PostgreSQL in Docker; market-data gap recovery; crash test (§5).
- End to end: Playwright and axe against the running stack.
- Markers or features separate fast checks (every push) from slow ones (nightly or manual): crash test, fuzzing campaigns, full benchmarks.

## 15. Gates and exit criteria

| Gate | Read | Deliverables | Exit criteria |
|---|---|---|---|
| G0 Foundations | §12, §13, §14 | Rust toolchain; Cargo workspace with the §2.1 domain types; Python project with tooling; Makefile; CI (fmt, clippy, tests, audits, gitleaks); ADR-0001; threat model. (PostgreSQL arrives in G4 and the web scaffold in G6, when something real uses them.) | `make check` green from a fresh clone; CI green; the private-tooling check finds no assistant references in tracked files |
| G1 Matching engine | §2, §3 | Book, order types, STP, market states, state hash; reference engine; differential and property tests; criterion benchmarks | 10 million random commands with no divergence from the reference engine; P1 and P2 measured |
| G2 Ledger and risk | §2.3-§2.4, §4 | Double-entry ledger, reservations, netting, settlement, void at 50, kill switches | I1-I5 hold over 10 million random commands; settlement zeroes escrow; the §1.2 test passes |
| G3 Journal and recovery | §5 | Journal, group commit, snapshots, replay, crash harness | 1,000 SIGKILLs with zero lost acknowledged orders and matching state hashes; P5 measured |
| G4 Gateway and APIs | §6, §12 | Docker Compose with PostgreSQL 16 and the first read models; REST, WebSocket order entry and market data, auth, idempotency, rate limits, OpenAPI and AsyncAPI, reference client, fuzzing | Fuzzers run 1 hour each with no crash; gap recovery tested; P3 and P4 measured |
| G5 Markets and resolution | §7 | Rulebook, admin console API, FRED/BLS/NOAA resolvers with evidence, two-step settlement | Every launch-catalogue market resolves end to end from a recorded source response |
| G6 Web app | §0 item 8, §7 | Next.js scaffold; trading UI, portfolio, rules and evidence pages, forecaster scorecard and leaderboards, admin console | Playwright flows and axe checks pass |
| G7 Market maker and simulation | §8, §10 | Reference market maker; agent-based harness | Market maker quotes through the public API in simulation and on live markets; metrics reported |
| G8 Surveillance | §9 | Detectors, case management, pre-registered evaluation | Hypotheses commit predates results; precision and recall reported per typology |
| G9 Live catalogue | §0 item 8, §7, §8 | Events and recurring series from public sources, automatic listing and machine-checked resolution, quotes on every modelled market, a Polymarket-grade browsing and trading interface (`docs/gates/G9-plan.md`; added 8 November 2026) | A 72-hour live run with at least 40 open markets in 5 categories, 20 settling within 72 hours of listing (corrected before the run; G9 plan), all with evidence; Playwright and axe pass |
| G10 Public launch | §0, §1, §16 | Public accounts (terms, 18+, recovery codes, deletion, export), daily allowance, profiles, notifications, more series and sports, deployment on a free VM with backups, runbooks | `docs/gates/G10-plan.md` exit criteria |

## 16. Industry benchmark (what a reviewer will compare against)

| Dimension | Kalshi | Polymarket | This project |
|---|---|---|---|
| Regulation | CFTC-designated contract market since 2020 | US entity via the QCEX acquisition; international venue settles on Polygon | None; play money by design |
| Matching | Central limit order book | Hybrid: off-chain order book, on-chain settlement; order book rebuilt April 2026 | Deterministic CLOB with a published replay hash and benchmarks |
| Scale | Reported over $2B annualized revenue; valuations of $22B (May 2026) and later reportedly up to $40B | Roughly $9-10B monthly volume (spring 2026) | Simulation load only; no real users yet |
| Liquidity | Professional market makers | Professional and retail market makers | One reference market maker |
| Surveillance | Required of a regulated venue | Not publicly documented for the international venue | Detectors with published precision and recall |
| Order-entry protocols | REST, WebSocket; FIX unverified (O1) | REST, WebSocket | REST, WebSocket; no FIX |

Where this project can be better than the leaders: open, verifiable engineering (published invariants, replay hashes, crash tests, surveillance metrics); every market's rules and settlement evidence pinned by hash in the journal; and a scored record of each user's forecasting skill against the market (§7). Where it cannot compete: license, real money, users, liquidity. Figures in this table come from press and analyst reports gathered on 6 November 2026; sources disagree, so they are context, not claims the product depends on.

## 17. Non-claims
- Play money changes trader behavior; prices here are not forecasts of the quality of real-money markets.
- Surveillance metrics are measured on simulated manipulation; real manipulators adapt.
- One machine's benchmarks are not evidence of production capacity.
- Nothing here is a licensed venue, investment advice, or an offer of event contracts.
- The path to real money (partnering with a designated contract market, or applying for designation) is not designed here.

## 18. References
- Brier, G. W. (1950). Verification of forecasts expressed in terms of probability. *Monthly Weather Review* 78(1), 1-3. (The product's name.)
- Avellaneda, M., Stoikov, S. (2008). High-frequency trading in a limit order book. *Quantitative Finance* 8(3), 217-224.
- Fowler, M. (2011). The LMAX Architecture. martinfowler.com. (Single-threaded, event-sourced matching design.)
- Harris, L. (2003). *Trading and Exchanges: Market Microstructure for Practitioners*. Oxford University Press.
- Commodity Exchange Act §5(d) (core principles for designated contract markets) and §4c (prohibited trading practices), 7 U.S.C.
- Apple, fsync(2) and fcntl(2) man pages (F_FULLFSYNC).

## 19. Open items
- O1: Kalshi and Polymarket API features (FIX availability, rate limits, order types) from their official docs, for §16.
- O2: Whether twofold's European pipeline is ready to serve fair values live, or only in studies (G7).
- O3: Terms of use for FRED, BLS, and NOAA data as resolution sources (G5). Resolved 7 November 2026 in `docs/data-licenses.md`: BLS and NOAA public domain, raw responses committable; FRED usable for public-domain series only, raw responses kept local; S&P 500 excluded.
- O6: A source for index markets (S&P 500) whose terms allow this use; until then no index markets, which affects the market maker's fair value in §8 (decided in G7).
- O4: The Rust crate choices (serialization, hashing, WebSocket) and their licenses (G0).
- O5 (closed 8 November 2026): the owner chose an Oracle Cloud Always Free VM. The account asks for a card to verify identity and does not charge it (G10).
