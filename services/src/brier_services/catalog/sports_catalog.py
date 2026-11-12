"""Sports in the catalogue (G10 U6): upcoming games as events, and the Elo ratings the market
maker quotes them with (docs/notes/sports-models.md, docs/catalog.md).

- **Schedules** come from the same sources that settle the markets: balldontlie (NBA, NFL) and
  football-data.org (five soccer leagues).
- **Events.** Each game is one event. NBA and NFL games have two outcomes, one per team; an NFL
  tie resolves both No. Soccer matches have three: home win, draw, away win.
- **Trading** opens three days before kickoff and closes at kickoff.
- **Settlement** goes through the manual lane: an admin confirms each result against the league
  (balldontlie's terms require independent verification).
- **Pricing.** Ratings are the registered Elo model (`sports.Elo`, unchanged parameters), warmed
  up on the two previous seasons and updated with every finished game of the current one.

Market IDs: ``100,000,000 x league + (provider game ID mod 10,000,000) x 10 + outcome``, with
leagues 1-7. This keeps them apart from the daily catalogue (10,000,000 + ...) and from each
other: two games collide only if their provider IDs differ by a multiple of ten million, and
the gateway would refuse the second listing.
"""

from __future__ import annotations

import datetime as dt
import json
import os
import time
import urllib.parse
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from brier_services.catalog import sports
from brier_services.rules import Provider, find_series

LISTING_DAYS = 3  # list games starting within this many days
OPEN_BEFORE = dt.timedelta(days=3)
RESULT_AFTER = {
    "NBA": dt.timedelta(hours=4),
    "NFL": dt.timedelta(hours=5),
    "SOCCER": dt.timedelta(hours=3),
}
DEADLINE_AFTER = dt.timedelta(days=7)
HISTORY = 2  # previous seasons that warm the ratings up


@dataclass(frozen=True)
class League:
    code: int  # 1-7, the market ID's leading digit
    key: str  # registry prefix: NBA, NFL, PL, ...
    name: str
    provider: Provider
    sport: str  # basketball, football, soccer
    tag: str
    series: str  # the event series slug


LEAGUES = (
    League(1, "NBA", "NBA", Provider.BALLDONTLIE, "basketball", "nba", "nba-games"),
    League(2, "NFL", "NFL", Provider.BALLDONTLIE, "football", "nfl", "nfl-games"),
    League(3, "PL", "Premier League", Provider.FOOTBALL_DATA, "soccer", "premier-league", "premier-league"),  # noqa: E501
    League(4, "PD", "La Liga", Provider.FOOTBALL_DATA, "soccer", "la-liga", "la-liga"),
    League(5, "SA", "Serie A", Provider.FOOTBALL_DATA, "soccer", "serie-a", "serie-a"),
    League(6, "BL1", "Bundesliga", Provider.FOOTBALL_DATA, "soccer", "bundesliga", "bundesliga"),
    League(7, "FL1", "Ligue 1", Provider.FOOTBALL_DATA, "soccer", "ligue-1", "ligue-1"),
)  # fmt: skip


@dataclass(frozen=True)
class Team:
    key: str  # what the ratings are keyed by: abbreviation (NBA, NFL) or football-data ID
    short: str  # "Celtics", "Arsenal"
    full: str  # "Boston Celtics", "Arsenal FC"
    code: str  # three letters for the slug


@dataclass(frozen=True)
class Fixture:
    league: League
    game_id: str
    starts: dt.datetime  # UTC
    home: Team
    away: Team


# ---- schedules ----------------------------------------------------------------------------


def _bdl_team(t: dict[str, Any]) -> Team:
    return Team(str(t["abbreviation"]), str(t["name"]), str(t["full_name"]), str(t["abbreviation"]))


def _fd_team(t: dict[str, Any]) -> Team:
    return Team(
        str(t["id"]), str(t.get("shortName") or t["name"]), str(t["name"]), str(t.get("tla") or "")
    )


def _instant(s: str) -> dt.datetime:
    return dt.datetime.fromisoformat(s.replace("Z", "+00:00")).astimezone(dt.UTC)


