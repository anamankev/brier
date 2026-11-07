# G0 review: foundations

Closed at tag `gate-0` (`639b024`). CI was first verified green on the push of `9721ceb`.

## Exit criteria
| Criterion | Status | Evidence |
|---|---|---|
| `make check` green from a fresh clone | MET | Fresh clone into a scratch directory: `uv sync --project services --locked`, then `make check` passed; no private tooling files in the clone |
| CI green | MET | `check` and `security` workflows green on GitHub from `9721ceb` onward |
| No assistant references in tracked files | MET | `make private-check`; CI job `private-tooling` |

## What was built
- Rust 1.99.1 pinned by `rust-toolchain.toml`; workspace lints deny panics, unwraps, floating point, and unchecked arithmetic in library code.
- `brier-types`: validated price, quantity, credits, IDs, and sequence numbers, with no runtime dependencies; unit and property tests.
- `services/`: Python 3.12 project with ruff, mypy --strict, pytest, and hypothesis. It holds an independent statement of the same units.
- Makefile, pre-commit hooks (gitleaks, private-tooling check, `make check`), CI (`check`, `security` with gitleaks over full history, cargo-audit, pip-audit), Dependabot.
- README, threat model, ADR-0001.

## Deviations
- PostgreSQL moved to G4, and the web scaffold to G6, where something real first uses them. In G0 they would have been placeholders. SPEC §15 records the move.

## Found in review
- The private-tooling check matched its own source: its list of private filenames contained the words it bans. It now uses character classes, and catches a deliberately tracked private file.

## Exam questions (asked 2026-11-06)
1. Python's integers never overflow, yet `Credits` rejects values beyond the 64-bit range. What would go wrong in G1's engine-versus-engine tests if it did not?
2. `Price::complement` carries a local exception to the arithmetic lint. Prove `100 − p` cannot overflow, and explain why silencing the lint with `wrapping_sub` would be worse.
3. Which ledger invariant rests on YES cost + NO cost = 100 per pair? Construct trades where escrow would drift if a trade were misclassified as opening instead of closing.
4. Hooks can be skipped with `--no-verify`. If a secret is committed that way, what still catches it, and what can no tool undo once it is pushed to a public repo?
