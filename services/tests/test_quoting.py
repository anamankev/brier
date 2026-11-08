"""The market maker's quoting rules (docs/notes/market-maker.md §3)."""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import strategies as st

from brier_services.marketmaker.quoting import Params, quote

P = Params()
probabilities = st.floats(min_value=0.005, max_value=0.995)
inventories = st.integers(min_value=-600, max_value=600)
taus = st.floats(min_value=0.0, max_value=1.0)


@given(probabilities, inventories, taus)
def test_quotes_are_valid_ticks_with_bid_below_ask(p: float, q: int, tau: float) -> None:
    x = quote(p, q, tau)
    assert x.bid is None or 1 <= x.bid <= 98
    assert x.ask is None or 2 <= x.ask <= 99
    if x.bid is not None and x.ask is not None:
        assert x.bid < x.ask
    assert x.size == P.size


@given(probabilities, taus)
def test_the_spread_never_falls_below_the_floor(p: float, tau: float) -> None:
    x = quote(p, 0, tau)
    assert x.bid is not None
    assert x.ask is not None
    if x.bid >= 5 and x.ask <= 95:  # away from the clamped edges
        assert x.ask - x.bid >= 2 * round(100 * P.floor)


@given(
    st.floats(min_value=0.1, max_value=0.9),
    st.integers(min_value=1, max_value=499),
    st.floats(min_value=0.2, max_value=1.0),
)
def test_quotes_lean_against_inventory(p: float, q: int, tau: float) -> None:
    flat, long, short = quote(p, 0, tau), quote(p, q, tau), quote(p, -q, tau)
    bids = [x.bid for x in (long, flat, short)]
    asks = [x.ask for x in (long, flat, short)]
    assert None not in bids
    assert None not in asks
    assert bids == sorted(bids)  # type: ignore[type-var]
    assert asks == sorted(asks)  # type: ignore[type-var]


def test_at_the_limit_only_the_reducing_side_is_quoted() -> None:
    assert quote(0.5, P.limit, 1.0).bid is None
    assert quote(0.5, P.limit, 1.0).ask is not None
    assert quote(0.5, -P.limit, 1.0).ask is None
    assert quote(0.5, -P.limit, 1.0).bid is not None


def test_a_worked_example() -> None:
    # p = 0.6, flat, half the time left: variance 0.96, half-spread 0.015 + 0.02 * 0.96 * 0.5
    # = 0.0246, so bid floor(57.54) = 57 and ask ceil(62.46) = 63.
    assert quote(0.6, 0, 0.5) == quote(0.6, 0, 0.5)
    x = quote(0.6, 0, 0.5)
    assert (x.bid, x.ask) == (57, 63)
    # Long 250 (q_hat = 0.5): reservation 0.6 - 0.08 * 0.5 * 0.96 * 0.5 = 0.5808: bid 55, ask 61.
    y = quote(0.6, 250, 0.5)
    assert (y.bid, y.ask) == (55, 61)


@pytest.mark.parametrize("p", [0.0, 1.0, float("nan")])
def test_fair_value_must_be_a_probability(p: float) -> None:
    with pytest.raises(ValueError, match="fair probability"):
        quote(p, 0, 1.0)
