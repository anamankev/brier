"""balldontlie (api.balldontlie.io): one NBA or NFL game's final margin, home points minus
away points, including overtime.

The rulebook names the game by balldontlie's ID and pins its scheduled start. The game is read
by ID; the response must be that game and start within a day of the pinned start (a game moved
to another day is not the game the market asked about, and stays missing until the deadline
voids the market). Only a game whose state is final has a value.

balldontlie's terms forbid settling a wager on its data alone without independent
verification, so every result is verified from an independent source (second_source.py) before
payout, and the gateway checks the verifying evidence names a different provider
(docs/data-licenses.md).
"""

from __future__ import annotations

import os
from fractions import Fraction

from brier_services.resolution.fetch import Request
from brier_services.resolution.sources import (
    MalformedResponseError,
    Missing,
    Reading,
    field,
    strict_json,
)
from brier_services.rules import Rulebook, instant

HOST = "api.balldontlie.io"
KEY_ENV = "BALLDONTLIE_API_KEY"
MOVED_SECONDS = 86_400  # a start further than a day from the pinned one is a different game day
PATHS = {"NBA/MARGIN": "/v1/games/", "NFL/MARGIN": "/nfl/v1/games/"}


def _game(rule: Rulebook) -> dict[str, str]:
    if rule.game is None:
        msg = "a per-game rulebook without a game"
        raise MalformedResponseError(msg)
    return rule.game


def request(rule: Rulebook, api_key: str | None = None, now: int | None = None) -> Request:
    key = os.environ.get(KEY_ENV)
    if not key:
        msg = f"{KEY_ENV} is not set"
        raise MalformedResponseError(msg)
    path = PATHS.get(rule.series.id)
    if path is None:
        msg = f"no balldontlie path for {rule.series.id}"
        raise MalformedResponseError(msg)
    return Request(f"https://{HOST}{path}{_game(rule)['id']}", headers=(("Authorization", key),))


def _start(game: dict[str, object], series: str) -> int | None:
    raw = game.get("datetime") if series == "NBA/MARGIN" else game.get("date")
    if not isinstance(raw, str):
        return None
    return instant(raw[:19] + "Z")


def parse(body: bytes, rule: Rulebook) -> Reading:
    """Home points minus away points once the game is final."""
    pinned = _game(rule)
    game = field(strict_json(body), "data", dict)
    if str(field(game, "id", int)) != pinned["id"]:
        msg = f"asked for game {pinned['id']} and got {game.get('id')!r}"
        raise MalformedResponseError(msg)
    if game.get("postponed") is True:
        return Missing("the game was postponed")
    started, scheduled = _start(game, rule.series.id), instant(pinned["starts"])
    if started is None or scheduled is None:
        msg = "the game has no readable start time"
        raise MalformedResponseError(msg)
    if abs(started - scheduled) > MOVED_SECONDS:
        return Missing("the game was moved to another day")
    if field(game, "status_state", str) != "final":
        return Missing(f"the game is not final ({game.get('status')!r})")
    home = field(game, "home_team_score", int)
    away = field(game, "visitor_team_score", int)
    return Fraction(home - away)
