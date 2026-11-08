"""Fair probabilities for the launch markets (docs/gates/G7-plan.md), from the same free public
sources the resolvers read. Each function returns P(the rule resolves YES) for the rule as
written, including how the resolver rounds the value.

The models are deliberately simple, and each is documented where it is defined:
- 10-year yield: a daily random walk, sigma from the last 250 daily changes.
- CPI 12-month change: each unpublished month adds a log change drawn from that calendar month's
  history (10 years, 2020-21 left out), year-on-year computed as the resolver does; Monte Carlo.
- Unemployment rate: a monthly random walk, sigma from the median absolute deviation over 10 years.
- Central Park maximum: climatology over 30 years, days within +/-7 of the date.
- FOMC upper limit: no model, so no quotes (no free, licensed futures or OIS data).
"""

from __future__ import annotations

import datetime as dt
import itertools
import math
import random
import statistics
from collections.abc import Mapping, Sequence
from statistics import NormalDist

STANDARD = NormalDist()


def _rounded_tail(op: str, threshold: float, mean: float, sd: float, places: int) -> float:
    """P(round(V, places) op threshold) for V ~ N(mean, sd²), rounding half up."""
    h = 0.5 * 10**-places
    if sd <= 0:
        value = math.floor(mean / (2 * h) + 0.5) * 2 * h
        return float(
            {
                ">": value > threshold,
                ">=": value >= threshold,
                "<": value < threshold,
                "<=": value <= threshold,
            }[op]
        )
    at_least = 1 - STANDARD.cdf((threshold - h - mean) / sd)  # P(rounded >= threshold)
    above = 1 - STANDARD.cdf((threshold + h - mean) / sd)  # P(rounded > threshold)
    return {">=": at_least, ">": above, "<": 1 - at_least, "<=": 1 - above}[op]


def trading_days_between(start: dt.date, end: dt.date) -> int:
    """Weekdays after ``start`` up to and including ``end`` (holidays are not removed: a slight
    overstatement of the variance, which widens the price toward 50%)."""
    days = 0
    d = start
    while d < end:
        d += dt.timedelta(days=1)
        if d.weekday() < 5:
            days += 1
    return days


def yield_probability(
    history: Sequence[tuple[dt.date, float]], target: dt.date, op: str, threshold: float
) -> float:
    """10-year yield on ``target``: a random walk in daily steps from the latest value."""
    values = [v for _, v in sorted(history)]
    changes = [b - a for a, b in itertools.pairwise(values)][-250:]
    if len(changes) < 60:
        msg = "need at least 60 daily changes"
        raise ValueError(msg)
    last_date, last = sorted(history)[-1]
    n = trading_days_between(last_date, target)
    sd = statistics.pstdev(changes) * math.sqrt(max(n, 0))
    return _rounded_tail(op, threshold, last, sd, 2)


def rate_probability(
    history: Sequence[tuple[dt.date, float]], target: dt.date, op: str, threshold: float
) -> float:
    """Unemployment rate for the month of ``target``: a monthly random walk; sigma from the median
    absolute deviation of monthly changes over 10 years (so April 2020 does not dominate)."""
    series = sorted(history)
    changes = [b - a for (_, a), (_, b) in itertools.pairwise(series)][-120:]
    median = statistics.median(changes)
    sigma = 1.4826 * statistics.median([abs(c - median) for c in changes])
    last_date, last = series[-1]
    months = (target.year - last_date.year) * 12 + (target.month - last_date.month)
    return _rounded_tail(op, threshold, last, sigma * math.sqrt(max(months, 0)), 1)


def cpi_probability(
    index: Mapping[tuple[int, int], float],
    target: tuple[int, int],
    op: str,
    threshold: float,
    draws: int = 20_000,
    seed: int = 1,
) -> float:
    """The 12-month change for month ``target`` = (index_t / index_{t-12} - 1) x 100, rounded
    to one decimal, with each month not yet published drawn from its calendar month's history."""

    def shift(ym: tuple[int, int], k: int) -> tuple[int, int]:
        n = ym[0] * 12 + ym[1] - 1 + k
        return n // 12, n % 12 + 1

    base = shift(target, -12)
    if base not in index:
        msg = f"no index for {base}, the year-ago month"
        raise ValueError(msg)
    last = max(index)
    if last >= target:
        value = round((index[target] / index[base] - 1) * 100 + 1e-12, 1)
        return float(
            {
                ">": value > threshold,
                ">=": value >= threshold,
                "<": value < threshold,
                "<=": value <= threshold,
            }[op]
        )
    # Log changes by calendar month over the last 10 years, leaving out 2020-21.
    by_month: dict[int, list[float]] = {m: [] for m in range(1, 13)}
    for ym, v in index.items():
        prev = shift(ym, -1)
        if prev in index and last[0] - 10 <= ym[0] and ym[0] not in (2020, 2021):
            by_month[ym[1]].append(math.log(v / index[prev]))
    steps = []
    ym = last
    while ym < target:
        ym = shift(ym, 1)
        sample = by_month[ym[1]]
        if len(sample) < 3:
            msg = f"too little history for calendar month {ym[1]}"
            raise ValueError(msg)
        steps.append((statistics.fmean(sample), statistics.stdev(sample)))
    rng = random.Random(seed)  # noqa: S311 - a seeded simulation, not cryptography
    hits = 0
    for _ in range(draws):
        log_level = math.log(index[last]) + sum(rng.gauss(m, s) for m, s in steps)
        change = (math.exp(log_level) / index[base] - 1) * 100
        value = math.floor(change * 10 + 0.5) / 10
        hits += {
            ">": value > threshold,
            ">=": value >= threshold,
            "<": value < threshold,
            "<=": value <= threshold,
        }[op]
    return hits / draws


def climatology_probability(
    history: Mapping[dt.date, int],
    target: dt.date,
    op: str,
    threshold: float,
    years: int = 30,
    window: int = 7,
) -> float:
    """Central Park maximum (°F) on ``target``: the share of the last ``years`` years in which
    a day within +/-``window`` days of the same calendar day met the rule."""
    hits = total = 0
    for back in range(1, years + 1):
        try:
            centre = target.replace(year=target.year - back)
        except ValueError:  # 29 February
            centre = target.replace(year=target.year - back, day=28)
        for k in range(-window, window + 1):
            v = history.get(centre + dt.timedelta(days=k))
            if v is None:
                continue
            total += 1
            hits += {
                ">": v > threshold,
                ">=": v >= threshold,
                "<": v < threshold,
                "<=": v <= threshold,
            }[op]
    if total < 100:
        msg = "too little climatology"
        raise ValueError(msg)
    # Shrink toward 50% by one pseudo-observation each way, so no market is quoted at 0% or 100%.
    return (hits + 1) / (total + 2)
