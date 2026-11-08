"""The registered market-maker study, M1-M3 (docs/notes/market-maker.md).

    python -m brier_services.sim.study [--out var/mm-study]

Runs 200 seeded markets in each of four conditions (noise only; noise with informed traders at
signal sd 2.0, 1.0, and 0.25), then tests:
- M1: mean maker P&L under noise only > 0 (one-sided t-test, 5%: t > 1.6526, the critical value
  of Student's t with 199 degrees of freedom);
- M2: mean P&L strictly decreases as the informed traders' signal improves;
- M3: |inventory| never exceeds the limit plus one quote size.
"""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
import time
from pathlib import Path
from typing import Any

from brier_services.marketmaker.quoting import Params
from brier_services.sim.market import STEPS, run

SEEDS = range(1, 201)
T_CRITICAL_199 = 1.6526  # one-sided 5%, 199 degrees of freedom
CONDITIONS: list[tuple[str, float | None]] = [
    ("noise", None),
    ("informed_sd_2.0", 2.0),
    ("informed_sd_1.0", 1.0),
    ("informed_sd_0.25", 0.25),
]


def summarize(pnls: list[int]) -> dict[str, float]:
    n = len(pnls)
    mean = statistics.fmean(pnls)
    sd = statistics.stdev(pnls)
    se = sd / math.sqrt(n)
    return {
        "n": n,
        "mean": mean,
        "sd": sd,
        "t": mean / se,
        "ci95_low": mean - 1.972 * se,
        "ci95_high": mean + 1.972 * se,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.sim.study")
    parser.add_argument("--out", type=Path, default=Path("var/mm-study"))
    args = parser.parse_args(argv)
    params = Params()
    started = time.time()
    results: dict[str, Any] = {
        "params": params.__dict__,
        "steps": STEPS,
        "seeds": [SEEDS.start, SEEDS.stop - 1],
        "conditions": {},
    }
    worst_inventory = 0
    for name, signal in CONDITIONS:
        runs = [run(seed, signal, params) for seed in SEEDS]
        pnls = [r.maker_pnl for r in runs]
        worst_inventory = max(worst_inventory, *(r.max_inventory for r in runs))
        results["conditions"][name] = {
            "pnl": summarize(pnls),
            "max_inventory": max(r.max_inventory for r in runs),
            "quoting_share": sum(r.steps_quoted for r in runs) / (len(runs) * STEPS),
            "average_spread_ticks": sum(r.quoted_spread_sum for r in runs)
            / max(1, sum(r.steps_quoted for r in runs)),
            "yes_outcomes": sum(r.outcome == "yes" for r in runs),
            "commands": sum(r.commands for r in runs),
        }
    means = [results["conditions"][n]["pnl"]["mean"] for n, _ in CONDITIONS[1:]]
    noise = results["conditions"]["noise"]["pnl"]
    results["m1"] = {
        "t": noise["t"],
        "critical": T_CRITICAL_199,
        "verdict": "passed" if noise["t"] > T_CRITICAL_199 else "refuted",
    }
    results["m2"] = {
        "means_poor_to_good": means,
        "verdict": "passed" if means[0] > means[1] > means[2] else "refuted",
    }
    results["m3"] = {
        "worst_inventory": worst_inventory,
        "bound": params.limit + params.size,
        "verdict": "passed" if worst_inventory <= params.limit + params.size else "refuted",
    }
    results["example_inventory_path_seed_1"] = list(
        run(1, None, params, record_path=True).inventory_path
    )
    results["seconds"] = round(time.time() - started, 1)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "results.json").write_text(json.dumps(results, indent=1) + "\n")
    for name, _ in CONDITIONS:
        c = results["conditions"][name]
        p = c["pnl"]
        print(
            f"{name}: mean P&L {p['mean']:.1f} (sd {p['sd']:.1f}, t {p['t']:.2f}), "
            f"max |inventory| {c['max_inventory']}, spread {c['average_spread_ticks']:.2f} ticks, "
            f"quoting {c['quoting_share']:.1%}"
        )
    for h in ("m1", "m2", "m3"):
        print(h.upper(), results[h]["verdict"])
    return 0 if all(results[h]["verdict"] == "passed" for h in ("m1", "m2", "m3")) else 1


if __name__ == "__main__":
    sys.exit(main())
