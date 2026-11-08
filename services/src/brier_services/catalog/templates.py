"""The recurring series: each turns a day into the rulebooks of one event (docs/catalog.md).

Templates are pure and deterministic: the same day and inputs always give the same
rulebooks, so the same rules hashes, so a retried listing is recognized rather than refused.
Every series here resolves from a public source by machine (``review = "automatic"``), lists
two days ahead, and stops trading before its day starts, because each value can be watched
as it forms.

Market IDs are ``10,000,000 + day index * 1,000 + series code * 10 + outcome index``, where the
day index counts days from 1 January 2026; they cannot collide with hand-listed markets (below
10,000,000) or with each other.
"""

from __future__ import annotations

import datetime as dt
import itertools
from dataclasses import dataclass
from typing import Any

from brier_services.rules import Provider, find_series

ID_BASE = 10_000_000
EPOCH = dt.date(2026, 1, 1)
MONTH_NAMES = (
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
)  # fmt: skip


@dataclass(frozen=True)
class City:
    code: int  # series code, 1-12
    location: str  # NWS climate report location (CLI{location})
    slug: str
    name: str  # as people say it
    station: str  # where the official reading is taken
    latitude: float
    longitude: float
    timezone: str  # IANA zone; NWS climate days are local standard time


CITIES = (
    City(1, "NYC", "nyc", "New York", "Central Park", 40.7789, -73.9692, "America/New_York"),
    City(
        2,
        "ORD",
        "chicago",
        "Chicago",
        "O'Hare International Airport",
        41.9786,
        -87.9048,
        "America/Chicago",
    ),
    City(
        3,
        "LAX",
        "los-angeles",
        "Los Angeles",
        "Los Angeles International Airport",
        33.9382,
        -118.3866,
        "America/Los_Angeles",
    ),
    City(
        4,
        "MIA",
        "miami",
        "Miami",
        "Miami International Airport",
        25.7881,
        -80.3169,
        "America/New_York",
    ),
    City(
        5,
        "BOS",
        "boston",
        "Boston",
        "Logan International Airport",
        42.3606,
        -71.0106,
        "America/New_York",
    ),
    City(
        6,
        "SEA",
        "seattle",
        "Seattle",
        "Seattle-Tacoma International Airport",
        47.4447,
        -122.3144,
        "America/Los_Angeles",
    ),
    City(
        7,
        "DEN",
        "denver",
        "Denver",
        "Denver International Airport",
        39.8466,
        -104.6562,
        "America/Denver",
    ),
    City(
        8,
        "ATL",
        "atlanta",
        "Atlanta",
        "Hartsfield-Jackson International Airport",
        33.6301,
        -84.4418,
        "America/New_York",
    ),
    City(
        9,
        "PHX",
        "phoenix",
        "Phoenix",
        "Sky Harbor International Airport",
        33.4278,
        -112.0037,
        "America/Phoenix",
    ),
    City(
        10,
        "DCA",
        "washington-dc",
        "Washington, DC",
        "Reagan National Airport",
        38.8472,
        -77.0345,
        "America/New_York",
    ),
    City(
        11,
        "AUS",
        "austin",
        "Austin",
        "Austin-Bergstrom International Airport",
        30.1830,
        -97.6799,
        "America/Chicago",
    ),
    City(
        12,
        "DFW",
        "dallas",
        "Dallas",
        "Dallas/Fort Worth International Airport",
        32.8998,
        -97.0403,
        "America/Chicago",
    ),
)
QUAKE_M5_CODE, QUAKE_M6_CODE, KP_CODE = 50, 51, 60

TEMPERATURE_WIDTH = 2  # degrees per inner range
TEMPERATURE_INNER = 5  # inner ranges; with the two open ends, 7 outcomes


def market_id(day: dt.date, code: int, index: int) -> int:
    if not (0 <= index < 10 and 0 < code < 100):
        msg = f"code {code} and index {index} out of range"
        raise ValueError(msg)
    return ID_BASE + (day - EPOCH).days * 1000 + code * 10 + index


