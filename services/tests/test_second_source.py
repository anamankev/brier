"""The independent second source for NBA and NFL results (verifying balldontlie): real game
logs and games files, the same final margin, and evidence that names its own provider."""

from __future__ import annotations

import datetime as dt
import urllib.request
from fractions import Fraction
from pathlib import Path

import pytest

from brier_services.catalog.sports_catalog import LEAGUES, Fixture, Team, event
from brier_services.resolution import fetch, second_source
from brier_services.resolution.fetch import FetchError, Request, Response
from brier_services.resolution.resolver import Proposal, verify_source
from brier_services.resolution.sources import MalformedResponseError, Missing
from brier_services.rules import Rulebook, validate

NBA, NFL = LEAGUES[0], LEAGUES[1]
FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
NBA_LOGS = (FIXTURES / "nba_team_game_logs_2027_slice.csv").read_bytes()
NFL_GAMES = (FIXTURES / "nfldata_games_2026_weeks_9_10.csv").read_bytes()


def game(league: object, away: str, home: str, starts: str) -> tuple[Rulebook, Rulebook]:
    """The home-win and away-win rulebooks for a game, as the catalogue lists them."""
    fixture = Fixture(
        league,  # type: ignore[arg-type]
        "18447001",
        dt.datetime.fromisoformat(starts),
        Team(home, home.title(), home, home),
        Team(away, away.title(), away, away),
    )
    home_rb, away_rb = (validate(rb) for rb in event(fixture))
    return home_rb, away_rb


def test_nba_margin_from_the_game_logs() -> None:
    # Cleveland won 115-107 at New York on 11 November (local date); the game started after
    # midnight UTC, so the rulebook's observation is the 12th.
    home_win, _ = game(NBA, "CLE", "NYK", "2026-11-12T00:30:00+00:00")
    assert second_source.parse(NBA_LOGS, home_win) == Fraction(-8)
    assert "team_game_logs_2027" in second_source.request(home_win).url


def test_nfl_margin_from_the_games_file() -> None:
    home_win, _ = game(NFL, "WAS", "NYG", "2026-11-13T01:15:00+00:00")
    assert second_source.parse(NFL_GAMES, home_win) == Fraction(21 - 24)


def test_an_unplayed_or_unknown_game_waits() -> None:
    later, _ = game(NFL, "KC", "ATL", "2026-11-15T18:00:00+00:00")
    assert isinstance(second_source.parse(NFL_GAMES, later), Missing), "listed, no score yet"
    absent, _ = game(NBA, "LAL", "BOS", "2026-11-12T00:30:00+00:00")
    assert isinstance(second_source.parse(NBA_LOGS, absent), Missing)


def test_the_home_team_must_really_be_at_home() -> None:
    swapped, _ = game(NBA, "NYK", "CLE", "2026-11-12T00:30:00+00:00")
    with pytest.raises(MalformedResponseError, match="not at home"):
        second_source.parse(NBA_LOGS, swapped)


def test_nba_season_is_named_by_its_end_year() -> None:
    assert second_source.nba_season("2026-11-12") == 2027
    assert second_source.nba_season("2027-04-02") == 2027
    assert second_source.nba_season("2027-10-25") == 2028


def test_verification_evidence_names_the_independent_provider() -> None:
    home_win, away_win = game(NBA, "CLE", "NYK", "2026-11-12T00:30:00+00:00")

    def fetcher(request: Request) -> Response:
        assert request.url.startswith("https://github.com/sportsdataverse/")
        return Response(request.url, 200, NBA_LOGS, "2026-11-13T09:00:00Z")

    now = int(dt.datetime(2026, 11, 13, 9, tzinfo=dt.UTC).timestamp())
    for rule, outcome in ((home_win, "no"), (away_win, "yes")):
        result = verify_source(rule, now, None, fetcher)
        assert isinstance(result, Proposal)
        assert result.outcome == outcome
        assert result.evidence["provider"] == "sportsdataverse"
        assert result.evidence["verifies"] == "balldontlie"
        assert result.evidence["value"] == "-8"


def test_only_a_github_release_download_may_redirect() -> None:
    handler = fetch._NoRedirects()

    def follow(src: str, dst: str, *, auth: bool = False) -> object:
        req = urllib.request.Request(src, headers={"Authorization": "x"} if auth else {})  # noqa: S310 - never opened
        return handler.redirect_request(req, None, 302, "Found", None, dst)  # type: ignore[arg-type]

    assert (
        follow(
            "https://github.com/o/r/releases/download/t/f.csv",
            "https://release-assets.githubusercontent.com/a/b",
        )
        is not None
    )
    for src, dst, auth in [
        ("https://github.com/o/r", "https://evil.example/x", False),
        (
            "https://api.balldontlie.io/v1/games/1",
            "https://release-assets.githubusercontent.com/a",
            False,
        ),
        ("https://github.com/o/r", "http://release-assets.githubusercontent.com/a", False),
        ("https://github.com/o/r", "https://release-assets.githubusercontent.com/a", True),
    ]:
        with pytest.raises(FetchError, match="refusing a redirect"):
            follow(src, dst, auth=auth)
