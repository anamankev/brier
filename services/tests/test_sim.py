"""The simulation harness: reproducible, and the market maker behaves as registered."""

from __future__ import annotations

from brier_services.marketmaker.quoting import Params
from brier_services.sim.market import CLOSE_BUFFER, STEPS, run


def test_the_same_seed_gives_the_same_market() -> None:
    assert run(7) == run(7)
    assert run(7, signal_sd=1.0) == run(7, signal_sd=1.0)
    assert run(7) != run(8)


def test_the_maker_respects_its_limit_and_pulls_quotes_before_close() -> None:
    p = Params()
    for seed in range(1, 21):
        r = run(seed, signal_sd=0.25, record_path=True)
        assert r.max_inventory <= p.limit + p.size
        assert len(r.inventory_path) == STEPS
        assert r.steps_quoted <= STEPS - CLOSE_BUFFER


def test_a_tighter_limit_binds() -> None:
    tight = Params(limit=50, size=10)
    r = run(3, signal_sd=0.25, params=tight)
    assert r.max_inventory <= tight.limit + tight.size
