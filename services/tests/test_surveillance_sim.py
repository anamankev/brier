"""The surveillance simulation does what docs/notes/surveillance.md §3, §3a, and §3b say."""

from __future__ import annotations

import pytest

from brier_services.sim import surveillance as sim
from brier_services.sim.market import MARKET
from brier_services.surveillance.tape import build, replay

TUNING = range(1, 101)


def test_a_seed_always_gives_the_same_market() -> None:
    assert sim.run(7) == sim.run(7)
    assert sim.run(7).records != sim.run(8).records


def test_the_records_are_what_the_reference_engine_gives_the_commands() -> None:
    s = sim.run(3)
    assert list(replay(c for c, _ in s.records)) == s.records


@pytest.mark.parametrize("kind", ["wash", "spoofer", "closer", "insider"])
def test_each_manipulator_is_present_in_about_half_the_markets(kind: str) -> None:
    present = sum(any(sim.KIND[a] == kind for _, a in sim.run(seed).truth) for seed in TUNING)
    assert 35 <= present <= 65  # binomial(100, 0.5): outside this range has p < 0.005


def test_manipulators_do_what_they_were_injected_to_do() -> None:
    for seed in range(1, 41):
        s = sim.run(seed)
        tape = build(s.records)
        m = tape.markets[MARKET]
        accounts = {a for _, a in s.truth}
        if sim.WASH[0] in accounts:
            assert s.links[sim.WASH[0]] == {sim.WASH[1]}
            assert any({f.buyer, f.seller} == set(sim.WASH) for f in m.fills)
        else:
            assert sim.WASH[0] not in s.links
        if sim.INSIDER in accounts:
            assert tape.account_opened[sim.INSIDER] > (m.open_seq or 0)
            sign = 1 if s.outcome == "yes" else -1
            position = sum(
                (f.qty if f.buyer == sim.INSIDER else -f.qty)
                for f in m.fills
                if sim.INSIDER in (f.buyer, f.seller)
            )
            assert sign * position >= 50
        if sim.SPOOFER in accounts:
            large = [o for o in m.orders.values() if o.account == sim.SPOOFER and o.tif == "gtc"]
            assert large
            assert all(o.qty >= 5 * sim.USUAL for o in large)
        assert s.links[sim.LINKED_NOISE[0]] == {sim.LINKED_NOISE[1]}
        assert m.outcome == s.outcome


def test_the_market_clock_runs_about_five_commands_a_step() -> None:
    rates = [sim.commands_per_step(sim.run(seed)) for seed in range(1, 11)]
    assert all(4 < r < 7 for r in rates)