def schedule(
    league: League, now: dt.datetime, get: Callable[[str, dict[str, str]], Any] | None = None
) -> list[Fixture]:
    """Games that start between now and ``LISTING_DAYS`` from now and have not started."""
    get = get or sports.fetch_json
    end = now + dt.timedelta(days=LISTING_DAYS)
    out: list[Fixture] = []
    if league.provider is Provider.BALLDONTLIE:
        key = os.environ["BALLDONTLIE_API_KEY"]
        path = "/v1/games" if league.key == "NBA" else "/nfl/v1/games"
        days = [
            (now.date() + dt.timedelta(days=i)).isoformat() for i in range(-1, LISTING_DAYS + 1)
        ]
        query = "&".join(f"dates[]={d}" for d in days)
        page = get(f"https://api.balldontlie.io{path}?{query}&per_page=100", {"Authorization": key})
        for g in page["data"]:
            start = g.get("datetime") if league.key == "NBA" else g.get("date")
            if not isinstance(start, str) or g.get("postponed"):
                continue
            starts = _instant(start)
            if now < starts <= end:
                out.append(
                    Fixture(
                        league,
                        str(g["id"]),
                        starts,
                        _bdl_team(g["home_team"]),
                        _bdl_team(g["visitor_team"]),
                    )
                )
    else:
        key = os.environ["FOOTBALL_DATA_API_KEY"]
        q = urllib.parse.urlencode(
            {"dateFrom": now.date().isoformat(), "dateTo": end.date().isoformat()}
        )
        url = f"https://api.football-data.org/v4/competitions/{league.key}/matches?{q}"
        for m in get(url, {"X-Auth-Token": key})["matches"]:
            if m["status"] not in {"SCHEDULED", "TIMED"}:
                continue
            starts = _instant(m["utcDate"])
            if now < starts <= end:
                out.append(
                    Fixture(
                        league,
                        str(m["id"]),
                        starts,
                        _fd_team(m["homeTeam"]),
                        _fd_team(m["awayTeam"]),
                    )
                )
    return sorted(out, key=lambda f: (f.starts, f.game_id))


# ---- templates ----------------------------------------------------------------------------


