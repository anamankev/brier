"""NOAA NCEI Access Data Service, GHCN-Daily ``daily-summaries`` (no token): a station's daily
maximum temperature.

The request asks for metric units: GHCN-Daily stores tenths of a degree Celsius, and the
service returns degrees with one decimal (``[{"DATE": "2025-07-01", "STATION": "USW00094728",
"TMAX": "31.7"}]``). Brier converts exactly, F = C * 9/5 + 32, and rounds to whole degrees,
which recovers the station's original Fahrenheit reading. A day not yet published is an empty
list.
"""

from __future__ import annotations

from fractions import Fraction

from brier_services.resolution.sources import (
    MalformedResponseError,
    Missing,
    Reading,
    field,
    strict_json,
)
from brier_services.resolution.values import parse_decimal
from brier_services.rules import Rulebook

HOST = "www.ncei.noaa.gov"


def _station_and_type(rule: Rulebook) -> tuple[str, str]:
    station, data_type = rule.series.id.split("/")
    return station, data_type


def request_url(rule: Rulebook, api_key: str | None = None, now: int | None = None) -> str:
    station, data_type = _station_and_type(rule)
    return (
        f"https://{HOST}/access/services/data/v1?dataset=daily-summaries&stations={station}"
        f"&startDate={rule.observation}&endDate={rule.observation}&dataTypes={data_type}"
        "&units=metric&format=json"
    )


def parse(body: bytes, rule: Rulebook) -> Reading:
    """Degrees Fahrenheit, unrounded."""
    station, data_type = _station_and_type(rule)
    doc = strict_json(body)
    if not isinstance(doc, list):
        msg = "expected a list of daily records"
        raise MalformedResponseError(msg)
    rows = [
        r
        for r in doc
        if field(r, "DATE", str) == rule.observation and field(r, "STATION", str) == station
    ]
    if not rows:
        return Missing(f"{rule.observation} not published")
    if len(rows) != 1:
        msg = f"{rule.observation} appears {len(rows)} times"
        raise MalformedResponseError(msg)
    raw = rows[0].get(data_type)
    if raw is None or (isinstance(raw, str) and not raw.strip()):
        return Missing(f"{rule.observation} has no {data_type}")
    celsius = parse_decimal(raw) if isinstance(raw, str) else None
    if celsius is None:
        msg = f"unreadable {data_type} {raw!r}"
        raise MalformedResponseError(msg)
    return celsius * Fraction(9, 5) + 32
