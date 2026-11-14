"""Sports listings and pricing (G10 U6): fixtures become valid schema 3 rulebooks, outcomes
cover every result exactly once, and the market maker reads each outcome's fair value."""

from __future__ import annotations

import datetime as dt
import time
from pathlib import Path
from typing import Any

import pytest

from brier_services.catalog import sports, sports_catalog
from brier_services.catalog.sports_catalog import LEAGUES, Fixture, Ratings, Team, event, market_id
from brier_services.marketmaker.service import FairValues
from brier_services.rules import validate

NBA, NFL, PL = LEAGUES[0], LEAGUES[1], LEAGUES[2]
KICKOFF = dt.datetime(2026, 11, 14, 0, 30, tzinfo=dt.UTC)
CELTICS = Team("BOS", "Celtics", "Boston Celtics", "BOS")
LAKERS = Team("LAL", "Lakers", "Los Angeles Lakers", "LAL")
ARSENAL = Team("57", "Arsenal", "Arsenal FC", "ARS")
CHELSEA = Team("61", "Chelsea", "Chelsea FC", "CHE")


def fixture(league: sports_catalog.League, game_id: str = "21718999") -> Fixture:
    if league is PL:
        return Fixture(league, "537001", KICKOFF, ARSENAL, CHELSEA)
    return Fixture(league, game_id, KICKOFF, CELTICS, LAKERS)


@pytest.mark.parametrize("league", [NBA, NFL, PL])
def test_every_listing_is_a_valid_rulebook(league: sports_catalog.League) -> None:
    rulebooks = event(fixture(league))
    checked = [validate(rb) for rb in rulebooks]
    assert len(checked) == (3 if league is PL else 2)
    assert len({rb["market"] for rb in rulebooks}) == len(rulebooks)
    assert len({rb["event"]["slug"] for rb in rulebooks}) == 1
    for rb in rulebooks:
        assert rb["schedule"]["close"] == "2026-11-14T00:30:00Z", "trading stops at kickoff"
        assert rb["review"] == "automatic", "an independent source verifies every result"
        assert rb["source"]["game"]["starts"] == "2026-11-14T00:30:00Z"


def test_outcomes_cover_every_margin_exactly_once() -> None:
    soccer = event(fixture(PL))
    ops = [(rb["condition"]["op"], rb["condition"]["threshold"]) for rb in soccer]
    assert ops == [(">", "0"), ("==", "0"), ("<", "0")]
    labels = [rb["event"]["outcome"] for rb in soccer]
    assert labels == ["Arsenal", "Draw", "Chelsea"]
    nba = event(fixture(NBA))
    assert [rb["event"]["outcome"] for rb in nba] == ["Celtics", "Lakers"]
    assert nba[0]["event"]["title"] == "Lakers at Celtics, Nov 14"
    assert nba[0]["event"]["slug"] == "nba-lal-bos-2026-11-14"


def test_market_ids_keep_leagues_apart() -> None:
    assert market_id(NBA, "21718052", 0) == 100_000_000 + 1_718_052 * 10
    assert market_id(PL, "537001", 2) == 300_000_000 + 537_001 * 10 + 2
    assert market_id(NBA, "21718052", 1) != market_id(NFL, "21718052", 1)
    with pytest.raises(ValueError, match="numeric"):
        market_id(NBA, "abc", 0)


def test_schedule_keeps_only_games_not_yet_started(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("BALLDONTLIE_API_KEY", "k")
    now = dt.datetime(2026, 11, 12, 12, 0, tzinfo=dt.UTC)

    def team(abbr: str) -> dict[str, str]:
        return {"abbreviation": abbr, "name": abbr.title(), "full_name": abbr}

    def game(gid: int, start: str, *, postponed: bool = False) -> dict[str, Any]:
        return {"id": gid, "datetime": start, "postponed": postponed,
                "home_team": team("BOS"), "visitor_team": team("LAL")}  # fmt: skip

    def get(url: str, headers: dict[str, str]) -> Any:  # noqa: ANN401 - JSON
        assert headers == {"Authorization": "k"}
        return {"data": [
            game(1, "2026-11-12T00:00:00.000Z"),  # already played
            game(2, "2026-11-13T00:30:00.000Z"),
            game(3, "2026-11-13T23:00:00.000Z", postponed=True),
            game(4, "2026-11-20T00:30:00.000Z"),  # beyond the listing window
        ]}  # fmt: skip

    fixtures = sports_catalog.schedule(NBA, now, get)
    assert [f.game_id for f in fixtures] == ["2"]


def test_the_maker_quotes_each_outcome_from_the_ratings(tmp_path: Path) -> None:
    ratings = Ratings(tmp_path)
    elo = sports.Elo(20, 100)
    elo.ratings = {"BOS": 1600.0, "LAL": 1450.0}
    ratings.elos["NBA"], ratings.built["NBA"] = elo, time.time()
    probs = ratings.price(fixture(NBA), KICKOFF)
    assert probs["home"] > 0.7
    assert probs["home"] + probs["away"] == pytest.approx(1.0)
    ratings.save()
    reread = Ratings(tmp_path)
    fair = FairValues(None, sports=reread.lookup)
    home, away = event(fixture(NBA))
    assert fair.probability(home) == pytest.approx(round(probs["home"], 12), abs=1e-9)
    assert fair.probability(away) == pytest.approx(probs["away"], abs=1e-9)
    assert FairValues(None).probability(home) is None, "no ratings, no quotes"


def test_soccer_probabilities_include_the_draw(tmp_path: Path) -> None:
    ratings = Ratings(tmp_path)
    ratings.elos["PL"], ratings.built["PL"] = sports.Elo(20, 60), time.time()
    ratings.draw_rates["PL"] = 0.25
    probs = ratings.price(fixture(PL), KICKOFF)
    assert probs["draw"] == 0.25
    assert sum(probs.values()) == pytest.approx(1.0)
    _, draw, _ = event(fixture(PL))
    fair = FairValues(None, sports=ratings.lookup)
    assert fair.probability(draw) == pytest.approx(0.25)
