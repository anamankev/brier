"""The market maker's fair values for the recurring catalogue, through ``FairValues`` with
recorded inputs: NOAA's storm probabilities come through exactly, a day's temperature ranges
sum to one, and earthquake shares are read off the history."""

from __future__ import annotations

import datetime as dt
import json
from pathlib import Path

import pytest

from brier_services.catalog import templates
from brier_services.marketmaker import catalog_values as cv
from brier_services.marketmaker.service import FairValues
from brier_services.resolution.fetch import Request, Response

FIXTURES = Path(__file__).parent / "fixtures" / "catalog"
OPEN_METEO = (FIXTURES / "open_meteo_forecast_nyc_2026-11-08.json").read_bytes()
SWPC = (FIXTURES / "swpc_3day_geomag_2026-11-07.txt").read_bytes()
NOW = dt.datetime(2026, 11, 8, 12, tzinfo=dt.UTC).timestamp()


def quakes(days_with: dict[int, int], magnitude: float = 5.2) -> bytes:
    """A USGS response with ``n`` earthquakes on the day ``i`` days before 2026-11-08."""
    features = []
    for ago, n in days_with.items():
        t = int(
            (dt.datetime(2026, 11, 8, tzinfo=dt.UTC) - dt.timedelta(days=ago, hours=-1)).timestamp()
            * 1000
        )
        features += [{"properties": {"mag": magnitude, "time": t}} for _ in range(n)]
    return json.dumps({"features": features, "metadata": {}}).encode()


def values(usgs: bytes = b'{"features": [], "metadata": {}}') -> FairValues:
    def fetch(request: Request) -> Response:
        if "open-meteo" in request.url:
            body = OPEN_METEO
        elif "swpc" in request.url:
            body = SWPC
        else:
            body = usgs
        return Response(request.url, 200, body, "2026-11-08T12:00:00Z")

    return FairValues(None, fetcher=fetch, wall=lambda: NOW)


def test_storm_markets_take_noaas_probabilities() -> None:
    fair = values()
    got = [fair.probability(rb) for rb in templates.geomagnetic(dt.date(2026, 11, 8))]
    assert got == pytest.approx([0.25, 0.35, 0.35, 0.05])
    assert fair.probability(templates.geomagnetic(dt.date(2026, 11, 20))[0]) is None, (
        "beyond the forecast"
    )


def test_a_days_temperature_ranges_sum_to_one() -> None:
    nyc = templates.CITIES[0]
    high = cv.forecast_high(OPEN_METEO, "2026-11-09")
    assert high is not None
    assert cv.forecast_high(OPEN_METEO, "2026-11-12") is None, "outside the forecast"
    rulebooks = templates.temperature(nyc, dt.date(2026, 11, 9), round(high))
    raw = []
    for rb in rulebooks:
        c = rb["condition"]
        if c["op"] == "between":
            raw.append(
                cv.temperature(nyc, high, ">=", float(c["threshold"]))
                - cv.temperature(nyc, high, ">", float(c["upper"]))
            )
        else:
            raw.append(cv.temperature(nyc, high, c["op"], float(c["threshold"])))
    assert sum(raw) == pytest.approx(1.0)
    priced = [values().probability(rb) for rb in rulebooks]
    assert all(p is not None and 0.02 <= p <= 0.98 for p in priced)
    assert (
        max(priced, key=lambda p: p or 0) == priced[3]
        or max(priced, key=lambda p: p or 0) == priced[4]
    ), "the most likely range is next to the forecast (the bias moves it up)"


def test_earthquake_shares_come_from_the_past_year() -> None:
    # 100 of the last 365 days had 7 or more magnitude 5+ earthquakes; the rest had none.
    body = quakes(dict.fromkeys(range(1, 101), 8))
    fair = values(body)
    seven_plus = templates.earthquakes(dt.date(2026, 11, 9))[0][3]
    assert fair.probability(seven_plus) == pytest.approx((100 + 1) / (365 + 2))
    two_or_fewer = templates.earthquakes(dt.date(2026, 11, 9))[0][0]
    assert fair.probability(two_or_fewer) == pytest.approx((265 + 1) / (365 + 2))
    any_six = templates.earthquakes(dt.date(2026, 11, 9))[1][0]
    assert fair.probability(any_six) == pytest.approx(0.02), (
        "no magnitude 6 in the history: the floor"
    )