def _iso(day: dt.date, hour: int = 0) -> str:
    return f"{day.isoformat()}T{hour:02d}:00:00Z"


def _long(day: dt.date) -> str:
    return f"{MONTH_NAMES[day.month - 1]} {day.day}, {day.year}"


def _short(day: dt.date) -> str:
    return f"{MONTH_NAMES[day.month - 1][:3]} {day.day}"


def _source(provider: Provider, series: str) -> dict[str, str]:
    found = find_series(provider, series)
    if found is None:
        msg = f"{provider} {series} is not in the registry"
        raise ValueError(msg)
    return {"provider": str(provider), "series": series, "url": found.url}


def _rulebook(
    *,
    market: int,
    title: str,
    question: str,
    source: dict[str, str],
    day: dt.date,
    condition: dict[str, str],
    release: str,
    deadline: str,
    event: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": 2,
        "market": market,
        "title": title,
        "question": question,
        "source": source,
        "observation": day.isoformat(),
        "condition": {"transform": "level", **condition},
        "schedule": {
            "open": _iso(day - dt.timedelta(days=2)),
            "close": _iso(day),
            "release": release,
            "deadline": deadline,
        },
        "revision": "first_release",
        "void": ["no_value_by_deadline"],
        "event": event,
        "review": "automatic",
    }


