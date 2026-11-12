"""The lister service: lists the next two days of every recurring series (docs/catalog.md).

    python -m brier_services.catalog run --gateway URL --key-id N --key-file PATH [--every S]

Each pass, for each of the next two UTC days and each series, it asks the gateway whether
the day's event exists; if not, it lists the event's markets one by one. Temperature events
need the NWS forecast high to centre their ranges; a city whose forecast is unavailable is
skipped and tried again on the next pass. It runs as a user with the ``lister`` role, which can
list only markets like these.
"""

from __future__ import annotations

import datetime as dt
import json
from collections.abc import Callable
from typing import Any, Protocol

from brier_services.catalog import templates
from brier_services.catalog.forecast import ForecastHigh
from brier_services.client import ApiError
from brier_services.resolution.fetch import FetchError
from brier_services.resolution.sources import MalformedResponseError

DAYS_AHEAD = (1, 2)


class Api(Protocol):
    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any: ...  # noqa: ANN401 - JSON


def _log(event: str, **fields: Any) -> None:  # noqa: ANN401 - JSON fields
    print(json.dumps({"event": event, **fields}, sort_keys=True), flush=True)


def events_for(
    day: dt.date,
    forecast: ForecastHigh,
    say: Callable[..., None],
    low: ForecastHigh | None = None,
) -> list[list[dict[str, Any]]]:
    """Every series' event for the day, as lists of rulebooks (daily lows only with ``low``)."""
    out: list[list[dict[str, Any]]] = []
    for city in templates.CITIES:
        try:
            high = forecast(city, day)
        except (FetchError, MalformedResponseError, OSError) as e:
            say("forecast_unavailable", city=city.slug, day=day.isoformat(), error=str(e))
            continue
        if high is None:
            say(
                "forecast_unavailable",
                city=city.slug,
                day=day.isoformat(),
                error="no daytime period",
            )
            continue
        out.append(templates.temperature(city, day, high))
        if low is not None:
            try:
                forecast_low = low(city, day)
            except (FetchError, MalformedResponseError, OSError) as e:
                say("forecast_unavailable", city=city.slug, day=day.isoformat(), error=str(e))
                forecast_low = None
            if forecast_low is not None:
                out.append(templates.temperature(city, day, forecast_low, low=True))
    out += templates.earthquakes(day)
    out.append(templates.geomagnetic(day))
    return out


def _exists(api: Api, slug: str) -> bool:
    try:
        api.request("GET", f"/v1/events/{slug}")
    except ApiError as e:
        if e.status == 404:
            return False
        raise
    return True


def run_once(
    api: Api,
    now: dt.datetime,
    forecast: ForecastHigh,
    say: Callable[..., None] = _log,
    low: ForecastHigh | None = None,
) -> list[str]:
    """One pass; returns the slugs of the events it listed."""
    listed: list[str] = []
    today = now.astimezone(dt.UTC).date()
    for ahead in DAYS_AHEAD:
        day = today + dt.timedelta(days=ahead)
        for event in events_for(day, forecast, say, low):
            slug = event[0]["event"]["slug"]
            try:
                if _exists(api, slug):
                    continue
                for rulebook in event:
                    api.request(
                        "POST",
                        "/v1/admin/markets",
                        {
                            "rulebook": rulebook,
                            "reason": f"series listing: {rulebook['event']['series']}",
                        },
                    )
            except ApiError as e:
                say("list_failed", slug=slug, status=e.status, error=str(e))
                continue
            listed.append(slug)
            say("listed", slug=slug, markets=len(event))
    return listed
