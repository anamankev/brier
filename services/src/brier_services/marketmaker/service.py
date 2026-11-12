"""The reference market maker on the exchange (SPEC §8), through the public API only.

    python -m brier_services.marketmaker run --gateway URL --key-id N --key-file PATH

Each pass, for every open market with a fair-value model:
1. fair value from the model (inputs fetched from the public source, refreshed every 6 hours
   of real time; a model whose inputs are older than 24 hours, or failed to load, is stale);
2. quotes from ``quoting.quote`` with the maker's own position and the fraction of the market's
   trading window left;
3. the resting orders brought into line: an order at the wrong price, or less than half filled
   size, is cancelled and replaced; quotes are pulled when fair value is stale, the market is not
   open, or its close is less than 15 minutes away.

A user with no special role runs it, with the same limits as anyone. It logs one JSON line per
action and appends one metrics line per market per pass.
"""

from __future__ import annotations

import datetime as dt
import gzip
import json
import time
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Protocol

from brier_services.catalog import models as catalog_models
from brier_services.marketmaker import catalog_values as cv
from brier_services.marketmaker import fairvalue as fv
from brier_services.marketmaker.quoting import Params, quote
from brier_services.resolution import bls
from brier_services.resolution.fetch import FetchError, Request, Response, fetch
from brier_services.rules import instant

REFRESH = 6 * 3600
STALE = 24 * 3600
CLOSE_BUFFER = 15 * 60
CLAMP = (0.02, 0.98)
GHCN = "https://www.ncei.noaa.gov/pub/data/ghcn/daily/by_station/USW00094728.csv.gz"
SWPC_3DAY = "https://services.swpc.noaa.gov/text/3-day-geomag-forecast.txt"


class Api(Protocol):
    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any: ...  # noqa: ANN401 - JSON


@dataclass
class Inputs:
    """One model's inputs and when they were fetched (real time)."""

    data: Any
    fetched: float


