"""Economy and rates events (G10 U5): valid rulebooks whose outcomes cover every possible
value exactly once, centred on the latest value, on the official calendar."""

from __future__ import annotations

import datetime as dt
from decimal import Decimal
from fractions import Fraction
from typing import Any

import pytest

from brier_services.catalog import economy
from brier_services.resolution.values import holds
from brier_services.rules import validate


def exactly_one(rulebooks: list[dict[str, Any]], tick: Decimal, around: Decimal) -> None:
    """Every value on the series' lattice within ±50 ticks of ``around`` satisfies exactly one
    outcome."""
    checked = [validate(rb) for rb in rulebooks]
    for k in range(-50, 51):
        v = Fraction(around + tick * k)
        hits = [r.raw["event"]["outcome"] for r in checked if holds(v, r.op, r.threshold, r.upper)]
        assert len(hits) == 1, (float(v), hits)


def test_cpi_ranges_partition_and_hold_the_latest_value() -> None:
    rbs = economy.cpi(dt.date(2026, 12, 10), Decimal("3.94"))
    exactly_one(rbs, Decimal("0.1"), Decimal("3.9"))
    middle = rbs[2]
    assert middle["condition"]["threshold"] <= "3.9" <= middle["condition"]["upper"]
    assert rbs[0]["observation"] == "2026-11"
    assert rbs[0]["schedule"]["release"] == "2026-12-10T13:30:00Z", "08:30 Eastern in winter"
    assert all(rb["review"] == "automatic" for rb in rbs)


def test_treasury_ranges_have_no_gaps_at_hundredths() -> None:
    rbs = economy.treasury(dt.date(2026, 11, 20), Decimal("4.88"))
    exactly_one(rbs, Decimal("0.01"), Decimal("4.88"))
    assert rbs[0]["schedule"]["close"] == "2026-11-20T00:00:00Z", "closes before the day starts"


def test_jobs_values_and_fed_moves_partition() -> None:
    exactly_one(economy.jobs(dt.date(2026, 12, 4), Decimal("4.1")), Decimal("0.1"), Decimal("4.1"))
    fed = economy.fed(dt.date(2026, 12, 9), Decimal("4.00"))
    assert [rb["event"]["outcome"] for rb in fed] == [
        "Cut 50+ bp",
        "Cut 25 bp",
        "No change",
        "Hike",
    ]
    checked = [validate(rb) for rb in fed]
    for v in ("3.25", "3.50", "3.75", "4.00", "4.25", "4.50"):
        hits = [r for r in checked if holds(Fraction(Decimal(v)), r.op, r.threshold, r.upper)]
        assert len(hits) == 1, v
    assert fed[0]["observation"] == "2026-12-10"
    assert fed[0]["schedule"]["release"] == "2026-12-09T19:00:00Z", "2:00 p.m. Eastern"


def test_market_ids_never_repeat_across_series() -> None:
    day = dt.date(2026, 12, 9)
    ids = [rb["market"] for rbs in (economy.cpi(day, Decimal(3)), economy.jobs(day, Decimal(4)),
                                    economy.fed(day, Decimal(4)), economy.treasury(day, Decimal(4)))
           for rb in rbs]  # fmt: skip
    assert len(ids) == len(set(ids))


def test_events_reads_the_calendar_and_latest_values() -> None:
    def get(url: str) -> Any:  # noqa: ANN401 - JSON
        if "release/dates" in url:
            date = "2026-12-10" if "release_id=10" in url else "2026-12-04"
            return {"release_dates": [{"date": "2026-11-10"}, {"date": date}]}
        if "CPIAUCNS" in url:
            return {
                "observations": [
                    {"date": f"m{i}", "value": "314" if i == 0 else "302"} for i in range(14)
                ]
            }
        series = next(s for s in ("UNRATE", "DFEDTARU", "DGS10") if s in url)
        return {
            "observations": [
                {
                    "date": "2026-11-11",
                    "value": {"UNRATE": "4.1", "DFEDTARU": "4.00", "DGS10": "4.88"}[series],
                }
            ]
        }

    evs = economy.events(dt.datetime(2026, 11, 12, 2, 0, tzinfo=dt.UTC), "k", get)
    assert [e[0]["event"]["series"] for e in evs] == [
        "us-cpi",
        "us-unemployment",
        "fomc-decision",
        "treasury-10y-weekly",
    ]
    assert evs[1][2]["event"]["outcome"] == "4.1%"
    assert evs[3][0]["observation"] == "2026-11-20", "the next Friday at least two days away"


@pytest.mark.parametrize("today", ["2026-11-19", "2026-11-20"])
def test_a_friday_too_close_to_trade_rolls_to_the_next(today: str) -> None:
    def get(url: str) -> Any:  # noqa: ANN401 - JSON
        if "release/dates" in url:
            return {"release_dates": []}
        return {"observations": [{"date": "x", "value": "4.5"}]}

    now = dt.datetime.fromisoformat(f"{today}T12:00:00+00:00")
    evs = economy.events(now, "k", get)
    assert evs[-1][0]["observation"] == "2026-11-27"
