"""Economy and rates in the catalogue (G10 U5): the next CPI and jobs report, the next Fed
decision, and next Friday's 10-year Treasury yield, each one event of ranges centred on the
latest published value (docs/catalog.md).

Every series here is machine-checkable (BLS and FRED publish one number), so these settle by
automatic review like the weather.

- **Release dates** for CPI and the Employment Situation come from FRED's release calendar
  (releases 10 and 50), which lists the next scheduled date.
- **FOMC meeting dates** are the Federal Reserve's published schedule, recorded below with its
  source; the statement is released at 2:00 p.m. Eastern on the second day.

Templates are pure: the same calendar and latest values always give the same rulebooks.
Market IDs follow the daily catalogue (``templates.market_id``) on the release or observation
day, with series codes 70-73.
"""

from __future__ import annotations

import datetime as dt
import json
import urllib.parse
from collections.abc import Callable
from decimal import ROUND_HALF_UP, Decimal
from typing import Any
from zoneinfo import ZoneInfo

from brier_services.catalog import templates
from brier_services.rules import Provider, find_series

EASTERN = ZoneInfo("America/New_York")
CPI_CODE, JOBS_CODE, FED_CODE, YIELD_CODE = 70, 71, 72, 73

# https://www.federalreserve.gov/monetarypolicy/fomccalendars.htm, read 12 November 2026: the
# second (decision) day of each scheduled meeting.
FOMC_DECISIONS = tuple(
    dt.date.fromisoformat(d)
    for d in (
        "2026-12-09",
        "2027-01-27",
        "2027-03-17",
        "2027-04-28",
        "2027-06-09",
        "2027-07-28",
        "2027-09-15",
        "2027-10-27",
        "2027-12-08",
    )
)
MONTHS = templates.MONTH_NAMES


