"""Sports: game history from balldontlie and football-data.org, the Elo model the market maker
quotes with, and the backtest registered in docs/notes/sports-models.md.

    BALLDONTLIE_API_KEY=... FOOTBALL_DATA_API_KEY=... python -m brier_services.catalog.sports

The parameters below are the registered ones; they are not fitted. Downloads are cached under
``var/sports`` so a rerun reads the same games.
"""

from __future__ import annotations

import json
import os
import random
import time
import urllib.request
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[4]
CACHE = ROOT / "var" / "sports"
UA = "brier-catalog/1"

# Registered parameters (docs/notes/sports-models.md §3).
START = 1500.0
CARRY = 2 / 3  # between seasons a rating keeps two thirds of its distance from 1500
PARAMS = {"NBA": (20.0, 100.0), "NFL": (20.0, 48.0), "SOCCER": (20.0, 60.0)}  # (K, home advantage)
SOCCER_LEAGUES = ("PL", "PD", "SA", "BL1", "FL1")
SEED = 20261109
RESAMPLES = 1000
BALLDONTLIE_PAUSE = 12.5  # the free plan allows 5 requests a minute


@dataclass(frozen=True)
class Game:
    """One completed game: when, who, and the final score."""

    league: str
    season: int
    start: str  # ISO UTC
    home: str
    away: str
    home_score: int
    away_score: int

    @property
    def margin(self) -> int:
        return self.home_score - self.away_score


# ---- history -------------------------------------------------------------------------------


def _get(url: str, headers: dict[str, str]) -> Any:  # noqa: ANN401 - JSON
    req = urllib.request.Request(url, headers={"User-Agent": UA, **headers})  # noqa: S310 - fixed HTTPS hosts
    with urllib.request.urlopen(req, timeout=60) as res:  # noqa: S310
        return json.loads(res.read())


def _cached(name: str, load: Any) -> Any:  # noqa: ANN401 - JSON
    path = CACHE / name
    if path.exists():
        return json.loads(path.read_text())
    data = load()
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data))
    return data


def balldontlie_season(league: str, season: int) -> list[Game]:
    """Every final game of an NBA or NFL season, regular season and playoffs."""
    key = os.environ["BALLDONTLIE_API_KEY"]
    path = "/v1/games" if league == "NBA" else "/nfl/v1/games"

    def load() -> list[dict[str, Any]]:
        rows: list[dict[str, Any]] = []
        cursor: int | None = None
        while True:
            url = f"https://api.balldontlie.io{path}?seasons[]={season}&per_page=100"
            if cursor is not None:
                url += f"&cursor={cursor}"
            page = _get(url, {"Authorization": key})
            rows += page["data"]
            cursor = page.get("meta", {}).get("next_cursor")
            time.sleep(BALLDONTLIE_PAUSE)
            if not cursor:
                return rows

    rows = _cached(f"{league.lower()}-{season}.json", load)
    games = []
    for g in rows:
        if g.get("status_state") != "final" or g.get("postponed"):
            continue
        start = g.get("datetime") or g.get("date")
        games.append(
            Game(
                league,
                season,
                str(start),
                g["home_team"]["abbreviation"],
                g["visitor_team"]["abbreviation"],
                int(g["home_team_score"]),
                int(g["visitor_team_score"]),
            )
        )
    return sorted(games, key=lambda x: x.start)


def football_data_season(code: str, season: int) -> list[Game]:
    """Every finished league match of a season."""
    key = os.environ["FOOTBALL_DATA_API_KEY"]

    def load() -> dict[str, Any]:
        url = f"https://api.football-data.org/v4/competitions/{code}/matches?season={season}"
        data = _get(url, {"X-Auth-Token": key})
        time.sleep(7)  # 10 requests a minute on the free plan
        return dict(data)

    data = _cached(f"{code.lower()}-{season}.json", load)
    games = []
    for m in data["matches"]:
        full = m["score"]["fullTime"]
        if m["status"] != "FINISHED" or full["home"] is None:
            continue
        games.append(
            Game(
                code,
                season,
                m["utcDate"],
                str(m["homeTeam"]["id"]),
                str(m["awayTeam"]["id"]),
                int(full["home"]),
                int(full["away"]),
            )
        )
    return sorted(games, key=lambda x: x.start)


# ---- the model ------------------------------------------------------------------------------


class Elo:
    """Elo ratings with a home advantage, carried between seasons (registered parameters)."""

    def __init__(self, k: float, home_advantage: float) -> None:
        self.k, self.h = k, home_advantage
        self.ratings: dict[str, float] = {}
        self.season: int | None = None

    def _rating(self, team: str) -> float:
        return self.ratings.get(team, START)

    def expected(self, home: str, away: str) -> float:
        """The expected home score: the probability the home team wins, for NBA and NFL."""
        diff = self._rating(home) - self._rating(away) + self.h
        return 1 / (1 + 10 ** (-diff / 400))

    def new_season(self, season: int) -> None:
        if self.season is not None and season != self.season:
            self.ratings = {t: START + (r - START) * CARRY for t, r in self.ratings.items()}
        self.season = season

    def update(self, game: Game) -> None:
        self.new_season(game.season)
        e = self.expected(game.home, game.away)
        result = 1.0 if game.margin > 0 else 0.0 if game.margin < 0 else 0.5
        delta = self.k * (result - e)
        self.ratings[game.home] = self._rating(game.home) + delta
        self.ratings[game.away] = self._rating(game.away) - delta


