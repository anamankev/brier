# Brier

A prediction-market exchange, named after the Brier score (Brier, 1950), the standard measure of how good probability forecasts are. It is built to the engineering standard of a regulated venue, and run with play money.

Traders buy and sell yes/no contracts on public events, such as "Will October CPI come in above 3.0%?". A contract pays 100 if the event happens and 0 if it does not, so its price (1 to 99) reads as the market's probability. Under that simple product sits the machinery a real exchange needs:

- **Deterministic matching engine.** Price-time priority order book. Replaying the command log reproduces every state hash exactly.
- **Double-entry ledger.** Fully collateralized contracts and pre-trade collateral checks. A conservation invariant is checked after every event: credits issued = balances + escrow.
- **Crash-safe journal.** No order is acknowledged before it is on disk, and recovery is proven by killing the engine under load.
- **APIs.** REST and WebSocket order entry with signed requests and idempotent retries; sequenced market data with gap recovery.
- **Evidence-backed resolution.** Every market has a written rulebook entry and resolves from free public sources (FRED, BLS, NOAA), with the evidence stored.
- **Reference market maker.** Trades only through the public API, quoting around options-implied fair values.
- **Market surveillance.** Detects wash trading, spoofing, marking the close, and insider-like trading. Precision and recall are measured on simulated manipulation.

The full standard is [docs/SPEC.md](docs/SPEC.md). Design decisions are in [docs/adr/](docs/adr/).

## Play money, by design

Credits are granted once at signup. They cannot be bought, sold, withdrawn, redeemed, or transferred, and they have no cash value. Real-money event contracts for US persons require a CFTC-designated contract market, which this project is not. The code contains no deposit, withdrawal, or payment path, and a test will enforce that from G2 onward.

## Status

Built in gates G0 to G9 ([SPEC §15](docs/SPEC.md)). Nothing is released before every gate passes.

| Gate | Scope | State |
|---|---|---|
| G0 | Toolchains, domain types, build, CI, security scans | Done |
| G1 | Matching engine and independent reference engine | Done |
| G2 | Ledger, collateral, settlement | Done |
| G3 | Journal, snapshots, crash recovery | Next |
| G4 | Gateway, APIs, PostgreSQL read models | |
| G5 | Markets, rulebook, resolution | |
| G6 | Web app | |
| G7 | Market maker, simulation harness | |
| G8 | Surveillance and its evaluation | |
| G9 | Release | |

## Measured so far

| What | Result | Evidence |
|---|---|---|
| Matching throughput, one core | 12.3 million commands per second (median of 5 runs) | [performance note](docs/notes/performance.md) |
| Matching latency per command | p99 334 ns, p99.9 500 ns (timer resolution 42 ns) | [performance note](docs/notes/performance.md) |
| Agreement with an independent reference engine | 10 million random commands, 12.4 million events, 0 divergences (matching only) | [differential note](docs/notes/differential.md) |
| The same, with the ledger | 10 million commands with accounts, collateral, and settlement: 11.95 million events, 0 divergences, full invariant checks every 100,000 commands | [differential note](docs/notes/differential.md) |
| Throughput with the ledger in the order path | 2.51 million commands per second; p99 1.4 µs | [performance note](docs/notes/performance.md) |
| Book and ledger invariants (I1-I5) | Checked after every command in 2,560 random streams per CI run | `crates/engine/tests/properties.rs` |

Measured on a 16 GB Apple M5 MacBook Air on AC power. Both performance hypotheses were registered before the engine existed.

## Run the checks

Requirements: [Rust via rustup](https://rustup.rs) (the pinned toolchain installs itself from `rust-toolchain.toml`), [uv](https://docs.astral.sh/uv/), [pre-commit](https://pre-commit.com/), and [gitleaks](https://github.com/gitleaks/gitleaks).

```bash
make setup   # locked Python dependencies and git hooks
make check   # Rust fmt, clippy (warnings are errors), tests; Python ruff, mypy --strict, tests
make audit   # known-vulnerability audit of locked dependencies (needs cargo-audit)
make bench   # the registered engine benchmark
make differential-full SEED=1   # 10 million commands through both engines
make help    # every command
```

## How it is built

| Part | Technology |
|---|---|
| Order path: engine, ledger, journal, gateway | Rust ([ADR-0001](docs/adr/0001-languages-and-architecture.md)) |
| Resolvers, market maker, surveillance, simulation, reference engine | Python 3.12 |
| Web app (G6) | Next.js, TypeScript |
| Read models (G4) | PostgreSQL 16 |

Library code may not panic, unwrap, use floating point, or do unchecked integer arithmetic. These are compiler lints that fail the build, not conventions.

## License

All rights reserved. The code is published to be read, not reused: no license is granted to copy, modify, run, or redistribute it, and contributions are not accepted. See [LICENSE](LICENSE).