def _iso(t: dt.datetime) -> str:
    return t.astimezone(dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _day(t: dt.datetime) -> str:
    return f"{t.strftime('%b')} {t.day}"


def market_id(league: League, game_id: str, outcome: int) -> int:
    if not game_id.isdigit():
        msg = f"game ID {game_id!r} is not numeric"
        raise ValueError(msg)
    return league.code * 100_000_000 + (int(game_id) % 10_000_000) * 10 + outcome


def _slug_code(team: Team) -> str:
    code = "".join(c for c in team.code.lower() if c.isascii() and c.isalnum())
    return code or "".join(c for c in team.short.lower() if c.isascii() and c.isalnum())[:4]


def event(f: Fixture) -> list[dict[str, Any]]:
    """The game's rulebooks: one per outcome, all in one exclusive event."""
    lg = f.league
    soccer = lg.provider is Provider.FOOTBALL_DATA
    found = find_series(lg.provider, f"{lg.key}/MARGIN")
    if found is None:
        msg = f"{lg.key}/MARGIN is not in the registry"
        raise ValueError(msg)
    day = _day(f.starts)
    when = f"{f.starts.strftime('%B')} {f.starts.day}, {f.starts.year}"
    title = f"{f.home.short} vs {f.away.short}" if soccer else f"{f.away.short} at {f.home.short}"
    slug = f"{lg.tag}-{_slug_code(f.away)}-{_slug_code(f.home)}-{f.starts.date().isoformat()}"
    sport = "soccer" if soccer else lg.sport
    release = f.starts + RESULT_AFTER["SOCCER" if soccer else lg.key]
    outcomes: list[tuple[str, dict[str, str], str]] = [
        (
            f.home.short,
            {"op": ">", "threshold": "0"},
            f"Will {f.home.full} beat {f.away.full} on {when}"
            + (" (result after 90 minutes)?" if soccer else ", including overtime?"),
        )
    ]
    if soccer:
        outcomes.append(
            (
                "Draw",
                {"op": "==", "threshold": "0"},
                f"Will {f.home.full} and {f.away.full} draw on {when} (result after 90 minutes)?",
            )
        )
    outcomes.append(
        (
            f.away.short,
            {"op": "<", "threshold": "0"},
            f"Will {f.away.full} beat {f.home.full} on {when}"
            + (" (result after 90 minutes)?" if soccer else ", including overtime?"),
        )
    )
    rulebooks = []
    for i, (label, condition, question) in enumerate(outcomes):
        name = "Draw" if label == "Draw" else f"{label} win"
        rulebooks.append(
            {
                "schema": 3,
                "market": market_id(lg, f.game_id, i),
                "title": f"{title}, {day}: {name}"[:120],
                "question": question,
                "source": {
                    "provider": str(lg.provider),
                    "series": f"{lg.key}/MARGIN",
                    "url": found.url,
                    "game": {"id": f.game_id, "starts": _iso(f.starts)},
                },
                "observation": f.starts.date().isoformat(),
                "condition": {"transform": "level", **condition},
                "schedule": {
                    "open": _iso(f.starts - OPEN_BEFORE),
                    "close": _iso(f.starts),
                    "release": _iso(release),
                    "deadline": _iso(release + DEADLINE_AFTER),
                },
                "revision": "first_release",
                "void": ["no_value_by_deadline"],
                "event": {
                    "slug": slug,
                    "title": f"{title}, {day}"[:120],
                    "tags": sorted({sport, lg.tag}),
                    "series": lg.series,
                    "exclusive": True,
                    "outcome": label[:40],
                },
                "review": "manual",
            }
        )
    return rulebooks


# ---- ratings and fair values ---------------------------------------------------------------


def _season(league: League, today: dt.date) -> int:
    """The season a date belongs to, as the sources number it (the year it starts)."""
    starts_in = (
        8 if league.provider is Provider.FOOTBALL_DATA else (10 if league.key == "NBA" else 9)
    )
    return today.year if today.month >= starts_in else today.year - 1


def _games(league: League, season: int, *, current: bool) -> list[sports.Game]:
    if current:  # the season in progress changes: read it again, bypassing the cache
        name = f"{league.key.lower()}-{season}.json"
        (sports.CACHE / name).unlink(missing_ok=True)
    if league.provider is Provider.BALLDONTLIE:
        return sports.balldontlie_season(league.key, season)
    return sports.football_data_season(league.key, season)


class Ratings:
    """Elo ratings per league, refreshed at most every ``refresh`` seconds, and each upcoming
    game's fair probabilities for the market maker."""

    def __init__(self, state: Path, refresh: float = 6 * 3600) -> None:
        self.path = state / "sports-fair.json"
        self.refresh = refresh
        self.elos: dict[str, sports.Elo] = {}
        self.draw_rates: dict[str, float] = {}
        self.built: dict[str, float] = {}
        self.fair: dict[str, dict[str, float]] = (
            json.loads(self.path.read_text()) if self.path.exists() else {}
        )

    def elo(self, league: League, now: dt.datetime) -> sports.Elo:
        if time.time() - self.built.get(league.key, 0.0) < self.refresh and league.key in self.elos:
            return self.elos[league.key]
        season = _season(league, now.date())
        history = [
            g for s in range(season - HISTORY, season) for g in _games(league, s, current=False)
        ]
        current = _games(league, season, current=True)
        k, h = sports.PARAMS["SOCCER" if league.provider is Provider.FOOTBALL_DATA else league.key]
        elo = sports.Elo(k, h)
        for g in history + current:
            elo.update(g)
        elo.new_season(season)
        if history:
            self.draw_rates[league.key] = sum(g.margin == 0 for g in history) / len(history)
        self.elos[league.key] = elo
        self.built[league.key] = time.time()
        return elo

    def price(self, f: Fixture, now: dt.datetime) -> dict[str, float]:
        """P(home wins), P(draw), P(away wins) for a fixture; also kept for the maker."""
        e = self.elo(f.league, now).expected(f.home.key, f.away.key)
        if f.league.provider is Provider.FOOTBALL_DATA:
            home, draw, away = sports.soccer_probabilities(
                e, self.draw_rates.get(f.league.key, 0.25)
            )
        else:
            home, draw, away = e, 0.0, 1 - e
        probs = {"home": home, "draw": draw, "away": away}
        self.fair[f.game_id] = probs
        return probs

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(json.dumps(self.fair))

    def lookup(self, game_id: str) -> dict[str, float] | None:
        return self.fair.get(game_id)


# ---- the lister pass ----------------------------------------------------------------------


def run_once(
    api: Any,  # noqa: ANN401 - the lister's API (catalog.lister.Api)
    ratings: Ratings,
    now: dt.datetime,
    say: Callable[..., None],
    leagues: tuple[League, ...] = LEAGUES,
) -> list[str]:
    """List every upcoming game not listed yet, and price every upcoming game."""
    from brier_services.catalog.lister import _exists  # avoids an import cycle
    from brier_services.client import ApiError

    listed: list[str] = []
    for league in leagues:
        try:
            fixtures = schedule(league, now)
        except (OSError, KeyError, ValueError) as e:
            say("sports_schedule_failed", league=league.key, error=f"{type(e).__name__}: {e}")
            continue
        for f in fixtures:
            try:
                ratings.price(f, now)
            except (OSError, KeyError, ValueError) as e:
                say("sports_rating_failed", league=league.key, error=f"{type(e).__name__}: {e}")
            rulebooks = event(f)
            slug = rulebooks[0]["event"]["slug"]
            try:
                if _exists(api, slug):
                    continue
                for rb in rulebooks:
                    api.request(
                        "POST",
                        "/v1/admin/markets",
                        {"rulebook": rb, "reason": f"series listing: {league.series}"},
                    )
            except ApiError as e:
                say("list_failed", slug=slug, status=e.status, error=str(e))
                continue
            listed.append(slug)
    ratings.save()
    return listed
