"""The catalogue's pricing models: exact values, probabilities that sum to one, and ranges that
match the templates."""

from __future__ import annotations

import datetime as dt
import math
from pathlib import Path

import pytest

from brier_services.catalog import models as m
from brier_services.catalog import templates


def test_normal_ranges_sum_to_one_and_use_half_degree_edges() -> None:
    ranges = m.temperature_ranges(61)
    probs = m.normal_probs(61.0, 3.0, ranges)
    assert sum(probs) == pytest.approx(1.0)
    middle = m.normal_cdf(61.5, 61, 3) - m.normal_cdf(59.5, 61, 3)
    assert probs[3] == pytest.approx(middle)
    assert ranges[3] == (60, 61)


def test_poisson_and_negative_binomial() -> None:
    p = m.poisson_probs(2.0, [(None, 2), (3, None)])
    assert p[0] == pytest.approx(math.exp(-2) * (1 + 2 + 2))
    assert sum(p) == pytest.approx(1.0)
    same = m.negative_binomial_probs(4.0, 4.0, m.QUAKE_RANGES)
    assert same == pytest.approx(m.poisson_probs(4.0, m.QUAKE_RANGES))
    wide = m.negative_binomial_probs(4.0, 9.0, m.QUAKE_RANGES)
    assert sum(wide) == pytest.approx(1.0)
    assert wide[0] > same[0], "overdispersion fattens the low tail"
    assert wide[3] > same[3], "and the high tail"


def test_empirical_brier_and_outcomes() -> None:
    probs = m.empirical_probs([1, 2, 3, 3, 8], m.QUAKE_RANGES, pseudo=1.0)
    assert probs == pytest.approx([3 / 9, 3 / 9, 1 / 9, 2 / 9])
    assert m.outcome_index(8, m.QUAKE_RANGES) == 3
    assert m.brier([0, 1, 0, 0], 1) == 0
    assert m.brier([0.25] * 4, 0) == pytest.approx(0.75)


def test_model_ranges_are_the_templates_ranges() -> None:
    for forecast in (12, 61, 104):
        rulebooks = templates.temperature(templates.CITIES[0], dt.date(2026, 11, 10), forecast)
        listed: list[m.Range] = []
        for rb in rulebooks:
            c = rb["condition"]
            match c["op"]:
                case "<=":
                    listed.append((None, int(c["threshold"])))
                case ">=":
                    listed.append((int(c["threshold"]), None))
                case _:
                    listed.append((int(c["threshold"]), int(c["upper"])))
        assert listed == m.temperature_ranges(forecast)


def test_storm_probabilities_from_the_swpc_forecast() -> None:
    text = (
        Path(__file__).parent / "fixtures" / "catalog" / "swpc_3day_geomag_2026-11-07.txt"
    ).read_text()
    probs = m.storm_probabilities(text)
    assert list(probs) == ["2026-11-08", "2026-11-09", "2026-11-10"]
    assert probs["2026-11-08"] == pytest.approx([0.25, 0.35, 0.35, 0.05])
    assert probs["2026-11-10"] == pytest.approx([0.69, 0.25, 0.05, 0.01])
    for p in probs.values():
        assert sum(p) == pytest.approx(1.0)
