"""Fair-value models for the recurring catalogue (docs/notes/catalog-models.md).

Outcomes are inclusive integer ranges ``(low, high)``, either end ``None`` for unbounded, as
the templates build them. Readings are whole numbers, so a continuous model's probability of
``(low, high)`` is its mass over ``[low - 0.5, high + 0.5]``.
"""

from __future__ import annotations

import itertools
import math
from collections.abc import Sequence

Range = tuple[int | None, int | None]


def normal_cdf(x: float, mean: float, sd: float) -> float:
    return 0.5 * (1.0 + math.erf((x - mean) / (sd * math.sqrt(2.0))))


def normal_probs(mean: float, sd: float, ranges: Sequence[Range]) -> list[float]:
    """Probability of each whole-number range under Normal(mean, sd)."""
    out = []
    for low, high in ranges:
        lo = 0.0 if low is None else normal_cdf(low - 0.5, mean, sd)
        hi = 1.0 if high is None else normal_cdf(high + 0.5, mean, sd)
        out.append(max(0.0, hi - lo))
    return out


def _count_probs(pmf: Sequence[float], ranges: Sequence[Range]) -> list[float]:
    """Range probabilities from a pmf over 0..len(pmf)-1; the tail beyond goes to an open top."""
    out = []
    for low, high in ranges:
        lo = 0 if low is None else max(0, low)
        if high is None:
            out.append(max(0.0, 1.0 - sum(pmf[:lo])))
        else:
            out.append(sum(pmf[lo : high + 1]))
    return out


def poisson_probs(mean: float, ranges: Sequence[Range], support: int = 200) -> list[float]:
    pmf = [math.exp(k * math.log(mean) - mean - math.lgamma(k + 1)) for k in range(support)]
    return _count_probs(pmf, ranges)


def negative_binomial_probs(
    mean: float, variance: float, ranges: Sequence[Range], support: int = 200
) -> list[float]:
    """Negative binomial with this mean and variance (method of moments); Poisson if the
    variance does not exceed the mean."""
    if variance <= mean:
        return poisson_probs(mean, ranges, support)
    p = mean / variance  # success probability
    r = mean * mean / (variance - mean)  # number of successes
    pmf = [
        math.exp(
            math.lgamma(k + r)
            - math.lgamma(k + 1)
            - math.lgamma(r)
            + r * math.log(p)
            + k * math.log1p(-p)
        )
        for k in range(support)
    ]
    return _count_probs(pmf, ranges)


def empirical_probs(
    values: Sequence[int], ranges: Sequence[Range], pseudo: float = 1.0
) -> list[float]:
    """Frequency of each range among ``values``, with ``pseudo`` counts added to each."""
    counts = [
        sum(1 for v in values if (low is None or v >= low) and (high is None or v <= high))
        for low, high in ranges
    ]
    total = sum(counts) + pseudo * len(ranges)
    return [(c + pseudo) / total for c in counts]


def outcome_index(value: int, ranges: Sequence[Range]) -> int:
    for i, (low, high) in enumerate(ranges):
        if (low is None or value >= low) and (high is None or value <= high):
            return i
    msg = f"{value} is in no range"
    raise ValueError(msg)


def brier(probs: Sequence[float], outcome: int) -> float:
    """Multi-outcome Brier score: the sum of squared errors over the outcomes."""
    return sum((p - (1.0 if i == outcome else 0.0)) ** 2 for i, p in enumerate(probs))


def temperature_ranges(forecast_high: int, width: int = 2, inner: int = 5) -> list[Range]:
    """The ranges ``templates.temperature`` lists for this forecast high."""
    low = forecast_high - width * (inner // 2) - 2
    edges = [low + width * i for i in range(inner + 1)]
    return (
        [(None, edges[0])]
        + [(a + 1, b) for a, b in itertools.pairwise(edges)]
        + [(edges[-1] + 1, None)]
    )


QUAKE_RANGES: list[Range] = [(None, 2), (3, 4), (5, 6), (7, None)]
