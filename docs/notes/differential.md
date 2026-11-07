# Differential test: Rust engine against the reference engine (G1 and G2 gates)

Status: COMPLETE. Result from `3db43b7`.

## Question
Does the Rust matching engine produce exactly the same events as an independently written reference engine (SPEC §3.4) over a long random command stream?

## Criterion (SPEC §15, G1)
10 million random commands with no divergence: every event of every command, and the final book of every market, must be identical.

## Design
- Reference engine: `services/src/brier_services/reference_engine.py`. It's written from `docs/gates/G1-plan.md` with plain lists and O(n) scans, not from the Rust code.
- Stream: `brier_services.differential.generate(seed=1, count=10_000_000)`. It mixes every command and order type, has 4 accounts (so self-trade prevention fires), clusters prices at 45-55 (so orders cross), and includes out-of-range prices and quantities, lifecycle changes, and new markets.
- Coverage check: `test_generated_streams_exercise_every_outcome` requires that the streams reach all 21 event types and rejection and cancel reasons.
- Detection check: `test_the_harness_detects_a_divergence` requires the harness to flag a fake engine at the first command.
- Command: `make differential-full SEED=1`.

## Result
| Commands | Events compared | Trades | Divergences | Wall time | Peak memory |
|---|---|---|---|---|---|
| 10,000,000 | 12,385,249 | 716,607 | **0** | 62.4 s | 517 MB |

Verdict: criterion MET.

## Incident on the way
The first 10-million run (at `69682f1`) was stopped after 12 minutes with no result. The reference engine was at 100% CPU while the Rust engine had used 2.6 s. The stream lists a new market about once per 100 commands, about 100,000 markets in all, and the reference engine searched every market's list on each cancel and amend. That made each such command O(markets), and the run slowed as it went. The fix (`3db43b7`) is a plain dictionary from order ID to market, kept in step at every add and removal, with an error if the index and the book ever disagree. The stream and the engine semantics are unchanged, and every test passed before and after.

## Limitations
- One seed at full length. Five more seeds run at 5,000 commands in every CI run.
- Both engines were written by the same author from the same plan text. A shared misreading of the spec would not show up as a divergence. The rule tests (`crates/engine/tests/rules.rs`), written against SPEC §3.1 examples, are the guard against that.


## G2: with accounts, money, settlement, and kill switches

Result from `496af0f`. The stream now opens accounts (including one never opened), mixes in settlements, `cancel_all`, `halt_all`, oversized orders that test collateral, and a burst of 1,001 orders every million commands that reaches the resting-order limit. Both engines compare every ledger event: `entry`, `reserved`, `account_opened`, `settled`. Both also run their full I1-I5 checks every 100,000 commands and at the end (`--check-every 100000`). The reference engine recomputes collateral from its order list, while the Rust engine uses per-price totals, so the two are independent in method as well as in code.

| Commands | Events compared | Trades | Full invariant checks | Divergences | Wall time | Peak memory |
|---|---|---|---|---|---|---|
| 10,000,000 | 11,950,054 | 162,052 | 100 per engine, all passed | **0** | 384 s | 1.49 GB |

Verdict: criterion MET (SPEC §15, G2: I1-I5 hold over 10 million random commands).

### Incident on the way
The first G2 1-million-command sample (at `443cc4b`) found no divergence but made only 128 trades, so it tested little. Two causes:
- `halt_all` stopped every market, while orders were spread across thousands of markets that a random `set_state` rarely reopened.
- Large resting orders locked most of the busy accounts' cash.

The generator now aims 85% of market-directed commands at the three newest markets, prefers the busiest and newest accounts, and sends 0.5% instead of 2% oversized orders (`496af0f`). The same sample then made 26,122 trades. In the same change, the reference engine's full check went from scanning every account × market pair to only the pairs holding something, cutting the sample from 134 s to 8.8 s.
