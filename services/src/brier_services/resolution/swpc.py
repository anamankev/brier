"""NOAA Space Weather Prediction Center: the planetary K-index, a 0-9 measure of geomagnetic
activity, estimated every 3 hours (``products/noaa-planetary-k-index.json``, the last 7 days).

A day's reading is its highest Kp, once all eight 3-hour values (00:00 to 21:00 UTC) are
published; until then it is missing. Values are thirds written to two decimals (4.67 is "5-"),
read exactly from their decimal text. These are SWPC's estimates; GFZ Potsdam later publishes
definitive values that can differ, and under the first-release rule the estimate decides.
"""

from __future__ import annotations

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

URL = "https://services.swpc.noaa.gov/products/noaa-planetary-k-index.json"
SLOTS = tuple(f"{h:02d}:00:00" for h in range(0, 24, 3))


def request(rule: Rulebook, api_key: str | None = None, now: int | None = None) -> Request:
    return Request(URL)


def parse(body: bytes, rule: Rulebook) -> Reading:
    """The day's maximum Kp."""
    doc = strict_json(body)
    if not isinstance(doc, list):
        msg = "expected a list of 3-hour records"
        raise MalformedResponseError(msg)
    values = {}
    for row in doc:
        tag = field(row, "time_tag", str)
        day, _, slot = tag.partition("T")
        if day != rule.observation:
            continue
        kp = row.get("Kp")
        value = parse_decimal(repr(kp)) if isinstance(kp, (int, float)) else None
        if value is None or not 0 <= value <= 9 or slot not in SLOTS or slot in values:
            msg = f"unreadable or repeated record {row!r}"
            raise MalformedResponseError(msg)
        values[slot] = value
    if len(values) < len(SLOTS):
        return Missing(f"{len(values)} of 8 three-hour values for {rule.observation} published")
    return max(values.values())
