"""The registered surveillance evaluation, V1-V4 (docs/notes/surveillance.md).

    python -m brier_services.surveillance.study [--out var/surveillance-study]

1. Simulate seeds 1-300 (``brier_services.sim.surveillance``) and score every account in every
   market with every detector.
2. For each typology, choose the threshold that maximizes F1 on the tuning seeds (1-100),
   ties to the higher threshold.
3. At that threshold, report precision, recall, and F1 on the test seeds (101-300), with 95%
   percentile intervals from 1,000 bootstrap resamples of test seeds (resampling seed
   20261108), and the false positives by kind of account.
"""

from __future__ import annotations

import argparse
import json
import random
import sys
import time
from collections import Counter
from collections.abc import Iterable
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from brier_services.sim.surveillance import run
from brier_services.surveillance.detectors import DETECTORS, scores
from brier_services.surveillance.tape import build

TUNING = range(1, 101)
TEST = range(101, 301)
RESAMPLES = 1000
RESAMPLE_SEED = 20261108
TARGETS = {
    "wash": (0.80, 0.80),
    "spoof": (0.70, 0.70),
    "close": (0.70, 0.70),
    "insider": (0.60, 0.60),
}
HYPOTHESIS = {"wash": "V1", "spoof": "V2", "close": "V3", "insider": "V4"}


@dataclass(frozen=True)
class Row:
    seed: int
    typology: str
    account: int
    score: float
    kind: str


@dataclass
class Data:
    rows: list[Row]
    positives: set[tuple[int, str, int]]  # (seed, typology, account)
    kind: dict[int, str]


def collect(seeds: Iterable[int]) -> Data:
    rows: list[Row] = []
    positives: set[tuple[int, str, int]] = set()
    kind: dict[int, str] = {}
    for seed in seeds:
        s = run(seed)
        kind |= s.kind
        positives |= {(seed, t, a) for t, a in s.truth}
        rows += [
            Row(seed, a.typology, a.account, a.score, s.kind[a.account])
            for a in scores(build(s.records), s.links)
        ]
    return Data(rows, positives, kind)


def per_seed(
    data: Data, typology: str, threshold: float, seeds: range
) -> dict[int, tuple[int, int, int]]:
    """(true positives, false positives, false negatives) for each seed."""
    alerts: dict[int, set[int]] = {seed: set() for seed in seeds}
    truth: dict[int, set[int]] = {seed: set() for seed in seeds}
    for r in data.rows:
        if r.typology == typology and r.score >= threshold and r.seed in alerts:
            alerts[r.seed].add(r.account)
    for s, t, a in data.positives:
        if t == typology and s in truth:
            truth[s].add(a)
    return {
        seed: (
            len(alerts[seed] & truth[seed]),
            len(alerts[seed] - truth[seed]),
            len(truth[seed] - alerts[seed]),
        )
        for seed in seeds
    }


def total(counts: Iterable[tuple[int, int, int]]) -> tuple[int, int, int]:
    tp = fp = fn = 0
    for a, b, c in counts:
        tp, fp, fn = tp + a, fp + b, fn + c
    return tp, fp, fn


def rates(tp: int, fp: int, fn: int) -> dict[str, float | None]:
    return {
        "precision": tp / (tp + fp) if tp + fp else None,
        "recall": tp / (tp + fn) if tp + fn else None,
        "f1": 2 * tp / (2 * tp + fp + fn) if tp + fp + fn else None,
    }


def tune(data: Data, typology: str) -> float:
    """The threshold with the highest F1 on the tuning seeds; ties to the higher one."""
    candidates = sorted({r.score for r in data.rows if r.typology == typology and r.seed in TUNING})
    best = (-1.0, 0.0)
    for threshold in candidates:
        f1 = rates(*total(per_seed(data, typology, threshold, TUNING).values()))["f1"] or 0.0
        best = max(best, (f1, threshold))
    return best[1] if candidates else float("inf")


def interval(values: list[float]) -> list[float] | None:
    if not values:
        return None
    values = sorted(values)
    return [values[int(0.025 * (len(values) - 1))], values[int(0.975 * (len(values) - 1))]]


def evaluate(data: Data) -> dict[str, Any]:
    out: dict[str, Any] = {}
    rng = random.Random(RESAMPLE_SEED)  # noqa: S311 - resampling, not cryptography
    draws = [[rng.choice(TEST) for _ in TEST] for _ in range(RESAMPLES)]
    for typology in DETECTORS:
        threshold = tune(data, typology)
        tuning = total(per_seed(data, typology, threshold, TUNING).values())
        seeds = per_seed(data, typology, threshold, TEST)
        test = total(seeds.values())
        boot: dict[str, list[float]] = {"precision": [], "recall": [], "f1": []}
        undefined = Counter[str]()
        for draw in draws:
            for name, value in rates(*total(seeds[s] for s in draw)).items():
                if value is None:
                    undefined[name] += 1
                else:
                    boot[name].append(value)
        point = rates(*test)
        p_target, r_target = TARGETS[typology]
        ci = {name: interval(v) for name, v in boot.items()}
        false_by_kind = Counter(
            data.kind[r.account]
            for r in data.rows
            if r.typology == typology
            and r.seed in TEST
            and r.score >= threshold
            and (r.seed, typology, r.account) not in data.positives
        )
        supported = (point["precision"] or 0) >= p_target and (point["recall"] or 0) >= r_target
        inside = []
        for name, target in (("precision", p_target), ("recall", r_target)):
            bounds = ci[name]
            if bounds is not None and bounds[0] <= target <= bounds[1]:
                inside.append(name)
        out[typology] = {
            "hypothesis": HYPOTHESIS[typology],
            "threshold": threshold,
            "tuning": {"tp": tuning[0], "fp": tuning[1], "fn": tuning[2], **rates(*tuning)},
            "test": {"tp": test[0], "fp": test[1], "fn": test[2], **point},
            "ci95": ci,
            "undefined_resamples": dict(undefined),
            "false_positives_by_kind": dict(false_by_kind),
            "targets": {"precision": p_target, "recall": r_target},
            "supported": supported,
            "target_inside_interval": inside,
        }
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.surveillance.study")
    parser.add_argument("--out", type=Path, default=Path("var/surveillance-study"))
    args = parser.parse_args(argv)
    started = time.time()
    data = collect([*TUNING, *TEST])
    results: dict[str, Any] = {
        "tuning_seeds": [TUNING.start, TUNING.stop - 1],
        "test_seeds": [TEST.start, TEST.stop - 1],
        "resamples": RESAMPLES,
        "resample_seed": RESAMPLE_SEED,
        "typologies": evaluate(data),
        "seconds": round(time.time() - started, 1),
    }
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "results.json").write_text(json.dumps(results, indent=1, sort_keys=True) + "\n")
    for typology, r in results["typologies"].items():
        t = r["test"]
        print(
            f"{r['hypothesis']} {typology}: threshold {r['threshold']:.4g}, precision "
            f"{t['precision']}, recall {t['recall']} (tp {t['tp']}, fp {t['fp']}, fn {t['fn']}), "
            f"ci {r['ci95']}, {'SUPPORTED' if r['supported'] else 'NOT SUPPORTED'}"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
