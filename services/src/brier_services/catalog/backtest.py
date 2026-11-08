"""The registered backtest of the catalogue's pricing models (docs/notes/catalog-models.md).

    python -m brier_services.catalog.backtest [--out var/catalog-backtest]

Downloads are cached under ``--out/raw`` by URL, so a rerun reads the same bytes.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import random
import statistics
import sys
import time
import urllib.request
from collections import defaultdict
from collections.abc import Callable, Sequence
from fractions import Fraction
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

from brier_services.catalog import models as m
from brier_services.catalog.templates import CITIES
from brier_services.resolution import nws
from brier_services.resolution.values import round_half_up

# GHCN-Daily station and IANA time zone of each city's official station.
STATIONS = {
    "nyc": ("USW00094728", "America/New_York"),
    "chicago": ("USW00094846", "America/Chicago"),
    "los-angeles": ("USW00023174", "America/Los_Angeles"),
    "miami": ("USW00012839", "America/New_York"),
    "boston": ("USW00014739", "America/New_York"),
    "seattle": ("USW00024233", "America/Los_Angeles"),
    "denver": ("USW00003017", "America/Denver"),
    "atlanta": ("USW00013874", "America/New_York"),
    "phoenix": ("USW00023183", "America/Phoenix"),
    "washington-dc": ("USW00013743", "America/New_York"),
    "austin": ("USW00013904", "America/Chicago"),
    "dallas": ("USW00003927", "America/Chicago"),
}
HOSTS = {
    "www.ncei.noaa.gov",
    "previous-runs-api.open-meteo.com",
    "earthquake.usgs.gov",
    "api.weather.gov",
}
FIT = (dt.date(2025, 11, 1), dt.date(2026, 6, 30))
TEST = (dt.date(2026, 7, 1), dt.date(2026, 10, 31))
QUAKE_FIT = (dt.date(2024, 11, 1), dt.date(2025, 10, 31))
QUAKE_TEST = (dt.date(2025, 11, 1), dt.date(2026, 10, 31))
CLIMATE_YEARS = (1996, 2025)
RESAMPLES = 1000
SEED = 20261108


def fetcher(cache: Path) -> Callable[[str], Any]:
    cache.mkdir(parents=True, exist_ok=True)

    def get(url: str) -> Any:  # noqa: ANN401 - JSON
        parts = urlsplit(url)
        if parts.scheme != "https" or parts.hostname not in HOSTS:
            msg = f"refusing {url}"
            raise ValueError(msg)
        path = cache / (hashlib.sha256(url.encode()).hexdigest() + ".json")
        if not path.exists():
            req = urllib.request.Request(url, headers={"User-Agent": "brier-backtest/1"})  # noqa: S310 - checked above
            for attempt in range(4):
                try:
                    with urllib.request.urlopen(req, timeout=120) as res:  # noqa: S310
                        path.write_bytes(res.read())
                    break
                except OSError:
                    if attempt == 3:
                        raise
                    time.sleep(5 * (attempt + 1))
        return json.loads(path.read_bytes())

    return get


def days(span: tuple[dt.date, dt.date]) -> list[dt.date]:
    return [span[0] + dt.timedelta(days=i) for i in range((span[1] - span[0]).days + 1)]


def ghcn_tmax(get: Callable[[str], Any], station: str) -> dict[dt.date, int]:
    """Whole °F by date, converted as the NOAA resolver converts."""
    url = (
        "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries"
        f"&stations={station}&startDate={CLIMATE_YEARS[0]}-01-01&endDate={TEST[1].isoformat()}"
        "&dataTypes=TMAX&units=metric&format=json"
    )
    out = {}
    for row in get(url):
        raw = row.get("TMAX")
        if isinstance(raw, str) and raw.strip():
            celsius = Fraction(raw.strip())
            out[dt.date.fromisoformat(row["DATE"])] = int(
                round_half_up(celsius * Fraction(9, 5) + 32, 0)
            )
    return out


def forecast_highs(
    get: Callable[[str], Any], lat: float, lon: float, tz: str
) -> dict[dt.date, int]:
    """Day-ahead forecast high, whole °F, by local date."""
    url = (
        "https://previous-runs-api.open-meteo.com/v1/forecast"
        f"?latitude={lat}&longitude={lon}&hourly=temperature_2m_previous_day1"
        f"&temperature_unit=fahrenheit&timezone={tz.replace('/', '%2F')}"
        f"&start_date={FIT[0].isoformat()}&end_date={TEST[1].isoformat()}"
    )
    hourly = get(url)["hourly"]
    by_day: dict[dt.date, list[float]] = defaultdict(list)
    missing: set[dt.date] = set()
    for stamp, value in zip(hourly["time"], hourly["temperature_2m_previous_day1"], strict=True):
        day = dt.date.fromisoformat(stamp[:10])
        if value is None:
            missing.add(day)
        else:
            by_day[day].append(value)
    return {
        d: int(round_half_up(Fraction(str(max(v))), 0))
        for d, v in by_day.items()
        if d not in missing and len(v) >= 23
    }


def climatology(history: dict[dt.date, int], day: dt.date) -> list[int]:
    target = day.timetuple().tm_yday
    out = []
    for d, v in history.items():
        if CLIMATE_YEARS[0] <= d.year <= CLIMATE_YEARS[1]:
            gap = abs(d.timetuple().tm_yday - target)
            if min(gap, 365 - gap) <= 7:
                out.append(v)
    return out


def bootstrap(diffs: Sequence[float], rng: random.Random) -> list[float]:
    n = len(diffs)
    means = sorted(statistics.fmean(rng.choice(diffs) for _ in range(n)) for _ in range(RESAMPLES))
    return [means[int(0.025 * (RESAMPLES - 1))], means[int(0.975 * (RESAMPLES - 1))]]


def temperature(get: Callable[[str], Any], rng: random.Random) -> dict[str, Any]:
    per_city: dict[str, Any] = {}
    pooled: list[tuple[float, int]] = []
    for city in CITIES:
        station, tz = STATIONS[city.slug]
        actual = ghcn_tmax(get, station)
        forecast = forecast_highs(get, city.latitude, city.longitude, tz)
        errors = [actual[d] - forecast[d] for d in days(FIT) if d in actual and d in forecast]
        bias, sd = statistics.fmean(errors), statistics.stdev(errors)
        model_scores, clim_scores, diffs = [], [], []
        for d in days(TEST):
            if d not in actual or d not in forecast:
                continue
            ranges = m.temperature_ranges(forecast[d])
            outcome = m.outcome_index(actual[d], ranges)
            p_model = m.normal_probs(forecast[d] + bias, sd, ranges)
            p_clim = m.empirical_probs(climatology(actual, d), ranges)
            a, b = m.brier(p_model, outcome), m.brier(p_clim, outcome)
            model_scores.append(a)
            clim_scores.append(b)
            diffs.append(a - b)
            pooled += [(p, int(i == outcome)) for i, p in enumerate(p_model)]
        per_city[city.slug] = {
            "station": station,
            "fit_days": len(errors),
            "bias": bias,
            "sd": sd,
            "test_days": len(model_scores),
            "model_brier": statistics.fmean(model_scores),
            "climatology_brier": statistics.fmean(clim_scores),
            "difference_ci95": bootstrap(diffs, rng),
            "model_better": statistics.fmean(model_scores) < statistics.fmean(clim_scores),
        }
    bins: list[dict[str, Any]] = []
    for k in range(10):
        lo, hi = k / 10, (k + 1) / 10
        inside = [(p, o) for p, o in pooled if lo <= p < hi or (k == 9 and p == 1.0)]
        if inside:
            mean_p = statistics.fmean(p for p, _ in inside)
            freq = statistics.fmean(o for _, o in inside)
            bins.append(
                {
                    "bin": [lo, hi],
                    "n": len(inside),
                    "mean_forecast": mean_p,
                    "observed": freq,
                    "within_0.05": abs(freq - mean_p) <= 0.05,
                }
            )
    counted = [b for b in bins if b["n"] >= 100]
    better = sum(1 for c in per_city.values() if c["model_better"])
    return {
        "cities": per_city,
        "calibration_bins": bins,
        "T1": {"cities_better": better, "supported": better >= 10},
        "T2": {"bins_counted": len(counted), "supported": all(b["within_0.05"] for b in counted)},
    }


def report_check(get: Callable[[str], Any]) -> dict[str, Any]:
    """Recent NWS climate-report maxima against GHCN-Daily, wherever both exist."""
    checked = agreed = 0
    disagreements = []
    for city in CITIES:
        station, _ = STATIONS[city.slug]
        listing = get(
            f"https://api.weather.gov/products?type=CLI&location={city.location}&limit=20"
        )
        for item in listing["@graph"]:
            text = get(f"https://api.weather.gov/products/{item['id']}")["productText"]
            summary = nws._SUMMARY.search(text)
            if summary is None:
                continue
            month = nws.MONTHS.index(summary.group(1)) + 1
            day = dt.date(int(summary.group(3)), month, int(summary.group(2)))
            value = nws.report_maximum(text, day.isoformat())
            if not isinstance(value, Fraction):
                continue
            url = (
                "https://www.ncei.noaa.gov/access/services/data/v1?dataset=daily-summaries"
                f"&stations={station}&startDate={day.isoformat()}&endDate={day.isoformat()}"
                "&dataTypes=TMAX&units=metric&format=json"
            )
            rows = get(url)
            if not rows or not rows[0].get("TMAX", "").strip():
                continue
            ghcn = int(round_half_up(Fraction(rows[0]["TMAX"].strip()) * Fraction(9, 5) + 32, 0))
            checked += 1
            if ghcn == int(value):
                agreed += 1
            else:
                disagreements.append(
                    {"city": city.slug, "day": day.isoformat(), "report": int(value), "ghcn": ghcn}
                )
    return {"checked": checked, "agreed": agreed, "disagreements": disagreements}


def earthquakes(get: Callable[[str], Any], rng: random.Random) -> dict[str, Any]:
    url = (
        "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&eventtype=earthquake"
        f"&starttime={QUAKE_FIT[0].isoformat()}T00:00:00&endtime={QUAKE_TEST[1].isoformat()}T23:59:59.999"
        "&minmagnitude=5&orderby=time-asc&limit=20000"
    )
    doc = get(url)
    counts: dict[dt.date, int] = defaultdict(int)
    for f in doc["features"]:
        counts[dt.datetime.fromtimestamp(f["properties"]["time"] / 1000, dt.UTC).date()] += 1
    fit = [counts[d] for d in days(QUAKE_FIT)]
    mean, var = statistics.fmean(fit), statistics.variance(fit)
    nb = m.negative_binomial_probs(mean, var, m.QUAKE_RANGES)
    po = m.poisson_probs(mean, m.QUAKE_RANGES)
    uniform = [0.25] * 4
    scores: dict[str, list[float]] = {"negative_binomial": [], "poisson": [], "uniform": []}
    for d in days(QUAKE_TEST):
        o = m.outcome_index(counts[d], m.QUAKE_RANGES)
        scores["negative_binomial"].append(m.brier(nb, o))
        scores["poisson"].append(m.brier(po, o))
        scores["uniform"].append(m.brier(uniform, o))
    means = {k: statistics.fmean(v) for k, v in scores.items()}
    nb_vs_po = [a - b for a, b in zip(scores["negative_binomial"], scores["poisson"], strict=True)]
    nb_vs_un = [a - b for a, b in zip(scores["negative_binomial"], scores["uniform"], strict=True)]
    test_counts = [counts[d] for d in days(QUAKE_TEST)]
    return {
        "events": len(doc["features"]),
        "fit": {"mean": mean, "variance": var},
        "test": {
            "mean": statistics.fmean(test_counts),
            "variance": statistics.variance(test_counts),
        },
        "probabilities": {"negative_binomial": nb, "poisson": po},
        "brier": means,
        "Q1": {
            "difference_ci95": bootstrap(nb_vs_po, rng),
            "supported": means["negative_binomial"] < means["poisson"],
        },
        "Q2": {
            "difference_ci95": bootstrap(nb_vs_un, rng),
            "supported": means["negative_binomial"] < means["uniform"],
        },
    }


def explore_earthquakes(get: Callable[[str], Any], rng: random.Random) -> dict[str, Any]:
    """Exploratory, chosen after the registered results (docs/notes/catalog-models.md §5):
    alternatives to the registered count model, scored on the same test year."""
    url = (
        "https://earthquake.usgs.gov/fdsnws/event/1/query?format=geojson&eventtype=earthquake"
        f"&starttime={QUAKE_FIT[0].isoformat()}T00:00:00&endtime={QUAKE_TEST[1].isoformat()}T23:59:59.999"
        "&minmagnitude=5&orderby=time-asc&limit=20000"
    )
    counts: dict[dt.date, int] = defaultdict(int)
    sixes: dict[dt.date, int] = defaultdict(int)
    for f in get(url)["features"]:
        day = dt.datetime.fromtimestamp(f["properties"]["time"] / 1000, dt.UTC).date()
        counts[day] += 1
        if f["properties"]["mag"] >= 6:
            sixes[day] += 1
    fit = [counts[d] for d in days(QUAKE_FIT)]
    cut = sorted(fit)[int(0.99 * (len(fit) - 1))]
    trimmed = [c for c in fit if c <= cut]
    models = {
        "registered_negative_binomial": m.negative_binomial_probs(
            statistics.fmean(fit), statistics.variance(fit), m.QUAKE_RANGES
        ),
        "poisson": m.poisson_probs(statistics.fmean(fit), m.QUAKE_RANGES),
        "empirical_frequencies": m.empirical_probs(fit, m.QUAKE_RANGES),
        "negative_binomial_without_top_1pct_days": m.negative_binomial_probs(
            statistics.fmean(trimmed), statistics.variance(trimmed), m.QUAKE_RANGES
        ),
        "uniform": [0.25] * 4,
    }
    test = days(QUAKE_TEST)
    scores = {
        name: [m.brier(p, m.outcome_index(counts[d], m.QUAKE_RANGES)) for d in test]
        for name, p in models.items()
    }
    best = min(scores, key=lambda k: statistics.fmean(scores[k]))
    six_rate = sum(1 for d in days(QUAKE_FIT) if sixes[d] > 0) / len(days(QUAKE_FIT))
    six_scores = [(six_rate - (1.0 if sixes[d] > 0 else 0.0)) ** 2 for d in test]
    base_six = sum(1 for d in test if sixes[d] > 0) / len(test)
    return {
        "trim_at": cut,
        "probabilities": models,
        "brier": {k: statistics.fmean(v) for k, v in scores.items()},
        "best": best,
        "best_minus_uniform_ci95": bootstrap(
            [a - b for a, b in zip(scores[best], scores["uniform"], strict=True)], rng
        ),
        "m6": {"fit_rate": six_rate, "test_rate": base_six, "brier": statistics.fmean(six_scores)},
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.catalog.backtest")
    parser.add_argument("--out", type=Path, default=Path("var/catalog-backtest"))
    parser.add_argument("--explore", action="store_true", help="the exploratory follow-ups only")
    args = parser.parse_args(argv)
    get = fetcher(args.out / "raw")
    rng = random.Random(SEED)  # noqa: S311 - resampling
    if args.explore:
        explored = explore_earthquakes(get, rng)
        (args.out / "explore.json").write_text(
            json.dumps(explored, indent=1, sort_keys=True) + "\n"
        )
        print(json.dumps(explored, indent=1, sort_keys=True))
        return 0
    results = {
        "temperature": temperature(get, rng),
        "earthquakes": earthquakes(get, rng),
        "report_check": report_check(get),
    }
    (args.out / "results.json").write_text(json.dumps(results, indent=1, sort_keys=True) + "\n")
    t, q, c = results["temperature"], results["earthquakes"], results["report_check"]
    for slug, r in t["cities"].items():
        print(
            f"{slug:14} bias {r['bias']:+.2f} sd {r['sd']:.2f}  model {r['model_brier']:.3f}"
            f"  clim {r['climatology_brier']:.3f}  n={r['test_days']}"
        )
    print("T1", t["T1"], "T2", t["T2"])
    print("quakes", q["fit"], q["test"], q["brier"], "Q1", q["Q1"], "Q2", q["Q2"])
    print("report check", c["checked"], c["agreed"], c["disagreements"][:5])
    return 0


if __name__ == "__main__":
    sys.exit(main())