def _iso(t: dt.datetime) -> str:
    return t.astimezone(dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _eastern(day: dt.date, hour: int, minute: int = 0) -> dt.datetime:
    return dt.datetime(day.year, day.month, day.day, hour, minute, tzinfo=EASTERN)


def _q(x: Decimal, places: str) -> Decimal:
    return x.quantize(Decimal(places), rounding=ROUND_HALF_UP)


def _source(provider: Provider, series: str) -> dict[str, str]:
    found = find_series(provider, series)
    if found is None:
        msg = f"{provider} {series} is not in the registry"
        raise ValueError(msg)
    return {"provider": str(provider), "series": series, "url": found.url}


def _rulebook(
    *,
    market: int,
    title: str,
    question: str,
    source: dict[str, str],
    observation: str,
    condition: dict[str, str],
    open_: dt.datetime,
    close: dt.datetime,
    release: dt.datetime,
    deadline: dt.datetime,
    event: dict[str, Any],
    void: list[str],
) -> dict[str, Any]:
    return {
        "schema": 3,
        "market": market,
        "title": title,
        "question": question,
        "source": source,
        "observation": observation,
        "condition": condition,
        "schedule": {
            "open": _iso(open_),
            "close": _iso(close),
            "release": _iso(release),
            "deadline": _iso(deadline),
        },
        "revision": "first_release",
        "void": void,
        "event": event,
        "review": "automatic",
    }


def _event(slug: str, title: str, tags: list[str], series: str, outcome: str) -> dict[str, Any]:
    return {
        "slug": slug,
        "title": title,
        "tags": sorted(tags),
        "series": series,
        "exclusive": True,
        "outcome": outcome,
    }


def _ranges(
    centre: Decimal, width: Decimal, tick: Decimal, unit: str
) -> list[tuple[str, dict[str, str]]]:
    """Five contiguous ranges at the series' precision (``tick``): two open ends and three
    inner ranges ``width`` wide, the middle one holding ``centre``. Every value falls in exactly
    one."""
    places = str(tick)
    mid_lo = _q(centre - width / 2 + tick, places)
    lo = mid_lo - width - tick  # the top of the lowest range

    def fmt(x: Decimal) -> str:
        return str(_q(x, places))

    out = [(f"{fmt(lo)}{unit} or lower", {"op": "<=", "threshold": fmt(lo)})]
    a = lo + tick
    for _ in range(3):
        b = a + width - tick
        out.append(
            (f"{fmt(a)}-{fmt(b)}{unit}", {"op": "between", "threshold": fmt(a), "upper": fmt(b)})
        )
        a = b + tick
    out.append((f"{fmt(a)}{unit} or higher", {"op": ">=", "threshold": fmt(a)}))
    return out


def cpi(release: dt.date, latest_yoy: Decimal) -> list[dict[str, Any]]:
    """The 12-month CPI change for the month before ``release``, in five ranges."""
    month = (release.replace(day=1) - dt.timedelta(days=1)).replace(day=1)
    name = f"{MONTHS[month.month - 1]} {month.year}"
    when = _eastern(release, 8, 30)
    centre = _q(latest_yoy, "0.1")
    out = []
    for i, (label, cond) in enumerate(_ranges(centre, Decimal("0.2"), Decimal("0.1"), "%")):
        out.append(
            _rulebook(
                market=templates.market_id(release, CPI_CODE, i),
                title=f"CPI inflation {label} for {name}",
                question=f"Will the 12-month change in CPI-U (not seasonally adjusted) for {name} be {label}, as the BLS first publishes it?",  # noqa: E501
                source=_source(Provider.BLS, "CUUR0000SA0"),
                observation=f"{month.year}-{month.month:02d}",
                condition={"transform": "pct_change_12m", **cond},
                open_=when - dt.timedelta(days=30),
                close=when - dt.timedelta(minutes=30),
                release=when,
                deadline=when + dt.timedelta(days=60),
                event=_event(
                    f"cpi-{month.year}-{month.month:02d}",
                    f"CPI inflation for {name}?",
                    ["economy", "inflation"],
                    "us-cpi",
                    label,
                ),
                void=["no_value_by_deadline", "source_discontinued"],
            )
        )
    return out


def jobs(release: dt.date, latest_rate: Decimal) -> list[dict[str, Any]]:
    """The unemployment rate for the month before ``release``: five values a tenth apart."""
    month = (release.replace(day=1) - dt.timedelta(days=1)).replace(day=1)
    name = f"{MONTHS[month.month - 1]} {month.year}"
    when = _eastern(release, 8, 30)
    c = _q(latest_rate, "0.1")
    t = Decimal("0.1")
    outcomes = [
        (f"{c - 2 * t}% or lower", {"op": "<=", "threshold": str(c - 2 * t)}),
        (f"{c - t}%", {"op": "==", "threshold": str(c - t)}),
        (f"{c}%", {"op": "==", "threshold": str(c)}),
        (f"{c + t}%", {"op": "==", "threshold": str(c + t)}),
        (f"{c + 2 * t}% or higher", {"op": ">=", "threshold": str(c + 2 * t)}),
    ]
    return [
        _rulebook(
            market=templates.market_id(release, JOBS_CODE, i),
            title=f"Unemployment rate {label} for {name}",
            question=f"Will the seasonally adjusted unemployment rate for {name} be {label}, as the BLS first publishes it?",  # noqa: E501
            source=_source(Provider.BLS, "LNS14000000"),
            observation=f"{month.year}-{month.month:02d}",
            condition={"transform": "level", **cond},
            open_=when - dt.timedelta(days=30),
            close=when - dt.timedelta(minutes=30),
            release=when,
            deadline=when + dt.timedelta(days=60),
            event=_event(
                f"unemployment-{month.year}-{month.month:02d}",
                f"Unemployment rate for {name}?",
                ["economy", "jobs"],
                "us-unemployment",
                label,
            ),
            void=["no_value_by_deadline", "source_discontinued"],
        )
        for i, (label, cond) in enumerate(outcomes)
    ]


def fed(decision: dt.date, upper: Decimal) -> list[dict[str, Any]]:
    """The federal funds target's upper limit the day after a decision: cut 50 or more, cut 25,
    no change, or hike."""
    q = Decimal("0.25")
    u = _q(upper, "0.01")
    when = _eastern(decision, 14, 0)
    after = decision + dt.timedelta(days=1)
    name = f"{MONTHS[decision.month - 1]} {decision.day}, {decision.year}"
    outcomes = [
        ("Cut 50+ bp", {"op": "<=", "threshold": str(u - 2 * q)}),
        ("Cut 25 bp", {"op": "==", "threshold": str(u - q)}),
        ("No change", {"op": "==", "threshold": str(u)}),
        ("Hike", {"op": ">=", "threshold": str(u + q)}),
    ]
    return [
        _rulebook(
            market=templates.market_id(decision, FED_CODE, i),
            title=f"Fed decision {name}: {label}",
            question=(
                f"After the FOMC decision of {name}, will the federal funds target range's upper limit for "  # noqa: E501
                f"{after.isoformat()} be {cond['op'].replace('==', 'exactly').replace('<=', 'at most').replace('>=', 'at least')} "  # noqa: E501
                f"{cond['threshold']}% (now {u}%)?"
            ),
            source=_source(Provider.FRED, "DFEDTARU"),
            observation=after.isoformat(),
            condition={"transform": "level", **cond},
            open_=when - dt.timedelta(days=42),
            close=when - dt.timedelta(minutes=5),
            release=when,
            deadline=when + dt.timedelta(days=30),
            event=_event(
                f"fed-decision-{decision.isoformat()}",
                f"Fed decision on {name}?",
                ["fed", "rates"],
                "fomc-decision",
                label,
            ),
            void=["no_value_by_deadline"],
        )
        for i, (label, cond) in enumerate(outcomes)
    ]


def treasury(friday: dt.date, latest: Decimal) -> list[dict[str, Any]]:
    """The 10-year yield for a Friday, in five ranges a tenth of a point wide."""
    name = f"{MONTHS[friday.month - 1]} {friday.day}"
    released = friday + dt.timedelta(days=3)  # H.15 for Friday appears the next business day
    when = _eastern(released, 16, 15)
    start = dt.datetime(friday.year, friday.month, friday.day, tzinfo=dt.UTC)
    out = []
    for i, (label, cond) in enumerate(
        _ranges(_q(latest, "0.01"), Decimal("0.10"), Decimal("0.01"), "%")
    ):
        out.append(
            _rulebook(
                market=templates.market_id(friday, YIELD_CODE, i),
                title=f"10-year yield {label} on {name}",
                question=f"Will the 10-year Treasury constant-maturity yield for {name}, {friday.year} be {label}?",  # noqa: E501
                source=_source(Provider.FRED, "DGS10"),
                observation=friday.isoformat(),
                condition={"transform": "level", **cond},
                open_=start - dt.timedelta(days=7),
                close=start,
                release=when,
                deadline=when + dt.timedelta(days=30),
                event=_event(
                    f"treasury-10y-{friday.isoformat()}",
                    f"10-year Treasury yield on {name}?",
                    ["rates", "treasury"],
                    "treasury-10y-weekly",
                    label,
                ),
                void=["no_value_by_deadline"],
            )
        )
    return out


# ---- inputs from FRED ----------------------------------------------------------------------


def _fred(path: str, params: dict[str, str], key: str, get: Callable[[str], Any]) -> Any:  # noqa: ANN401 - JSON
    q = urllib.parse.urlencode({**params, "api_key": key, "file_type": "json"})
    return get(f"https://api.stlouisfed.org/fred/{path}?{q}")


def latest(
    series: str, key: str, get: Callable[[str], Any], limit: int = 14
) -> list[tuple[str, Decimal]]:
    doc = _fred(
        "series/observations",
        {"series_id": series, "sort_order": "desc", "limit": str(limit)},
        key,
        get,
    )
    return [(o["date"], Decimal(o["value"])) for o in doc["observations"] if o["value"] != "."]


def next_release(
    release_id: int, today: dt.date, key: str, get: Callable[[str], Any]
) -> dt.date | None:
    doc = _fred(
        "release/dates",
        {
            "release_id": str(release_id),
            "realtime_start": today.isoformat(),
            "include_release_dates_with_no_data": "true",
            "sort_order": "asc",
            "limit": "10",
        },
        key,
        get,
    )
    dates = [dt.date.fromisoformat(d["date"]) for d in doc.get("release_dates", [])]
    return next((d for d in dates if d > today), None)


def events(now: dt.datetime, key: str, get: Callable[[str], Any]) -> list[list[dict[str, Any]]]:
    """Every economy and rates event open for listing now."""
    today = now.astimezone(dt.UTC).date()
    out: list[list[dict[str, Any]]] = []
    cpi_index = latest("CPIAUCNS", key, get)
    if len(cpi_index) >= 13:
        yoy = (cpi_index[0][1] / cpi_index[12][1] - 1) * 100
        release = next_release(10, today, key, get)
        if release:
            out.append(cpi(release, yoy))
    rate = latest("UNRATE", key, get, 1)
    release = next_release(50, today, key, get)
    if rate and release:
        out.append(jobs(release, rate[0][1]))
    upper = latest("DFEDTARU", key, get, 1)
    decision = next((d for d in FOMC_DECISIONS if d > today), None)
    if upper and decision and (decision - today).days <= 42:
        out.append(fed(decision, upper[0][1]))
    ten = latest("DGS10", key, get, 5)
    if ten:
        friday = today + dt.timedelta(days=(4 - today.weekday()) % 7 or 7)
        if (friday - today).days < 2:  # too close to trade: list the next one
            friday += dt.timedelta(days=7)
        out.append(treasury(friday, ten[0][1]))
    return out


def fetch_json(url: str) -> Any:  # noqa: ANN401 - JSON
    from brier_services.catalog.sports import fetch_json as get

    return get(url, {})


def run_once(api: Any, now: dt.datetime, key: str, say: Callable[..., None]) -> list[str]:  # noqa: ANN401
    """List every economy and rates event not listed yet."""
    from brier_services.catalog.lister import _exists
    from brier_services.client import ApiError

    listed: list[str] = []
    try:
        evs = events(now, key, fetch_json)
    except (OSError, KeyError, ValueError, json.JSONDecodeError) as e:
        say("economy_inputs_failed", error=f"{type(e).__name__}: {e}")
        return listed
    for rulebooks in evs:
        slug = rulebooks[0]["event"]["slug"]
        try:
            if _exists(api, slug):
                continue
            for rb in rulebooks:
                api.request(
                    "POST",
                    "/v1/admin/markets",
                    {"rulebook": rb, "reason": f"series listing: {rb['event']['series']}"},
                )
        except ApiError as e:
            say("list_failed", slug=slug, status=e.status, error=str(e))
            continue
        listed.append(slug)
    return listed
