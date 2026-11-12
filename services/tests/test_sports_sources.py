"""balldontlie (NBA, NFL) and football-data.org (soccer) final margins, on recorded responses
(services/tests/fixtures/resolution/SOURCES.md)."""

from __future__ import annotations

import json
from fractions import Fraction
from pathlib import Path

import pytest

from brier_services.resolution import balldontlie, football_data
from brier_services.resolution.fetch import Fetcher, Request, Response
from brier_services.resolution.resolver import Proposal, resolve
from brier_services.resolution.sources import MalformedResponseError, Missing
from brier_services.rules import Provider, Rulebook, find_series, validate

FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
NBA = (FIXTURES / "balldontlie_nba_21718052.json").read_bytes()  # home 102, visitors 114
NFL = (FIXTURES / "balldontlie_nfl_1392335.json").read_bytes()  # home 27, visitors 10
PL = (FIXTURES / "football_data_560631.json").read_bytes()  # Liverpool 0, Arsenal 1


def rule(provider: str, series: str, game_id: str, starts: str, op: str = ">") -> Rulebook:
    found = find_series(Provider(provider), series)
    assert found is not None
    day = starts[:10]
    return validate(
        {
            "schema": 3,
            "market": 30001,
            "title": "Home team wins",
            "question": "Will the home team win?",
            "observation": day,
            "revision": "first_release",
            "void": ["no_value_by_deadline"],
            "source": {"provider": provider, "series": series, "url": found.url,
                       "game": {"id": game_id, "starts": starts}},
            "condition": {"transform": "level", "op": op, "threshold": "0"},
            "schedule": {
                "open": "2026-10-01T00:00:00Z",
                "close": starts,
                "release": "2026-11-10T00:00:00Z",
                "deadline": "2026-11-17T00:00:00Z",
            },
            "event": {"slug": "game", "title": "A game", "tags": ["sports"], "exclusive": True,
                      "outcome": "Home"},
            "review": "manual",
        }
    )  # fmt: skip


def serve(body: bytes, now: str = "2026-11-10T01:00:00Z") -> Fetcher:
    seen: list[Request] = []

    def fetch(request: Request) -> Response:
        seen.append(request)
        return Response(request.url, 200, body, now)

    fetch.seen = seen  # type: ignore[attr-defined]
    return fetch


@pytest.fixture(autouse=True)
def keys(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BALLDONTLIE_API_KEY", "test-bdl-key")
    monkeypatch.setenv("FOOTBALL_DATA_API_KEY", "test-fd-key")


def test_nba_margin_is_home_minus_away_including_overtime() -> None:
    r = rule("balldontlie", "NBA/MARGIN", "21718052", "2026-11-07T22:00:00Z")
    assert balldontlie.parse(NBA, r) == Fraction(-12)


def test_nfl_margin_reads_the_nfl_game() -> None:
    r = rule("balldontlie", "NFL/MARGIN", "1392335", "2026-11-02T01:20:00Z")
    assert balldontlie.parse(NFL, r) == Fraction(17)
    req = balldontlie.request(r)
    assert req.url == "https://api.balldontlie.io/nfl/v1/games/1392335"
    assert req.headers == (("Authorization", "test-bdl-key"),)


def test_soccer_margin_is_full_time_home_minus_away() -> None:
    r = rule("football_data", "PL/MARGIN", "560631", "2026-11-01T16:30:00Z")
    assert football_data.parse(PL, r) == Fraction(-1)
    assert football_data.request(r).headers == (("X-Auth-Token", "test-fd-key"),)


def test_a_draw_market_resolves_with_equality() -> None:
    draw = rule("football_data", "PL/MARGIN", "560631", "2026-11-01T16:30:00Z", op="==")
    proposal = resolve(draw, 1_794_300_000, fetcher=serve(PL))
    assert isinstance(proposal, Proposal)
    assert proposal.outcome == "no"


def test_the_key_never_reaches_the_evidence() -> None:
    r = rule("balldontlie", "NBA/MARGIN", "21718052", "2026-11-07T22:00:00Z")
    fetcher = serve(NBA)
    proposal = resolve(r, 1_794_300_000, fetcher=fetcher)
    assert isinstance(proposal, Proposal)
    assert proposal.outcome == "no"  # the home team lost by 12
    assert "test-bdl-key" not in json.dumps(proposal.evidence)


def test_the_wrong_game_is_refused() -> None:
    r = rule("balldontlie", "NBA/MARGIN", "21718053", "2026-11-07T22:00:00Z")
    with pytest.raises(MalformedResponseError):
        balldontlie.parse(NBA, r)


def test_a_game_on_another_day_stays_missing() -> None:
    r = rule("balldontlie", "NBA/MARGIN", "21718052", "2026-11-05T22:00:00Z")
    assert isinstance(balldontlie.parse(NBA, r), Missing)


def test_an_unfinished_or_postponed_game_is_missing() -> None:
    r = rule("balldontlie", "NBA/MARGIN", "21718052", "2026-11-07T22:00:00Z")
    live = json.loads(NBA)
    live["data"]["status_state"] = "in"
    assert isinstance(balldontlie.parse(json.dumps(live).encode(), r), Missing)
    postponed = json.loads(NBA)
    postponed["data"]["postponed"] = True
    assert isinstance(balldontlie.parse(json.dumps(postponed).encode(), r), Missing)
    s = rule("football_data", "PL/MARGIN", "560631", "2026-11-01T16:30:00Z")
    off = json.loads(PL)
    off["status"] = "POSTPONED"
    assert isinstance(football_data.parse(json.dumps(off).encode(), s), Missing)


def test_a_league_match_with_extra_time_is_refused() -> None:
    s = rule("football_data", "PL/MARGIN", "560631", "2026-11-01T16:30:00Z")
    odd = json.loads(PL)
    odd["score"]["duration"] = "EXTRA_TIME"
    with pytest.raises(MalformedResponseError):
        football_data.parse(json.dumps(odd).encode(), s)


def test_a_missing_key_is_an_error_not_a_request(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("BALLDONTLIE_API_KEY")
    r = rule("balldontlie", "NBA/MARGIN", "21718052", "2026-11-07T22:00:00Z")
    with pytest.raises(MalformedResponseError):
        balldontlie.request(r)
