"""FRED API (St. Louis Fed; needs ``FRED_API_KEY``): the fed funds target upper limit
(``DFEDTARU``) and the 10-year Treasury yield (``DGS10``).

The request asks for the initial release only (``output_type=4``) over real time from the
observation date on, so a late fetch still reads the value as first published. Response:
``{"observations": [{"realtime_start": "2025-07-02", "date": "2025-07-01", "value": "4.26"}]}``;
a missing value is ``"."``. Raw FRED responses are not committed (docs/data-licenses.md).
"""

from __future__ import annotations

import datetime as dt
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
from brier_services.rules import Rulebook

HOST = "api.stlouisfed.org"
_DATE = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}")


def request(rule: Rulebook, api_key: str | None, now: int | None = None) -> Request:
    """FRED refuses a real-time start after today, so it is the observation date or today."""
    if not api_key:
        msg = "FRED needs FRED_API_KEY"
        raise ValueError(msg)
    today = (
        dt.datetime.fromtimestamp(
            now if now is not None else dt.datetime.now(dt.UTC).timestamp(), dt.UTC
        )
        .date()
        .isoformat()
    )
    start = min(rule.observation, today)
    return Request(
        f"https://{HOST}/fred/series/observations?series_id={rule.series.id}"
        f"&observation_start={rule.observation}&observation_end={rule.observation}"
        f"&realtime_start={start}&realtime_end=9999-12-31&output_type=4&file_type=json"
        f"&api_key={api_key}"
    )


def parse(body: bytes, rule: Rulebook) -> Reading:
    """The first-released value for the observation date."""
    doc = strict_json(body)
    if isinstance(doc, dict) and "error_code" in doc:
        msg = f"FRED error {doc.get('error_code')}: {doc.get('error_message')}"
        raise MalformedResponseError(msg)
    rows = [
        r for r in field(doc, "observations", list) if field(r, "date", str) == rule.observation
    ]
    if not rows:
        return Missing(f"{rule.observation} not published")
    for r in rows:
        if not _DATE.fullmatch(field(r, "realtime_start", str)):
            msg = "unreadable realtime_start"
            raise MalformedResponseError(msg)
    first = min(rows, key=lambda r: r["realtime_start"])
    if sum(1 for r in rows if r["realtime_start"] == first["realtime_start"]) != 1:
        msg = "two values with the same vintage"
        raise MalformedResponseError(msg)
    raw = field(first, "value", str)
    if raw == ".":
        return Missing(f"{rule.observation} published as missing")
    value = parse_decimal(raw)
    if value is None:
        msg = f"unreadable value {raw!r}"
        raise MalformedResponseError(msg)
    return value
