"""Daily lows (G10 U5; docs/notes/daily-lows.md): the report's minimum line, the overnight
forecast, the template, and the maker's low model."""

from __future__ import annotations

import datetime as dt
import json
from pathlib import Path

from brier_services.catalog import templates
from brier_services.catalog.forecast import forecast_low
from brier_services.marketmaker import catalog_values as cv
from brier_services.resolution import nws
from brier_services.rules import validate

FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
DENVER = templates.CITIES[6]


def final_report() -> str:
    doc = json.loads((FIXTURES / "nws_cli_den_2026-11-06.json").read_text())
    for body in doc["responses"].values():
        text = json.loads(body)["productText"] if isinstance(body, str) else body["productText"]
        if nws.report_maximum(text, "2026-11-06") is not None:
            return str(text)
    raise AssertionError


def test_the_minimum_comes_from_the_same_final_block() -> None:
    text = final_report()
    assert nws.report_maximum(text, "2026-11-06", minimum=True) == 35
    assert nws.report_maximum(text, "2026-11-05", minimum=True) is None, "another day's report"


def test_low_events_are_valid_and_never_share_ids_with_highs() -> None:
    day = dt.date(2026, 11, 14)
    lows = templates.temperature(DENVER, day, 30, low=True)
    highs = templates.temperature(DENVER, day, 60)
    for rb in lows:
        assert validate(rb).series.id == "CLI/DEN/MIN"
    assert lows[0]["event"]["slug"] == "denver-low-2026-11-14"
    assert lows[0]["event"]["title"] == "Lowest temperature in Denver on Nov 14?"
    assert not {rb["market"] for rb in lows} & {rb["market"] for rb in highs}


def test_the_forecast_low_is_the_night_before() -> None:
    periods = [
        {"startTime": "2026-11-13T06:00:00-07:00", "isDaytime": True, "temperature": 58, "temperatureUnit": "F"},  # noqa: E501
        {"startTime": "2026-11-13T18:00:00-07:00", "isDaytime": False, "temperature": 31, "temperatureUnit": "F"},  # noqa: E501
        {"startTime": "2026-11-14T06:00:00-07:00", "isDaytime": True, "temperature": 61, "temperatureUnit": "F"},  # noqa: E501
    ]  # fmt: skip
    body = json.dumps({"properties": {"periods": periods}}).encode()
    assert forecast_low(body, dt.date(2026, 11, 14)) == 31


def test_the_maker_prices_lows_with_the_low_fit() -> None:
    hours = [f"2026-11-14T{h:02d}:00" for h in range(24)]
    body = json.dumps(
        {"hourly": {"time": hours, "temperature_2m": [30.0 + h / 2 for h in range(24)]}}
    ).encode()
    assert cv.forecast_low(body, "2026-11-14") == 30.0
    p_low = cv.temperature(DENVER, 30.0, ">=", 30, low=True)
    p_high = cv.temperature(DENVER, 30.0, ">=", 30)
    assert p_low != p_high, "the low uses its own bias and spread"
    assert 0 < p_low < 1
