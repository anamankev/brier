"""NWS forecast highs (api.weather.gov), which centre the temperature ranges.

A forecast is two requests: ``/points/{lat},{lon}`` names the forecast URL for the grid square
of the official station, and that forecast lists 12-hour periods. The high for a day is the
daytime period that starts on that local date.
"""

from __future__ import annotations

import datetime as dt
from collections.abc import Callable
from dataclasses import dataclass, field

from brier_services.catalog.templates import City
from brier_services.resolution.fetch import Fetcher, Request, fetch
from brier_services.resolution.sources import (
    MalformedResponseError,
    strict_json,
)
from brier_services.resolution.sources import (
    field as json_field,
)


def forecast_high(body: bytes, day: dt.date) -> int | None:
    """The forecast high in °F for the local date ``day``, or None if the forecast has none."""
    periods = json_field(json_field(strict_json(body), "properties", dict), "periods", list)
    for p in periods:
        start = dt.datetime.fromisoformat(json_field(p, "startTime", str))
        if json_field(p, "isDaytime", bool) and start.date() == day:
            if json_field(p, "temperatureUnit", str) != "F":
                msg = "expected a forecast in °F"
                raise MalformedResponseError(msg)
            return int(json_field(p, "temperature", int))
    return None


@dataclass
class Forecasts:
    """Forecast highs by city, with each city's forecast URL looked up once."""

    fetcher: Fetcher = fetch
    urls: dict[str, str] = field(default_factory=dict)

    def _body(self, url: str) -> bytes:
        response = self.fetcher(Request(url))
        if response.status != 200:
            msg = f"{response.url} answered HTTP {response.status}"
            raise MalformedResponseError(msg)
        return response.body

    def high(self, city: City, day: dt.date) -> int | None:
        if city.location not in self.urls:
            points = strict_json(
                self._body(f"https://api.weather.gov/points/{city.latitude},{city.longitude}")
            )
            self.urls[city.location] = json_field(
                json_field(points, "properties", dict), "forecast", str
            )
        return forecast_high(self._body(self.urls[city.location]), day)


ForecastHigh = Callable[[City, dt.date], int | None]
