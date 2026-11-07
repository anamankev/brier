"""Tests for the domain units, from the rules in SPEC §2.1-§2.3."""

from __future__ import annotations

import pytest
from hypothesis import given
from hypothesis import strategies as st

from exchange_services.units import (
    I64_MAX,
    I64_MIN,
    MAX_ORDER_QTY,
    PAYOUT_TICKS,
    Credits,
    CreditsOverflowError,
    Price,
    PriceOutOfRangeError,
    Quantity,
    QuantityOutOfRangeError,
    pair_value,
)

prices = st.integers(min_value=1, max_value=99).map(Price)
quantities = st.integers(min_value=1, max_value=MAX_ORDER_QTY).map(Quantity)


@pytest.mark.parametrize("ticks", [1, 50, 99])
def test_price_accepts_the_valid_range(ticks: int) -> None:
    # SPEC §2.1: prices are integer ticks 1 to 99.
    assert Price(ticks).ticks == ticks


@pytest.mark.parametrize("ticks", [-1, 0, 100, 255])
def test_price_rejects_values_outside_the_range(ticks: int) -> None:
    with pytest.raises(PriceOutOfRangeError):
        Price(ticks)


@pytest.mark.parametrize("contracts", [0, MAX_ORDER_QTY + 1, -5])
def test_quantity_rejects_values_outside_the_range(contracts: int) -> None:
    # SPEC §2.1: 1 <= q <= MAX_ORDER_QTY (25,000).
    with pytest.raises(QuantityOutOfRangeError):
        Quantity(contracts)


@pytest.mark.parametrize("bad", [True, False, 1.0, "1"])
def test_units_refuse_non_integers_including_bool(bad: object) -> None:
    with pytest.raises(TypeError):
        Price(bad)  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        Quantity(bad)  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        Credits(bad)  # type: ignore[arg-type]


def test_credits_overflow_at_the_i64_limits_like_the_ledger() -> None:
    # The Rust ledger stores credit-cents in an i64; both sides must fail on the same inputs.
    one = Credits(1)
    with pytest.raises(CreditsOverflowError):
        Credits(I64_MAX) + one
    with pytest.raises(CreditsOverflowError):
        Credits(I64_MIN) - one
    with pytest.raises(CreditsOverflowError):
        -Credits(I64_MIN)
    assert -Credits(I64_MAX) == Credits(-I64_MAX)


def test_costs_at_the_limits() -> None:
    q = Quantity(MAX_ORDER_QTY)
    assert Price(99).yes_cost(q) == Credits(2_475_000)  # 99 x 25,000
    assert Price(1).no_cost(q) == Credits(2_475_000)  # (100 - 1) x 25,000
    assert pair_value(q) == Credits(2_500_000)  # 100 x 25,000


@given(prices)
def test_complement_is_an_involution_summing_to_the_payout(p: Price) -> None:
    assert p.complement().complement() == p
    assert p.ticks + p.complement().ticks == PAYOUT_TICKS


@given(prices, quantities)
def test_yes_and_no_cost_sum_to_the_pair_value(p: Price, q: Quantity) -> None:
    # Two opening sides fund exactly 100 per pair at any price (behind invariant I2, SPEC §4.3).
    assert p.yes_cost(q) + p.no_cost(q) == pair_value(q)
