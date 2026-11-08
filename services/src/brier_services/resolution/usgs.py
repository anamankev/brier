"""USGS earthquake catalog (ComCat, FDSN event service): how many earthquakes of at least a
magnitude happened worldwide in a UTC day.

``minmagnitude`` is inclusive (checked: a magnitude 5.0 event is returned for
``minmagnitude=5``). Magnitudes are revised after the fact; under the first-release rule the
catalog as read at the first fetch after the release time decides. Every returned event is
checked against the day and the magnitude, and a response that may be cut short (as many events as
the limit, or a listed count that differs from the events) is refused.
"""

from __future__ import annotations

from fractions import Fraction

from brier_services.resolution.fetch import Request
from brier_services.resolution.sources import MalformedResponseError, Reading, field, strict_json
from brier_services.resolution.values import parse_decimal
from brier_services.rules import Rulebook, period

HOST = "earthquake.usgs.gov"
LIMIT = 1000  # far above any day's count at magnitude 5; ordering by time lifts the range limits


def _magnitude(rule: Rulebook) -> int:
    return int(rule.series.id.removeprefix("WORLD/M"))


def request(rule: Rulebook, api_key: str | None = None, now: int | None = None) -> Request:
    return Request(
        f"https://{HOST}/fdsnws/event/1/query?format=geojson&eventtype=earthquake"
        f"&starttime={rule.observation}T00:00:00&endtime={rule.observation}T23:59:59.999"
        f"&minmagnitude={_magnitude(rule)}&orderby=time-asc&limit={LIMIT}"
    )


def parse(body: bytes, rule: Rulebook) -> Reading:
    """The number of events."""
    doc = strict_json(body)
    features = field(doc, "features", list)
    metadata = field(doc, "metadata", dict)
    listed = metadata.get("count", len(features))  # present only when no limit is asked
    if listed != len(features) or len(features) >= LIMIT:
        msg = f"the response lists {listed} events and holds {len(features)}: it may be cut short"
        raise MalformedResponseError(msg)
    bounds = period(rule.series.kind, rule.observation)
    if bounds is None:
        msg = f"unreadable observation {rule.observation!r}"
        raise MalformedResponseError(msg)
    start_ms, end_ms = bounds[0] * 1000, bounds[1] * 1000
    floor = _magnitude(rule)
    for f in features:
        props = field(f, "properties", dict)
        when = field(props, "time", int)
        mag = props.get("mag")
        value = parse_decimal(repr(mag)) if isinstance(mag, (int, float)) else None
        if value is None or value < floor or not start_ms <= when < end_ms:
            msg = f"event {f.get('id')!r} is outside the day or below magnitude {floor}"
            raise MalformedResponseError(msg)
    return Fraction(len(features))