@dataclass
class FairValues:
    """Fair probabilities for rulebooks, with each source's data cached and refreshed."""

    fred_key: str | None
    fetcher: Callable[[Request], Response] = fetch
    wall: Callable[[], float] = time.time
    cache: dict[str, Inputs] = field(default_factory=dict)
    # A game's fair probabilities (home, draw, away) by provider game ID, from the sports
    # ratings (catalog.sports_catalog.Ratings.lookup); None means no sports quotes.
    sports: Callable[[str], dict[str, float] | None] | None = None

    def _get(self, name: str, load: Callable[[], Any]) -> Any:  # noqa: ANN401 - model data
        now = self.wall()
        cached = self.cache.get(name)
        if cached is None or now - cached.fetched > REFRESH:
            try:
                self.cache[name] = Inputs(load(), now)
            except (FetchError, ValueError, KeyError, OSError) as e:
                if cached is None or now - cached.fetched > STALE:
                    msg = f"{name}: inputs unavailable ({e})"
                    raise StaleError(msg) from e
        return self.cache[name].data

    def _body(self, request: Request) -> bytes:
        response = self.fetcher(request)
        if response.status != 200:
            msg = f"HTTP {response.status} from {response.url}"
            raise FetchError(msg)
        return response.body

    def _bls_months(self, series: str) -> dict[tuple[int, int], float]:
        year = dt.datetime.fromtimestamp(self.wall(), dt.UTC).year
        doc = json.loads(self._body(bls.years(series, year - 9, year)))
        out = {}
        for row in doc["Results"]["series"][0]["data"]:
            if (
                row["period"].startswith("M")
                and row["period"] != "M13"
                and row["value"].strip() != "-"
            ):
                out[(int(row["year"]), int(row["period"][1:]))] = float(row["value"])
        return out

    def _fred_daily(self, series: str) -> list[tuple[dt.date, float]]:
        if not self.fred_key:
            msg = "FRED_API_KEY is not set"
            raise ValueError(msg)
        start = (
            dt.datetime.fromtimestamp(self.wall(), dt.UTC).date() - dt.timedelta(days=500)
        ).isoformat()
        url = (
            "https://api.stlouisfed.org/fred/series/observations?file_type=json"
            f"&series_id={series}&observation_start={start}&api_key={self.fred_key}"
        )
        doc = json.loads(self._body(Request(url)))
        return [
            (dt.date.fromisoformat(o["date"]), float(o["value"]))
            for o in doc["observations"]
            if o["value"] != "."
        ]

    def _ghcn_tmax_f(self) -> dict[dt.date, int]:
        text = gzip.decompress(self._body(Request(GHCN))).decode()
        out = {}
        for line in text.splitlines():
            f = line.split(",")
            if (
                len(f) >= 4 and f[2] == "TMAX" and not (len(f) > 5 and f[5])
            ):  # skip failed quality checks
                d = f[1]
                celsius = int(f[3]) / 10
                out[dt.date(int(d[:4]), int(d[4:6]), int(d[6:]))] = round(celsius * 9 / 5 + 32)
        return out

    def probability(self, rulebook: dict[str, Any]) -> float | None:
        """P(YES) for a rulebook, or None if Brier has no model for its series."""
        condition = rulebook["condition"]
        if condition["op"] == "==" and rulebook["source"]["provider"] not in {
            "balldontlie",
            "football_data",
        }:
            # P(X == t) = P(X >= t) - P(X > t), from any model that answers the two tails.
            condition = {**condition, "op": "between", "upper": condition["threshold"]}
        if condition["op"] == "between":
            # P(t <= X <= u) = P(X >= t) - P(X > u)
            at_least = self._model(rulebook, ">=", float(condition["threshold"]))
            above = self._model(rulebook, ">", float(condition["upper"]))
            if at_least is None or above is None:
                return None
            p = at_least - above
        else:
            model = self._model(rulebook, condition["op"], float(condition["threshold"]))
            if model is None:
                return None
            p = model
        return min(CLAMP[1], max(CLAMP[0], p))

    def _model(self, rulebook: dict[str, Any], op: str, threshold: float) -> float | None:
        """P(value op threshold) from the series' model, or None without one."""
        series = rulebook["source"]["series"]
        observation = rulebook["observation"]
        if series == "DGS10":
            history = self._get("DGS10", lambda: self._fred_daily("DGS10"))
            p = fv.yield_probability(history, dt.date.fromisoformat(observation), op, threshold)
        elif series == "CUUR0000SA0" and rulebook["condition"]["transform"] == "pct_change_12m":
            index = self._get("CPI", lambda: self._bls_months("CUUR0000SA0"))
            y, m = map(int, observation.split("-"))
            p = fv.cpi_probability(index, (y, m), op, threshold)
        elif series == "LNS14000000":
            rates = self._get("UNRATE", lambda: self._bls_months("LNS14000000"))
            y, m = map(int, observation.split("-"))
            history = [(dt.date(yy, mm, 1), v) for (yy, mm), v in rates.items()]
            p = fv.rate_probability(history, dt.date(y, m, 1), op, threshold)
        elif series == "USW00094728/TMAX":
            tmax = self._get("TMAX", self._ghcn_tmax_f)
            p = fv.climatology_probability(tmax, dt.date.fromisoformat(observation), op, threshold)
        else:
            return self._catalog_model(rulebook, op, threshold)
        return p

    def _catalog_model(self, rulebook: dict[str, Any], op: str, threshold: float) -> float | None:
        """The recurring catalogue's models (docs/notes/catalog-models.md §6), or None."""
        provider = rulebook["source"]["provider"]
        series = rulebook["source"]["series"]
        observation = rulebook["observation"]
        if provider == "nws":
            c = cv.city(series.removeprefix("CLI/"))
            if c is None:
                return None
            body = self._get(
                f"OPEN-METEO {c.location}", lambda: self._body(Request(cv.open_meteo_url(c)))
            )
            high = cv.forecast_high(body, observation)
            return None if high is None else cv.temperature(c, high, op, threshold)
        if series in {"WORLD/M5", "WORLD/M6"}:
            today = dt.datetime.fromtimestamp(self.wall(), dt.UTC).date()
            body = self._get("USGS M5 YEAR", lambda: self._body(Request(_usgs_year(today))))
            counts = cv.daily_counts(body, 5.0 if series == "WORLD/M5" else 6.0)
            days = [today - dt.timedelta(days=i) for i in range(1, 366)]
            return cv.earthquakes(counts, days, op, threshold)
        if provider in {"balldontlie", "football_data"}:
            game = rulebook["source"].get("game") or {}
            fair = self.sports(str(game.get("id"))) if self.sports else None
            if fair is None or threshold != 0:
                return None
            side = {">": "home", "<": "away", "==": "draw"}.get(op)
            return None if side is None else fair.get(side)
        if series == "KP/MAX":
            text = self._get("SWPC 3-DAY", lambda: self._body(Request(SWPC_3DAY)).decode())
            probs = catalog_models.storm_probabilities(text).get(observation)
            return None if probs is None else cv.storm(probs, op, f"{threshold:.2f}")
        return None  # the FOMC market and anything new: no model, no quotes