def temperature(city: City, day: dt.date, forecast_high: int) -> list[dict[str, Any]]:
    """Seven ranges for the city's official high, the middle one holding the forecast."""
    low = forecast_high - TEMPERATURE_WIDTH * (TEMPERATURE_INNER // 2) - 2  # middle: [f - 1, f]
    edges = [low + TEMPERATURE_WIDTH * i for i in range(TEMPERATURE_INNER + 1)]
    # Outcomes: <= edges[0], then [edges[i] + 1, edges[i + 1]], then >= edges[-1] + 1.
    outcomes: list[tuple[str, dict[str, str], str]] = [
        (
            f"{edges[0]}°F or below",
            {"op": "<=", "threshold": str(edges[0])},
            f"{edges[0]} °F or lower",
        )
    ]
    for a, b in itertools.pairwise(edges):
        outcomes.append(
            (
                f"{a + 1}-{b}°F",
                {"op": "between", "threshold": str(a + 1), "upper": str(b)},
                f"between {a + 1} and {b} °F",
            )
        )
    top = edges[-1] + 1
    outcomes.append(
        (f"{top}°F or above", {"op": ">=", "threshold": str(top)}, f"{top} °F or higher")
    )
    event = {
        "slug": f"{city.slug}-high-{day.isoformat()}",
        "title": f"Highest temperature in {city.name} on {_short(day)}?",
        "tags": sorted(["daily", city.slug, "temperature"]),
        "series": f"{city.slug}-daily-high",
        "exclusive": True,
    }
    return [
        _rulebook(
            market=market_id(day, city.code, i),
            title=f"{city.name} high {label} on {_short(day)}",
            question=(
                f"Will the official high temperature at {city.station} ({city.name}) on "
                f"{_long(day)} be {words}, as reported in the National Weather Service's daily "
                "climate report?"
            ),
            source=_source(Provider.NWS, f"CLI/{city.location}"),
            day=day,
            condition=condition,
            release=_iso(day + dt.timedelta(days=1), 15),
            deadline=_iso(day + dt.timedelta(days=5)),
            event={**event, "outcome": label},
        )
        for i, (label, condition, words) in enumerate(outcomes)
    ]


QUAKE_RANGES = (
    ("2 or fewer", {"op": "<=", "threshold": "2"}, "2 or fewer"),
    ("3-4", {"op": "between", "threshold": "3", "upper": "4"}, "3 or 4"),
    ("5-6", {"op": "between", "threshold": "5", "upper": "6"}, "5 or 6"),
    ("7 or more", {"op": ">=", "threshold": "7"}, "7 or more"),
)


def earthquakes(day: dt.date) -> list[list[dict[str, Any]]]:
    """Two events: how many magnitude 5+ earthquakes, and whether any magnitude 6+ one."""
    release, deadline = _iso(day + dt.timedelta(days=2)), _iso(day + dt.timedelta(days=20))
    count_event = {
        "slug": f"earthquakes-m5-{day.isoformat()}",
        "title": f"How many magnitude 5+ earthquakes on {_short(day)}?",
        "tags": ["daily", "earthquakes"],
        "series": "earthquakes-m5-daily",
        "exclusive": True,
    }
    counts = [
        _rulebook(
            market=market_id(day, QUAKE_M5_CODE, i),
            title=f"{label} magnitude 5+ earthquakes on {_short(day)}",
            question=(
                f"Will there be {words} earthquakes of magnitude 5.0 or more worldwide on "
                f"{_long(day)} (UTC), as listed in the USGS earthquake catalog?"
            ),
            source=_source(Provider.USGS, "WORLD/M5"),
            day=day,
            condition=condition,
            release=release,
            deadline=deadline,
            event={**count_event, "outcome": label},
        )
        for i, (label, condition, words) in enumerate(QUAKE_RANGES)
    ]
    any_six = _rulebook(
        market=market_id(day, QUAKE_M6_CODE, 0),
        title=f"Magnitude 6+ earthquake on {_short(day)}",
        question=(
            f"Will an earthquake of magnitude 6.0 or more strike anywhere on {_long(day)} (UTC), "
            "as listed in the USGS earthquake catalog?"
        ),
        source=_source(Provider.USGS, "WORLD/M6"),
        day=day,
        condition={"op": ">=", "threshold": "1"},
        release=release,
        deadline=deadline,
        event={
            "slug": f"earthquake-m6-{day.isoformat()}",
            "title": f"Magnitude 6+ earthquake anywhere on {_short(day)}?",
            "tags": ["daily", "earthquakes"],
            "series": "earthquake-m6-daily",
            "exclusive": False,
            "outcome": "Yes",
        },
    )
    return [counts, [any_six]]


# NOAA's geomagnetic storm scale: G1 is Kp 5 (5-, 5, 5+), G2 is Kp 6, G3 or stronger Kp 7 and up.
# Kp is published in thirds to two decimals (4.67 is 5-), so the ranges meet exactly.
KP_RANGES = (
    ("No storm", {"op": "<", "threshold": "4.67"}, "below storm level (Kp under 5-)"),
    (
        "G1 minor",
        {"op": "between", "threshold": "4.67", "upper": "5.66"},
        "a G1 minor storm (Kp 5- to 5+)",
    ),
    (
        "G2 moderate",
        {"op": "between", "threshold": "5.67", "upper": "6.66"},
        "a G2 moderate storm (Kp 6- to 6+)",
    ),
    ("G3 or stronger", {"op": ">=", "threshold": "6.67"}, "a G3 storm or stronger (Kp 7- or more)"),
)


def geomagnetic(day: dt.date) -> list[dict[str, Any]]:
    """The strongest geomagnetic storm level of the day."""
    event = {
        "slug": f"geomagnetic-storm-{day.isoformat()}",
        "title": f"Geomagnetic storm level on {_short(day)}?",
        "tags": ["daily", "space-weather"],
        "series": "geomagnetic-daily",
        "exclusive": True,
    }
    return [
        _rulebook(
            market=market_id(day, KP_CODE, i),
            title=f"Geomagnetic activity on {_short(day)}: {label}",
            question=(
                f"Will the highest planetary K-index on {_long(day)} (UTC) reach {words}, from "
                "NOAA Space Weather Prediction Center's estimated 3-hour values?"
            ),
            source=_source(Provider.SWPC, "KP/MAX"),
            day=day,
            condition=condition,
            release=_iso(day + dt.timedelta(days=1), 3),
            deadline=_iso(day + dt.timedelta(days=6)),
            event={**event, "outcome": label},
        )
        for i, (label, condition, words) in enumerate(KP_RANGES)
    ]