def soccer_probabilities(e: float, draw_rate: float) -> tuple[float, float, float]:
    """(home, draw, away) from the expected home score and the league's draw rate."""
    return ((1 - draw_rate) * e, draw_rate, (1 - draw_rate) * (1 - e))


# ---- the backtest ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Scored:
    """One test game's Brier scores under the model and the base rate, and the model's
    probability with the outcome for calibration (binary sports only)."""

    model: float
    base: float
    p: float | None
    hit: float | None


def binary_backtest(league: str, fit: Iterable[Game], test: Iterable[Game]) -> list[Scored]:
    fit, test = list(fit), list(test)
    k, h = PARAMS[league]
    elo = Elo(k, h)
    for g in fit:
        elo.update(g)
    base = sum(g.margin > 0 for g in fit) / len(fit)
    out = []
    for g in test:
        elo.new_season(g.season)
        p = elo.expected(g.home, g.away)
        y = 1.0 if g.margin > 0 else 0.0
        out.append(Scored((p - y) ** 2, (base - y) ** 2, p, y))
        elo.update(g)
    return out


def soccer_backtest(fit: Iterable[Game], test: Iterable[Game]) -> list[Scored]:
    fit, test = list(fit), list(test)
    k, h = PARAMS["SOCCER"]
    elo = Elo(k, h)
    for g in fit:
        elo.update(g)
    n = len(fit)
    freq = (
        sum(g.margin > 0 for g in fit) / n,
        sum(g.margin == 0 for g in fit) / n,
        sum(g.margin < 0 for g in fit) / n,
    )
    out = []
    for g in test:
        elo.new_season(g.season)
        probs = soccer_probabilities(elo.expected(g.home, g.away), freq[1])
        y = (float(g.margin > 0), float(g.margin == 0), float(g.margin < 0))
        brier = sum((p - o) ** 2 for p, o in zip(probs, y, strict=True))
        base = sum((p - o) ** 2 for p, o in zip(freq, y, strict=True))
        out.append(Scored(brier, base, None, None))
        elo.update(g)
    return out


def bootstrap(rows: list[Scored]) -> tuple[float, float, float]:
    """Mean model-minus-base difference with a 95% bootstrap interval over games."""
    diffs = [r.model - r.base for r in rows]
    rng = random.Random(SEED)  # noqa: S311 - a reproducible resample, not a secret
    means = sorted(sum(rng.choice(diffs) for _ in diffs) / len(diffs) for _ in range(RESAMPLES))
    return sum(diffs) / len(diffs), means[int(0.025 * RESAMPLES)], means[int(0.975 * RESAMPLES) - 1]


def calibration(rows: list[Scored]) -> list[tuple[str, int, float, float]]:
    bins: dict[int, list[Scored]] = {}
    for r in rows:
        if r.p is not None:
            bins.setdefault(min(int(r.p * 10), 9), []).append(r)
    out = []
    for b in sorted(bins):
        rs = bins[b]
        mean_p = sum(r.p or 0 for r in rs) / len(rs)
        freq = sum(r.hit or 0 for r in rs) / len(rs)
        out.append((f"{b / 10:.1f}-{(b + 1) / 10:.1f}", len(rs), mean_p, freq))
    return out


def report() -> Iterator[str]:
    nba = [g for s in (2023, 2024) for g in balldontlie_season("NBA", s)]
    nba_test = balldontlie_season("NBA", 2025)
    nfl = [g for s in (2023, 2024) for g in balldontlie_season("NFL", s)]
    nfl_test = balldontlie_season("NFL", 2025)
    results = {
        "S1 NBA": binary_backtest("NBA", nba, nba_test),
        "S2 NFL": binary_backtest("NFL", nfl, nfl_test),
    }
    soccer: list[Scored] = []
    for code in SOCCER_LEAGUES:
        soccer += soccer_backtest(
            football_data_season(code, 2024), football_data_season(code, 2025)
        )
    results["S3 soccer"] = soccer
    yield "| Hypothesis | Test games | Model Brier | Base-rate Brier | Difference (95% CI) | Supported |"  # noqa: E501
    yield "|---|---|---|---|---|---|"
    for name, rows in results.items():
        mean, lo, hi = bootstrap(rows)
        m = sum(r.model for r in rows) / len(rows)
        b = sum(r.base for r in rows) / len(rows)
        verdict = "yes" if mean < 0 else "no"
        if mean < 0 and hi >= 0:
            verdict = "weak"
        ci = f"{mean:+.4f} ({lo:+.4f} to {hi:+.4f})"
        yield f"| {name} | {len(rows)} | {m:.4f} | {b:.4f} | {ci} | {verdict} |"
    yield ""
    yield "S4 calibration, NBA and NFL test games pooled:"
    yield "| Bin | Forecasts | Mean forecast | Observed | Within 0.05 |"
    yield "|---|---|---|---|---|"
    for label, n, p, f in calibration(results["S1 NBA"] + results["S2 NFL"]):
        within = "yes" if abs(p - f) <= 0.05 else "no"
        yield f"| {label} | {n} | {p:.3f} | {f:.3f} | {within if n >= 100 else 'n/a (<100)'} |"


if __name__ == "__main__":
    for line in report():
        print(line)