def _usgs_year(today: dt.date) -> str:
    return (
        "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&eventtype=earthquake"
        f"&starttime={(today - dt.timedelta(days=365)).isoformat()}T00:00:00"
        f"&endtime={today.isoformat()}T00:00:00&minmagnitude=5&orderby=time-asc&limit=20000"
    )


class StaleError(Exception):
    """A model's inputs could not be loaded and the cached copy is too old."""


@dataclass
class MarketMaker:
    """One maker account on the exchange."""

    api: Api
    fair: FairValues
    now: Callable[[], float] = time.time
    params: Params = field(default_factory=Params)
    say: Callable[..., None] = lambda event, **f: print(
        json.dumps({"event": event, **f}, sort_keys=True), flush=True
    )
    metrics: Path | None = None
    rules: dict[int, dict[str, Any]] = field(default_factory=dict)

    def _rulebook(self, market: int) -> dict[str, Any]:
        if market not in self.rules:
            self.rules[market] = self.api.request("GET", f"/v1/markets/{market}/rules")["rulebook"]
        return self.rules[market]

    def run_once(self) -> dict[int, dict[str, Any]]:
        """One pass over every market; returns what was decided per market."""
        markets = self.api.request("GET", "/v1/markets")["markets"]
        portfolio = self.api.request("GET", "/v1/portfolio")
        positions = {p["market"]: p["position"] for p in portfolio["positions"]}
        orders: dict[int, list[dict[str, Any]]] = {}
        for o in portfolio["open_orders"]:
            orders.setdefault(o["market"], []).append(o)
        now = self.now()
        decided: dict[int, dict[str, Any]] = {}
        for m in markets:
            if not m.get("title"):
                continue
            market = m["market"]
            rulebook = self._rulebook(market)
            open_at = instant(rulebook["schedule"]["open"]) or 0
            close_at = instant(rulebook["schedule"]["close"]) or 0
            why, p = None, None
            if m["state"] != "open":
                why = f"market {m['state']}"
            elif close_at - now < CLOSE_BUFFER:
                why = "close is near"
            else:
                try:
                    p = self.fair.probability(rulebook)
                except StaleError as e:
                    why = str(e)
                if p is None and why is None:
                    why = "no model"
            target: dict[str, int | None] = {"buy": None, "sell": None}
            inventory = positions.get(market, 0)
            if p is not None and why is None:
                tau = (close_at - now) / max(1, close_at - open_at)
                q = quote(p, inventory, tau, self.params)
                target = {"buy": q.bid, "sell": q.ask}
            self._reconcile(market, target, orders.get(market, []))
            decided[market] = {
                "fair": p,
                "inventory": inventory,
                "bid": target["buy"],
                "ask": target["sell"],
                "pulled": why,
            }
            if self.metrics is not None:
                with self.metrics.open("a") as f:
                    f.write(json.dumps({"t": now, "market": market, **decided[market]}) + "\n")
        return decided

    def _reconcile(
        self, market: int, target: dict[str, int | None], resting: list[dict[str, Any]]
    ) -> None:
        for side in ("buy", "sell"):
            mine = [o for o in resting if o["side"] == side]
            keep = None
            for o in mine:
                # Book prices are YES prices for both sides of the book.
                if (
                    keep is None
                    and target[side] is not None
                    and o["price"] == target[side]
                    and o["remaining"] * 2 >= self.params.size
                ):
                    keep = o
                    continue
                self.api.request("DELETE", f"/v1/orders/{o['order']}")
                self.say("cancel", market=market, order=o["order"], side=side)
            price = target[side]
            if keep is None and price is not None:
                r = self.api.request(
                    "POST",
                    "/v1/orders",
                    {"market": market, "side": side, "price": price, "qty": self.params.size},
                )
                self.say("quote", market=market, side=side, price=price, status=r.get("status"))
