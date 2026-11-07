# Every command a developer or CI needs. `make help` lists them.
.DEFAULT_GOAL := help
SHELL := /bin/sh

CARGO ?= cargo
UV_RUN := uv run --project services --locked
PIP_AUDIT_VERSION := 2.10.1

.PHONY: help setup check rust-check py-check private-check test-all audit differential-full bench

SEED ?= 1

help: ## List every command
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "} {printf "  %-14s %s\n", $$1, $$2}'

setup: ## Locked Python dependencies and git hooks (Rust installs from rust-toolchain.toml on first use)
	uv sync --project services --locked
	pre-commit install

check: rust-check py-check private-check ## Lint, types and fast tests for everything (CI runs the same)

rust-check: ## cargo fmt, clippy with warnings as errors, Rust tests
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings
	$(CARGO) test --workspace --locked

py-check: ## ruff, mypy --strict, fast Python tests
	cd services && uv run --locked ruff format --check .
	cd services && uv run --locked ruff check .
	cd services && uv run --locked mypy
	cd services && uv run --locked pytest -q -m "not slow"

private-check: ## No tracked file mentions the private development tooling
	sh scripts/private_tooling_check.sh

differential-full: ## The gate study: 10 million random commands through both engines, full invariant checks every 100,000
	$(CARGO) build --release --locked -p brier-replay
	cd services && uv run --locked python -m brier_services.differential --binary ../target/release/brier-replay --seed $(SEED) --commands 10000000 --check-every 100000

bench: ## The registered P1/P2 engine benchmark (docs/notes/performance.md); plug in and close heavy apps first
	$(CARGO) build --release --locked -p brier-bench
	rustc --version
	sw_vers -productVersion
	pmset -g batt | head -1
	./target/release/brier-bench

test-all: check ## Every test, including slow ones
	$(CARGO) test --workspace --locked -- --include-ignored
	cd services && uv run --locked pytest -q

audit: ## Known-vulnerability audit of locked Rust and Python dependencies
	@command -v cargo-audit >/dev/null 2>&1 || { echo "cargo-audit is not installed: run 'cargo install cargo-audit --locked' (CI always runs it)"; exit 1; }
	$(CARGO) audit
	cd services && uv export --locked --all-groups --no-emit-project --format requirements-txt > ../target/requirements-audit.txt
	uvx --from pip-audit==$(PIP_AUDIT_VERSION) pip-audit --strict --requirement target/requirements-audit.txt
