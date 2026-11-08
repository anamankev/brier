"""Fair-value models (docs/gates/G7-plan.md) on recorded and synthetic data."""

from __future__ import annotations

import datetime as dt
import json
import random
from pathlib import Path

import pytest

from brier_services.marketmaker import fairvalue as fv

FIXTURES = Path(__file__).parent / "fixtures" / "resolution"


def months(path: str) -> dict[tuple[int, int], float]:
    doc = json.loads((FIXTURES / path).read_text())
    out = {}
    for row in doc["Results"]["series"][0]["data"]:
        if row["period"].startswith("M") and row["period"] != "M13" and row["value"] != "-":
            out[(int(row["year"]), int(row["period"][1:]))] = float(row["value"])
    return out


def test_rounding_tails_match_the_rule() -> None:
    # A value certain to be 4.75 rounds to 4.75: "> 4.75" is false, ">= 4.75" is true.
    assert fv._rounded_tail(">", 4.75, 4.75, 0.0, 2) == 0.0
    assert fv._rounded_tail(">=", 4.75, 4.751, 0.0, 2) == 1.0
    # Symmetric uncertainty centred on the threshold: P(rounded >= X) a little above 1/2.
    p = fv._rounded_tail(">=", 4.0, 4.0, 0.1, 1)
    assert 0.5 < p < 0.75
    assert fv._rounded_tail("<", 4.0, 4.0, 0.1, 1) == pytest.approx(1 - p)


def test_the_yield_model_widens_with_time_and_centres_on_the_last_value() -> None:
    rng = random.Random(3)  # noqa: S311 - test data
    start = dt.date(2025, 1, 1)
    level = 4.5
    history = []
    for i in range(400):
        level += rng.gauss(0, 0.05)
        history.append((start + dt.timedelta(days=i), round(level, 2)))
    last_date, last = history[-1]
    near = fv.yield_probability(history, last_date + dt.timedelta(days=3), ">", last + 0.2)
    far = fv.yield_probability(history, last_date + dt.timedelta(days=120), ">", last + 0.2)
    assert near < far < 0.5, "further away, more room to cross"
    assert fv.yield_probability(history, last_date + dt.timedelta(days=30), ">", last - 1.0) > 0.99
    with pytest.raises(ValueError, match="60"):
        fv.yield_probability(history[:20], last_date, ">", 4.0)


def test_cpi_settles_exactly_once_published_and_is_uncertain_before() -> None:
    index = months("bls_cpi_2025_2026.json")
    # September 2026 is published: 336.940 / 324.800 - 1 = 3.74% → 3.7, above 3.0 for certain.
    assert fv.cpi_probability(index, (2026, 9), ">", 3.0) == 1.0
    # November 2026 is two months out: neither certain nor impossible, and reproducible.
    with pytest.raises(ValueError, match="calendar month"):
        fv.cpi_probability(index, (2026, 11), ">", 3.5)  # two years of history is too little


def test_cpi_with_enough_history() -> None:
    rng = random.Random(5)  # noqa: S311 - test data
    index = {}
    level = 250.0
    for y in range(2014, 2027):
        for m in range(1, 13):
            if (y, m) > (2026, 9):
                break
            level *= 1 + rng.gauss(0.0025, 0.002)
            index[(y, m)] = round(level, 3)
    p1 = fv.cpi_probability(index, (2026, 11), ">", 3.0, draws=5_000, seed=9)
    p2 = fv.cpi_probability(index, (2026, 11), ">", 3.0, draws=5_000, seed=9)
    assert p1 == p2, "seeded"
    assert 0.0 <= p1 <= 1.0
    high = fv.cpi_probability(index, (2026, 11), ">", 9.9, draws=2_000)
    low = fv.cpi_probability(index, (2026, 11), ">", -9.9, draws=2_000)
    assert high == 0.0
    assert low == 1.0


def test_unemployment_from_the_recorded_series() -> None:
    rates = months("bls_unemployment_2025_2026.json")
    history = [(dt.date(y, m, 1), v) for (y, m), v in rates.items()]
    last = max(rates)
    p = fv.rate_probability(history, dt.date(2026, 11, 1), ">=", rates[last] + 0.1)
    assert 0.05 < p < 0.6
    assert fv.rate_probability(history, dt.date(2026, 11, 1), ">=", rates[last] - 2) > 0.99


def test_climatology_counts_nearby_days_and_never_says_certain() -> None:
    history = {}
    d = dt.date(1990, 1, 1)
    while d < dt.date(2026, 1, 1):
        history[d] = 45 if d.month == 12 else 70
        d += dt.timedelta(days=1)
    p = fv.climatology_probability(history, dt.date(2026, 12, 15), ">=", 40)
    assert 0.99 < p < 1.0
    assert fv.climatology_probability(history, dt.date(2026, 12, 15), ">=", 50) < 0.01
    with pytest.raises(ValueError, match="climatology"):
        fv.climatology_probability({}, dt.date(2026, 12, 25), ">=", 40)
