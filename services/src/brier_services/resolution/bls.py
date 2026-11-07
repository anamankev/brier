"""BLS Public Data API, version 1 (no key; 25 queries a day). CPI-U and the unemployment rate.

The request is a POST with the years in a JSON body: a GET ignores ``startyear`` and
``endyear`` and answers the latest three years only (found by the R1 study).

Response: ``{"status": "REQUEST_SUCCEEDED", "Results": {"series": [{"seriesID": ..., "data":
[{"year": "2026", "period": "M09", "value": "336.940", "footnotes": [...]}, ...]}]}}``. A month
BLS did not publish carries the value ``"-"`` (for example October 2025, during the lapse in
appropriations); a month not yet published is absent.
"""

from __future__ import annotations

import json
import re

from brier_services.resolution.fetch import Request
from brier_services.resolution.sources import (
    MalformedResponseError,
    Missing,
    Reading,
    field,
    strict_json,
)
from brier_services.resolution.values import parse_decimal
from brier_services.rules import Rulebook, Transform

HOST = "api.bls.gov"
_PERIOD = re.compile(r"M(0[1-9]|1[0-2])")


def _year_month(observation: str) -> tuple[int, int]:
    year, month = observation.split("-")
    return int(year), int(month)


def request(rule: Rulebook, api_key: str | None = None, now: int | None = None) -> Request:
    """The year before the observation's and its own, so a 12-month change has both months."""
    year, _ = _year_month(rule.observation)
    return years(rule.series.id, year - 1, year)


def years(series_id: str, first: int, last: int) -> Request:
    """Every month of ``series_id`` from ``first`` to ``last`` (at most 10 years in version 1)."""
    body = {"seriesid": [series_id], "startyear": str(first), "endyear": str(last)}
    return Request(f"https://{HOST}/publicAPI/v1/timeseries/data/", json.dumps(body).encode())


def _months(body: bytes, series_id: str) -> dict[tuple[int, int], str]:
    doc = strict_json(body)
    status = field(doc, "status", str)
    if status != "REQUEST_SUCCEEDED":
        msg = f"BLS status {status}: {doc.get('message')}"
        raise MalformedResponseError(msg)
    series = field(field(doc, "Results", dict), "series", list)
    if len(series) != 1 or field(series[0], "seriesID", str) != series_id:
        msg = f"expected exactly the series {series_id}"
        raise MalformedResponseError(msg)
    out: dict[tuple[int, int], str] = {}
    for row in field(series[0], "data", list):
        year, period = field(row, "year", str), field(row, "period", str)
        value = field(row, "value", str)
        if period == "M13":  # the annual average
            continue
        m = _PERIOD.fullmatch(period)
        if m is None or not re.fullmatch(r"[0-9]{4}", year):
            msg = f"unexpected period {year} {period}"
            raise MalformedResponseError(msg)
        key = (int(year), int(m.group(1)))
        if key in out:
            msg = f"{year} {period} appears twice"
            raise MalformedResponseError(msg)
        out[key] = value
    return out


def _value(months: dict[tuple[int, int], str], key: tuple[int, int]) -> Reading:
    raw = months.get(key)
    if raw is None:
        return Missing(f"{key[0]}-{key[1]:02d} not published")
    if raw.strip() == "-":
        return Missing(f"{key[0]}-{key[1]:02d} published as unavailable")
    value = parse_decimal(raw)
    if value is None or value <= 0:
        msg = f"unreadable value {raw!r}"
        raise MalformedResponseError(msg)
    return value


def parse(body: bytes, rule: Rulebook) -> Reading:
    """The value under the rule's transform, unrounded."""
    months = _months(body, rule.series.id)
    year, month = _year_month(rule.observation)
    now = _value(months, (year, month))
    if rule.measure.transform is Transform.LEVEL or isinstance(now, Missing):
        return now
    before = _value(months, (year - 1, month))
    if isinstance(before, Missing):
        return before
    return (now / before - 1) * 100
