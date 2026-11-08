"""Fair values for the recurring catalogue (docs/notes/catalog-models.md §6).

Each function returns P(value op threshold) for one market's condition; ranges are composed
from these by the caller as P(X >= low) - P(X > high).
"""

from __future__ import annotations

import datetime as dt
import json
from collections import defaultdict

from brier_services.catalog import models
from brier_services.catalog.templates import CITIES, City

# Per-city forecast bias and error spread (°F), fitted on 1 November 2025 to 30 June 2026
# (docs/notes/catalog-models.md §4; var/catalog-backtest/results.json).
TEMPERATURE_FIT: dict[str, tuple[float, float]] = {
    "atlanta": (2.58, 3.42),
    "austin": (2.47, 3.16),
    "boston": (1.76, 2.90),
    "chicago": (1.16, 3.06),
    "dallas": (2.16, 3.26),
    "denver": (-0.06, 3.50),
    "los-angeles": (1.37, 2.90),
    "miami": (1.63, 2.12),
    "nyc": (1.22, 3.53),
    "phoenix": (0.28, 2.56),
    "seattle": (1.60, 2.93),
    "washington-dc": (0.13, 3.52),
}
STORM_LEVELS = ("4.67", "5.67", "6.67")  # G1, G2, G3 lower edges in Kp


def city(location: str) -> City | None:
    return next((c for c in CITIES if c.location == location), None)


def open_meteo_url(c: City) -> str:
    return (
        f"https://api.open-meteo.com/v1/forecast?latitude={c.latitude}&longitude={c.longitude}"
        "&hourly=temperature_2m&temperature_unit=fahrenheit"
        f"&timezone={c.timezone.replace('/', '%2F')}"
        "&forecast_days=4"
    )


def forecast_high(body: bytes, day: str) -> float | None:
    """The day's forecast high (°F) from an Open-Meteo hourly forecast, if all 24 hours exist."""
    hourly = json.loads(body)["hourly"]
    values = [
        v
        for t, v in zip(hourly["time"], hourly["temperature_2m"], strict=True)
        if t.startswith(day)
    ]
    if len(values) < 24 or any(v is None for v in values):
        return None
    return float(max(values))


def temperature(c: City, forecast: float, op: str, threshold: float) -> float:
    """P(official whole-degree high op threshold) under Normal(forecast + bias, sd)."""
    bias, sd = TEMPERATURE_FIT[c.slug]
    mean = forecast + bias
    below = {
        ">=": threshold - 0.5,
        ">": threshold + 0.5,
        "<=": threshold + 0.5,
        "<": threshold - 0.5,
    }[op]
    cdf = models.normal_cdf(below, mean, sd)
    return 1.0 - cdf if op in {">=", ">"} else cdf


def daily_counts(body: bytes, minimum: float) -> dict[dt.date, int]:
    counts: dict[dt.date, int] = defaultdict(int)
    for f in json.loads(body)["features"]:
        if f["properties"]["mag"] >= minimum:
            counts[dt.datetime.fromtimestamp(f["properties"]["time"] / 1000, dt.UTC).date()] += 1
    return counts


def _holds(n: int, op: str, threshold: float) -> bool:
    if op == ">=":
        return n >= threshold
    if op == ">":
        return n > threshold
    if op == "<=":
        return n <= threshold
    return n < threshold


def earthquakes(
    counts: dict[dt.date, int], days: list[dt.date], op: str, threshold: float
) -> float:
    """The share of past days on which ``count op threshold`` held (with one pseudo-day each
    way, so no outcome is ever certain)."""
    hits = sum(1 for d in days if _holds(counts.get(d, 0), op, threshold))
    return (hits + 1) / (len(days) + 2)


def storm(probabilities: list[float], op: str, threshold: str) -> float | None:
    """P(day's maximum Kp op threshold) from SWPC's [no storm, G1, G2, G3+] probabilities, for
    the thresholds the templates use; None for any other."""
    exceed = {level: sum(probabilities[i + 1 :]) for i, level in enumerate(STORM_LEVELS)}
    match op:
        case ">=" if threshold in exceed:
            return exceed[threshold]
        case ">" if threshold in {"5.66", "6.66"}:
            return exceed["5.67" if threshold == "5.66" else "6.67"]
        case "<" if threshold in exceed:
            return 1.0 - exceed[threshold]
    return None
