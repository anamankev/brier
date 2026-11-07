"""Differential tests: the Rust engine against the reference engine (SPEC §3.4)."""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

import pytest

from brier_services.differential import generate, run
from brier_services.reference_engine import ReferenceEngine

ROOT = Path(__file__).resolve().parents[2]
FAST_SEEDS = range(1, 6)
FAST_COMMANDS = 5_000
# Measured on seeds 1-5 at 5,000 commands (commit after 0c33e86): trades 0.046-0.064 and
# accepted orders 0.442-0.496 per command. The floors are about half the lowest.
LIVE_TRADES_PER_COMMAND = 0.02
LIVE_ACCEPTED_PER_COMMAND = 0.25


@pytest.fixture(scope="session")
def replay_binary() -> Path:
    cargo = shutil.which("cargo") or str(Path.home() / ".cargo" / "bin" / "cargo")
    if not Path(cargo).exists():
        pytest.skip("cargo not found: differential tests need the Rust toolchain")
    subprocess.run(  # noqa: S603 - fixed arguments
        [cargo, "build", "--release", "--locked", "-p", "brier-replay"], cwd=ROOT, check=True
    )
    return ROOT / "target" / "release" / "brier-replay"


def test_generated_streams_exercise_every_outcome() -> None:
    # A differential test is only as good as its inputs: these streams must reach every event
    # and every rejection and cancel reason, or agreement between the engines proves little.
    # Entries are also checked by kind below.
    seen: set[tuple[str, str]] = set()
    kinds: set[str] = set()
    for seed in FAST_SEEDS:
        engine = ReferenceEngine()
        for cmd in generate(seed, FAST_COMMANDS):
            for e in engine.apply(cmd):
                seen.add((e["ev"], e.get("reason", "")))
                if e["ev"] == "entry":
                    kinds.add(e["kind"])
    expected = {
        ("market_created", ""),
        ("market_state", ""),
        ("accepted", ""),
        ("trade", ""),
        ("rested", ""),
        ("amended", ""),
        ("cancelled", "user"),
        ("cancelled", "ioc_remainder"),
        ("cancelled", "self_trade"),
        ("cancelled", "market_closed"),
        ("rejected", "unknown_market"),
        ("rejected", "market_exists"),
        ("rejected", "market_not_open"),
        ("rejected", "invalid_transition"),
        ("rejected", "invalid_order"),
        ("rejected", "invalid_price"),
        ("rejected", "invalid_quantity"),
        ("rejected", "post_only_would_cross"),
        ("rejected", "fok_unfillable"),
        ("rejected", "unknown_order"),
        ("rejected", "invalid_amend"),
        # G2: accounts, money, settlement, kill switches
        ("account_opened", ""),
        ("entry", ""),
        ("reserved", ""),
        ("settled", ""),
        ("cancelled", "kill_switch"),
        ("rejected", "unknown_account"),
        ("rejected", "account_exists"),
        ("rejected", "too_many_orders"),
        ("rejected", "insufficient_collateral"),
        ("duplicate", ""),  # G4: idempotent order entry
    }
    assert expected - seen == set()
    assert kinds == {"grant", "trade", "settle"}


def test_listings_and_settlements_carry_hashes_mostly() -> None:
    hashed = {"create_market": [0, 0], "settle": [0, 0]}
    for cmd in generate(FAST_SEEDS[0], FAST_COMMANDS):
        if cmd["cmd"] in hashed:
            has = "rules_hash" in cmd or "evidence_hash" in cmd
            hashed[cmd["cmd"]][has] += 1
    for kind, (without, with_hash) in hashed.items():
        assert without > 0, kind
        assert with_hash > without, kind


def test_generation_is_deterministic_in_the_seed() -> None:
    assert list(generate(7, 2_000)) == list(generate(7, 2_000))
    assert list(generate(7, 2_000)) != list(generate(8, 2_000))


@pytest.mark.differential
@pytest.mark.parametrize("seed", FAST_SEEDS)
def test_rust_engine_matches_the_reference_engine(replay_binary: Path, seed: int) -> None:
    report = run(replay_binary, seed, FAST_COMMANDS, check_every=500)
    assert report.divergence is None, report.divergence
    # Liveness floors: a stream that mostly bounces off rejections proves little even when
    # the engines agree (docs/notes/differential.md, G2 incidents). Floors are set well below
    # the measured rates so they catch a degenerate generator, not normal seed variation.
    assert report.trades >= LIVE_TRADES_PER_COMMAND * FAST_COMMANDS
    assert report.accepted >= LIVE_ACCEPTED_PER_COMMAND * FAST_COMMANDS


@pytest.mark.differential
def test_the_harness_detects_a_divergence() -> None:
    # `cat` echoes each command back: same seq, wrong content, so the very first command
    # must be reported as a divergence.
    cat = shutil.which("cat")
    assert cat is not None
    report = run(Path(cat), seed=1, count=50)
    assert report.divergence is not None
    assert report.divergence.seq == 1
