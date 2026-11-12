"""football-data.org (api.football-data.org, v4): one league match's full-time margin, home
goals minus away goals.

The rulebook names the match by football-data's ID and pins its scheduled kickoff. The match is
read by ID; it must be that match, kick off within a day of the pinned time, and have finished
in regular time (league matches have no extra time; anything else is refused rather than
guessed). A postponed or cancelled match stays missing until the deadline voids the market.
Like every sports series, the result is confirmed by an admin before payout.
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

HOST = "api.football-data.org"
KEY_ENV = "FOOTBALL_DATA_API_KEY"
MOVED_SECONDS = 86_400
NOT_PLAYED = frozenset({"POSTPONED", "CANCELLED", "SUSPENDED"})


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
    return Request(
        f"https://{HOST}/v4/matches/{_game(rule)['id']}", headers=(("X-Auth-Token", key),)
    )


def parse(body: bytes, rule: Rulebook) -> Reading:
    """Home goals minus away goals at full time, once the match has finished."""
    pinned = _game(rule)
    match = strict_json(body)
    if str(field(match, "id", int)) != pinned["id"]:
        msg = f"asked for match {pinned['id']} and got {match.get('id')!r}"
        raise MalformedResponseError(msg)
    status = field(match, "status", str)
    if status in NOT_PLAYED:
        return Missing(f"the match is {status.lower()}")
    kickoff, scheduled = instant(field(match, "utcDate", str)), instant(pinned["starts"])
    if kickoff is None or scheduled is None:
        msg = "the match has no readable kickoff time"
        raise MalformedResponseError(msg)
    if abs(kickoff - scheduled) > MOVED_SECONDS:
        return Missing("the match was moved to another day")
    if status != "FINISHED":
        return Missing(f"the match has not finished ({status})")
    score = field(match, "score", dict)
    if field(score, "duration", str) != "REGULAR":
        msg = f"a league match that went to {score.get('duration')!r}"
        raise MalformedResponseError(msg)
    full = field(score, "fullTime", dict)
    return Fraction(field(full, "home", int) - field(full, "away", int))
