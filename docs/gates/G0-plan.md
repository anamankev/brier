# G0 plan: foundations

Reads: SPEC §12, §13, §14. Exit: `make check` green from a fresh clone; CI green; the private-tooling check finds no assistant references in tracked files.

## Deviation from the SPEC §15 row
- PostgreSQL in Docker Compose moves to G4, where the first read model (positions, fills) needs it. Web scaffold moves to G6. In G0 both would be configuration or generated pages with nothing real behind them, which the no-placeholder rule forbids. SPEC §15 is updated to match.

## Units
1. **Toolchain pin.** `rust-toolchain.toml` pins Rust 1.99.1 with clippy and rustfmt, so every machine and CI builds with the same compiler.
2. **Rust workspace and `brier-types` crate.** Zero runtime dependencies. Holds the domain units from SPEC §2.1, each a validated newtype:
   - `Price` (1-99) with `complement` (100 − p).
   - `Quantity` (1 to `MAX_ORDER_QTY`).
   - `Credits` (i64 credit-cents, checked arithmetic only).
   - `Side`, `Outcome`.
   - IDs (`AccountId`, `MarketId`, `OrderId`) and `Seq` (checked successor).
   - Typed errors.

   Workspace lints: `unsafe_code = forbid`; clippy `unwrap_used`, `expect_used`, `panic` and `float_arithmetic` denied in library code. Tests: boundary values, complement involution, the pair identity cost_yes + cost_no = 100 × q (the root of invariant I2), and overflow detection. Property tests use proptest.
3. **Python project (`services/`).** uv, Python 3.12, ruff, mypy --strict, pytest, hypothesis. Holds the independent Python statement of the same units, used by the G1 reference engine. It's written from SPEC §2.1 rather than translated from the Rust, with its own tests.
4. **Quality gates.**
   - Makefile: `setup`, `check`, `test-all`, `audit`, `private-check`.
   - pre-commit hooks: gitleaks, cargo fmt, ruff.
   - `scripts/private_tooling_check.sh`.
   - GitHub Actions: Rust (fmt, clippy -D warnings, test), Python (ruff, mypy, pytest), audits (cargo-audit, pip-audit), gitleaks over full history.
5. **Docs.** README (what it is, the definition of done, the build-gate table, how to run); `docs/threat-model.md` with real entries for what exists (supply chain, secrets, CI) and the planned components' threats from SPEC §13.
6. **Publish.** Create the public GitHub repo and push, then confirm CI is green. This needs your OK (protocol rule 2).

## Risks
- cargo-audit is not installed locally. CI installs it. Locally, `make audit` uses it if present and says so if not; it never passes silently.
- `clippy::float_arithmetic` and `arithmetic_side_effects` are restriction lints. If one fires on correct code, the exception is local and documented, never a crate-wide allow.
