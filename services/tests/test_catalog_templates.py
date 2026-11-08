"""The recurring-series templates (docs/catalog.md): valid, deterministic, collision-free, and,
for exclusive events, covering every possible value exactly once."""

from __future__ import annotations

import datetime as dt
import itertools
from decimal import Decimal
from typing import Any

import pytest

from brier_services.catalog import templates as t
from brier_services.rules import Rulebook, validate

DAYS = [dt.date(2026, 11, 10) + dt.timedelta(days=i) for i in range(60)]


def events_for(day: dt.date) -> list[list[dict[str, Any]]]:
    out = [t.temperature(city, day, 50 + city.code) for city in t.CITIES]
    out += t.earthquakes(day)
    out.append(t.geomagnetic(day))
    return out


def yes_range(rule: Rulebook) -> tuple[Decimal | None, Decimal | None]:
    """The YES set as an inclusive range on the series' precision lattice (None: unbounded)."""
    step = Decimal(1).scaleb(-rule.measure.precision)
    th = Decimal(rule.threshold)
    match str(rule.op):
        case ">":
            return th + step, None
        case ">=":
            return th, None
        case "<":
            return None, th - step
        case "<=":
            return None, th
        case _:
            assert rule.upper is not None
            return th, Decimal(rule.upper)


def partitions(rules: list[Rulebook]) -> bool:
    step = Decimal(1).scaleb(-rules[0].measure.precision)
    ranges = sorted(
        (yes_range(r) for r in rules), key=lambda r: r[0] if r[0] is not None else Decimal("-1e9")
    )
    if ranges[0][0] is not None or ranges[-1][1] is not None:
        return False
    return all(a[1] is not None and b[0] == a[1] + step for a, b in itertools.pairwise(ranges))


@pytest.mark.parametrize("day", DAYS[:3])
def test_every_rulebook_is_valid_and_events_are_consistent(day: dt.date) -> None:
    for event in events_for(day):
        rules = [validate(rb) for rb in event]
        first = event[0]["event"]
        for rb in event:
            assert {k: v for k, v in rb["event"].items() if k != "outcome"} == {
                k: v for k, v in first.items() if k != "outcome"
            }
            assert rb["review"] == "automatic"
            assert rb["event"]["series"]
        assert len({rb["event"]["outcome"] for rb in event}) == len(event)
        assert len({r.times.close for r in rules}) == 1
        if first["exclusive"]:
            assert partitions(rules), first["slug"]


def test_market_ids_never_collide() -> None:
    ids = [rb["market"] for day in DAYS for event in events_for(day) for rb in event]
    assert len(ids) == len(set(ids))
    assert min(ids) > 10_000_000
    assert max(ids) < 2**32


def test_same_inputs_same_rulebooks() -> None:
    city = t.CITIES[0]
    a = [validate(rb).hash for rb in t.temperature(city, DAYS[0], 61)]
    b = [validate(rb).hash for rb in t.temperature(city, DAYS[0], 61)]
    c = [validate(rb).hash for rb in t.temperature(city, DAYS[0], 62)]
    assert a == b
    assert a != c


def test_temperature_ranges_centre_on_the_forecast() -> None:
    labels = [rb["event"]["outcome"] for rb in t.temperature(t.CITIES[0], DAYS[0], 61)]
    assert labels == [
        "55°F or below", "56-57°F", "58-59°F", "60-61°F", "62-63°F", "64-65°F", "66°F or above",
    ]  # fmt: skip
    middle = t.temperature(t.CITIES[0], DAYS[0], 61)[3]
    assert middle["condition"] == {
        "transform": "level",
        "op": "between",
        "threshold": "60",
        "upper": "61",
    }
    assert middle["schedule"] == {
        "open": "2026-11-08T00:00:00Z",
        "close": "2026-11-10T00:00:00Z",
        "release": "2026-11-11T15:00:00Z",
        "deadline": "2026-11-15T00:00:00Z",
    }


def test_text_is_specific_and_short_enough() -> None:
    for event in events_for(DAYS[0]):
        for rb in event:
            assert len(rb["title"]) <= 120
            assert len(rb["question"]) <= 500
            assert len(rb["event"]["title"]) <= 120
    q = t.temperature(t.CITIES[11], DAYS[0], 70)[1]["question"]
    assert q == (
        "Will the official high temperature at Dallas/Fort Worth International Airport (Dallas) on "
        "November 10, 2026 be between 65 and 66 °F, as reported in the National Weather Service's "
        "daily climate report?"
    )
