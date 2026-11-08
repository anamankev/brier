"""The NWS daily climate report reader, on recorded responses for Denver, 6 November 2026.

That day shows why only the morning-after report counts: the report issued at 2:29 PM gave
the maximum so far as 41 °F; the final report, issued the next morning, gave 60 °F.
"""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
from typing import Any

import pytest

from brier_services import resolve as cli
from brier_services.resolution import nws
from brier_services.resolution.evidence import EvidenceStore
from brier_services.resolution.fetch import Fetcher, Request, Response
from brier_services.resolution.resolver import Proposal, Wait, resolve
from brier_services.resolution.sources import MalformedResponseError, Missing
from brier_services.rules import Provider, Rulebook, find_series, validate

FIXTURE = Path(__file__).parent / "fixtures" / "resolution" / "nws_cli_den_2026-11-06.json"
RECORDED: dict[str, str] = json.loads(FIXTURE.read_text())["responses"]


def texts() -> dict[str, str]:
    """Each recorded report's text, by issue time."""
    out = {}
    for body in RECORDED.values():
        doc = json.loads(body)
        if "productText" in doc:
            out[doc["issuanceTime"]] = doc["productText"]
    return out


def rule(release: str, op: str = ">=", threshold: str = "60") -> Rulebook:
    series = find_series(Provider.NWS, "CLI/DEN")
    assert series is not None
    return validate(
        {
            "schema": 1,
            "market": 501,
            "title": "Denver high",
            "question": "Denver high?",
            "observation": "2026-11-06",
            "revision": "first_release",
            "void": ["no_value_by_deadline"],
            "source": {"provider": "nws", "series": "CLI/DEN", "url": series.url},
            "condition": {"transform": "level", "op": op, "threshold": threshold},
            "schedule": {
                "open": "2026-11-01T00:00:00Z",
                "close": "2026-11-06T00:00:00Z",
                "release": release,
                "deadline": "2026-11-12T00:00:00Z",
            },
        }
    )


def fetcher(now: str) -> Fetcher:
    def fetch(request: Request) -> Response:
        return Response(request.url, 200, RECORDED[request.url].encode(), now)

    return fetch


def test_only_the_final_report_of_the_day_counts() -> None:
    by_time = texts()
    finals = {t: nws.report_maximum(x, "2026-11-06") for t, x in by_time.items()}
    assert finals["2026-11-07T07:35:00+00:00"] == 60
    prelim = [t for t in by_time if t.startswith("2026-11-06")]
    assert prelim, "the fixture has same-day reports"
    assert all(finals[t] is None for t in prelim), "same-day reports are preliminary"
    assert all(
        nws.report_maximum(x, "2026-11-05") is None for x in by_time.values() if "NOVEMBER 6" in x
    )


def test_record_and_missing_markers() -> None:
    final = texts()["2026-11-07T07:35:00+00:00"]
    line = next(x for x in final.splitlines() if x.strip().startswith("MAXIMUM"))
    assert (
        nws.report_maximum(final.replace(line, line.replace(" 60 ", " 60R", 1)), "2026-11-06") == 60
    )
    missing = nws.report_maximum(final.replace(line, line.replace(" 60 ", "  M ", 1)), "2026-11-06")
    assert isinstance(missing, Missing)
    with pytest.raises(MalformedResponseError):
        nws.report_maximum(final.replace(line, line.replace(" 60 ", " 6O ", 1)), "2026-11-06")


def test_resolves_from_the_first_final_report() -> None:
    now = "2026-11-08T12:00:00Z"
    result = resolve(rule("2026-11-07T15:00:00Z"), 1_794_139_200, fetcher=fetcher(now))
    assert isinstance(result, Proposal)
    assert result.outcome == "yes"
    assert result.evidence["value"] == "60"
    index = next(u for u in RECORDED if "end=2026-11-08" in u)
    assert result.evidence["index_url"] == index
    assert result.evidence["index_sha256"] == hashlib.sha256(RECORDED[index].encode()).hexdigest()
    assert result.extra == (RECORDED[index].encode(),)
    assert result.evidence["request_url"].startswith("https://api.weather.gov/products/")
    below = resolve(rule("2026-11-07T15:00:00Z", "<", "60"), 1_794_139_200, fetcher=fetcher(now))
    assert isinstance(below, Proposal)
    assert below.outcome == "no"


def test_waits_while_only_preliminary_reports_exist() -> None:
    result = resolve(
        rule("2026-11-07T05:00:00Z"), 1_794_031_200, fetcher=fetcher("2026-11-07T06:00:00Z")
    )
    assert isinstance(result, Wait)
    assert "no final report" in result.reason


class VerifyingGateway:
    """One closed Denver market with automatic review and a live proposal."""

    def __init__(self, rule: Rulebook, proposed_at: str) -> None:
        self.rule = rule
        self.proposed_at = proposed_at
        self.verifications: list[dict[str, Any]] = []

    def markets(self) -> list[dict[str, Any]]:
        return [{"market": self.rule.market, "state": "closed"}]

    def rules(self, market: int) -> dict[str, Any]:
        return {
            "rulebook": self.rule.raw,
            "rules_hash": self.rule.hash,
            "journal": {"matches": True},
        }

    def resolutions(self, market: int) -> list[dict[str, Any]]:
        return [
            {
                "resolution": 9,
                "state": "proposed",
                "outcome": "yes",
                "evidence": {"retrieved_at": self.proposed_at},
            }
        ]

    def propose(self, market: int, body: Any) -> dict[str, Any]:  # noqa: ANN401
        raise AssertionError("already proposed")

    def verify(self, resolution: int, body: Any) -> dict[str, Any]:  # noqa: ANN401
        self.verifications.append(dict(body))
        return {
            "resolution": resolution,
            "state": "confirmed",
            "dispute_until": "2026-11-08T13:00:00Z",
        }


def test_a_proposal_with_automatic_review_is_verified_once_by_a_later_fetch(tmp_path: Path) -> None:
    raw = dict(rule("2026-11-07T15:00:00Z").raw)
    raw |= {
        "schema": 2,
        "review": "automatic",
        "event": {
            "slug": "denver-high-2026-11-06",
            "title": "Denver high?",
            "tags": [],
            "exclusive": False,
            "outcome": "Yes",
        },
    }
    automatic = validate(raw)
    gw = VerifyingGateway(automatic, "2026-11-08T11:49:00Z")
    said: list[str] = []
    store = EvidenceStore(tmp_path)
    now = "2026-11-08T12:00:00Z"
    verified: set[int] = set()
    # Six minutes after the proposal's fetch: too soon, and nothing is fetched.
    cli.run_once(
        gw, store, 1_794_138_900, None, fetcher(now), lambda e, **f: said.append(e), verified
    )
    assert gw.verifications == []
    assert "verify_wait" in said
    # Eleven minutes after: verified by a fresh fetch, once.
    cli.run_once(
        gw, store, 1_794_139_200, None, fetcher(now), lambda e, **f: said.append(e), verified
    )
    cli.run_once(
        gw, store, 1_794_139_200, None, fetcher(now), lambda e, **f: said.append(e), verified
    )
    assert len(gw.verifications) == 1
    assert gw.verifications[0]["evidence"]["outcome"] == "yes"
    assert said.count("verified") == 1
