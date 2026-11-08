"""USGS earthquake counts and SWPC geomagnetic activity, on recorded responses
(services/tests/fixtures/resolution/SOURCES.md)."""

from __future__ import annotations

import json
from fractions import Fraction
from pathlib import Path

import pytest

from brier_services.resolution import swpc, usgs
from brier_services.resolution.fetch import Fetcher, Request, Response
from brier_services.resolution.resolver import Proposal, Wait, resolve
from brier_services.resolution.sources import MalformedResponseError, Missing
from brier_services.rules import Provider, Rulebook, find_series, instant, validate

FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
M5 = (FIXTURES / "usgs_m5_2026-11-05.json").read_bytes()
M6 = (FIXTURES / "usgs_m6_2026-11-05.json").read_bytes()
KP = (FIXTURES / "swpc_kp_2026-11-08.json").read_bytes()


def rule(provider: str, series: str, day: str, condition: dict[str, str]) -> Rulebook:
    found = find_series(Provider(provider), series)
    assert found is not None
    nxt = f"{day[:8]}{int(day[8:]) + 1:02d}"
    return validate(
        {
            "schema": 1,
            "market": 601,
            "title": "Science market",
            "question": "Science question?",
            "observation": day,
            "revision": "first_release",
            "void": ["no_value_by_deadline"],
            "source": {"provider": provider, "series": series, "url": found.url},
            "condition": {"transform": "level", **condition},
            "schedule": {
                "open": "2026-10-01T00:00:00Z",
                "close": f"{day}T00:00:00Z",
                "release": f"{nxt}T03:00:00Z",
                "deadline": "2026-11-20T00:00:00Z",
            },
        }
    )


def serve(body: bytes, now: str = "2026-11-08T07:10:41Z") -> Fetcher:
    def fetch(request: Request) -> Response:
        return Response(request.url, 200, body, now)

    return fetch


def test_counts_earthquakes_including_the_boundary_magnitude() -> None:
    m5 = rule("usgs", "WORLD/M5", "2026-11-05", {"op": ">=", "threshold": "3"})
    assert usgs.parse(M5, m5) == 3  # magnitudes 5.0, 5.1 and 6.1: 5.0 counts
    m6 = rule("usgs", "WORLD/M6", "2026-11-05", {"op": ">=", "threshold": "1"})
    assert usgs.parse(M6, m6) == 1
    assert usgs.request(m5).url.endswith("&minmagnitude=5&orderby=time-asc&limit=1000")
    result = resolve(m5, instant("2026-11-08T07:10:41Z") or 0, fetcher=serve(M5))
    assert isinstance(result, Proposal)
    assert (result.outcome, result.evidence["value"]) == ("yes", "3")


def test_refuses_a_truncated_or_inconsistent_catalog_answer() -> None:
    m5 = rule("usgs", "WORLD/M5", "2026-11-05", {"op": ">=", "threshold": "3"})
    doc = json.loads(M5)
    doc["metadata"]["count"] = 4
    with pytest.raises(MalformedResponseError):
        usgs.parse(json.dumps(doc).encode(), m5)
    doc = json.loads(M5)
    doc["features"][0]["properties"]["mag"] = 4.9
    with pytest.raises(MalformedResponseError):
        usgs.parse(json.dumps(doc).encode(), m5)
    doc = json.loads(M5)
    doc["features"][0]["properties"]["time"] -= 86_400_000
    with pytest.raises(MalformedResponseError):
        usgs.parse(json.dumps(doc).encode(), m5)


def test_kp_is_the_day_maximum_read_exactly() -> None:
    storm = rule("swpc", "KP/MAX", "2026-11-07", {"op": ">=", "threshold": "4.67"})
    assert swpc.parse(KP, storm) == Fraction(533, 100)
    quiet = rule("swpc", "KP/MAX", "2026-11-01", {"op": ">=", "threshold": "4.67"})
    assert swpc.parse(KP, quiet) == Fraction(467, 100)
    result = resolve(storm, instant("2026-11-08T07:10:41Z") or 0, fetcher=serve(KP))
    assert isinstance(result, Proposal)
    assert (result.outcome, result.evidence["value"]) == ("yes", "5.33")


def test_a_day_with_missing_three_hour_values_waits_then_voids() -> None:
    today = rule("swpc", "KP/MAX", "2026-11-08", {"op": ">=", "threshold": "4.67"})
    assert isinstance(swpc.parse(KP, today), Missing)
    waiting = resolve(today, instant("2026-11-09T03:00:00Z") or 0, fetcher=serve(KP))
    assert isinstance(waiting, Wait)
    assert "2 of 8" in waiting.reason
    late = resolve(
        today, instant("2026-11-20T00:00:00Z") or 0, fetcher=serve(KP, "2026-11-20T00:00:00Z")
    )
    assert isinstance(late, Proposal)
    assert (late.outcome, late.void_condition) == ("void", "no_value_by_deadline")


def test_a_repeated_three_hour_record_is_refused() -> None:
    doc = json.loads(KP)
    doc.append(next(r for r in doc if r["time_tag"].startswith("2026-11-07")))
    with pytest.raises(MalformedResponseError):
        swpc.parse(
            json.dumps(doc).encode(),
            rule("swpc", "KP/MAX", "2026-11-07", {"op": ">=", "threshold": "5"}),
        )
