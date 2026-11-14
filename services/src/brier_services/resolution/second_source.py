"""An independent second source for NBA and NFL final scores, to verify balldontlie's.

balldontlie's terms forbid settling a wager on its data alone without independent
verification (docs/data-licenses.md). The resolver proposes from balldontlie; this module
re-computes the same game's final margin from open data that does not come from balldontlie,
and the gateway accepts the verification only when its evidence names a different provider.

- **NBA:** sportsdataverse's daily team game logs (from the NBA's own statistics), a release
  file in github.com/sportsdataverse/sportsdataverse-data (MIT licence), one row per team per
  game: date, matchup ("BOS vs. ORL" at home, "ORL @ BOS" away), result and points.
- **NFL:** nflverse's games file (github.com/nflverse/nfldata), one row per game: date, teams,
  and final scores.

A game is matched by its two teams and its date. Game logs use the local date while the
rulebook's observation is the UTC date of the start, so an evening game in the Americas can
be one day earlier; both days are searched and exactly one game must match.
"""

from __future__ import annotations

import csv
import datetime as dt
import io
from fractions import Fraction

from brier_services.resolution.fetch import Request
from brier_services.resolution.sources import MalformedResponseError, Missing, Reading
from brier_services.rules import Rulebook

NBA_LOGS = (
    "https://github.com/sportsdataverse/sportsdataverse-data/releases/download/"
    "nba_stats_team_game_logs/team_game_logs_{season}.csv"
)
NFL_GAMES = "https://raw.githubusercontent.com/nflverse/nfldata/master/data/games.csv"
PROVIDER = {"NBA/MARGIN": "sportsdataverse", "NFL/MARGIN": "nflverse"}
# balldontlie code (as in Brier's slugs) -> the second source's, where they differ.
NFL_CODES = {"LAR": "LA", "WSH": "WAS", "JAC": "JAX"}


def teams(rule: Rulebook) -> tuple[str, str]:
    """(away, home) team codes from the event slug, e.g. "nba-orl-bos-2026-11-17"."""
    slug = str((rule.raw.get("event") or {}).get("slug", ""))
    parts = slug.split("-")
    if len(parts) < 6 or parts[0] not in ("nba", "nfl"):
        msg = f"cannot read the teams from the event slug {slug!r}"
        raise MalformedResponseError(msg)
    return parts[1].upper(), parts[2].upper()


def _days(rule: Rulebook) -> set[str]:
    day = dt.date.fromisoformat(rule.observation)
    return {day.isoformat(), (day - dt.timedelta(days=1)).isoformat()}


def nba_season(observation: str) -> int:
    """The NBA season a date falls in, named by the year it ends (Oct 2026 - Jun 2027: 2027)."""
    day = dt.date.fromisoformat(observation)
    return day.year + 1 if day.month >= 8 else day.year


def request(rule: Rulebook) -> Request:
    if rule.series.id == "NBA/MARGIN":
        return Request(NBA_LOGS.format(season=nba_season(rule.observation)))
    if rule.series.id == "NFL/MARGIN":
        return Request(NFL_GAMES)
    msg = f"no independent source for {rule.series.id}"
    raise MalformedResponseError(msg)


def _rows(body: bytes) -> list[dict[str, str]]:
    try:
        return list(csv.DictReader(io.StringIO(body.decode("utf-8"))))
    except (UnicodeDecodeError, csv.Error) as e:
        msg = f"unreadable CSV: {e}"
        raise MalformedResponseError(msg) from e


def _int(row: dict[str, str], key: str) -> int:
    try:
        return int(float(row[key]))
    except (KeyError, ValueError) as e:
        msg = f"no readable {key!r} in {row.get('game_id', row)!r}"
        raise MalformedResponseError(msg) from e


def _nba(body: bytes, rule: Rulebook) -> Reading:
    away, home = teams(rule)
    days = _days(rule)
    games: dict[str, dict[str, dict[str, str]]] = {}
    for row in _rows(body):
        if str(row.get("game_date", ""))[:10] in days:
            games.setdefault(row.get("game_id", ""), {})[row.get("team_abbreviation", "")] = row
    found = [g for g in games.values() if set(g) == {away, home}]
    if not found:
        return Missing(f"{away} at {home} is not in the NBA game logs yet")
    if len(found) > 1:
        msg = f"{len(found)} games between {away} and {home} on {sorted(days)}"
        raise MalformedResponseError(msg)
    game = found[0]
    if not game[home].get("matchup", "").startswith(f"{home} vs."):
        msg = f"{home} was not at home: {game[home].get('matchup')!r}"
        raise MalformedResponseError(msg)
    if not (game[home].get("wl") and game[away].get("wl")):
        return Missing(f"{away} at {home} is not final in the NBA game logs")
    return Fraction(_int(game[home], "pts") - _int(game[away], "pts"))


def _nfl(body: bytes, rule: Rulebook) -> Reading:
    away, home = (NFL_CODES.get(t, t) for t in teams(rule))
    days = _days(rule)
    found = [
        r
        for r in _rows(body)
        if r.get("gameday") in days and r.get("away_team") == away and r.get("home_team") == home
    ]
    if not found:
        return Missing(f"{away} at {home} is not in the nflverse games file")
    if len(found) > 1:
        msg = f"{len(found)} games {away} at {home} on {sorted(days)}"
        raise MalformedResponseError(msg)
    game = found[0]
    if not (game.get("home_score") and game.get("away_score")):
        return Missing(f"{away} at {home} has no final score in the nflverse games file yet")
    return Fraction(_int(game, "home_score") - _int(game, "away_score"))


def parse(body: bytes, rule: Rulebook) -> Reading:
    """Home points minus away points for the rulebook's game, from the second source."""
    if rule.series.id == "NBA/MARGIN":
        return _nba(body, rule)
    if rule.series.id == "NFL/MARGIN":
        return _nfl(body, rule)
    msg = f"no independent source for {rule.series.id}"
    raise MalformedResponseError(msg)
