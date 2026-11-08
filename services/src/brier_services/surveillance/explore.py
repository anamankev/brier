"""Exploratory follow-ups to the registered evaluation (docs/notes/surveillance.md §5).

    python -m brier_services.surveillance.explore [--out var/surveillance-study]

Chosen after the registered results were seen, so they test nothing; they are measured on
fresh seeds 301-500, which no earlier run used. Each variant changes one detector rule,
is tuned on seeds 1-100 as registered, and is reported on the fresh seeds.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from brier_services.surveillance import detectors, study

FRESH = range(301, 501)
VARIANTS: dict[str, tuple[str, dict[str, float]]] = {
    # name: (typology, the detector parameters it changes)
    "registered spoof": ("spoof", {}),
    "spoof, unpaired orders weigh 0": ("spoof", {"SPOOF_UNPAIRED_WEIGHT": 0.0}),
    "spoof, only orders paired with an opposite trade the account initiated": (
        "spoof",
        {"SPOOF_UNPAIRED_WEIGHT": 0.0, "SPOOF_AGAINST_AS_TAKER_ONLY": 1.0},
    ),
    "registered insider": ("insider", {}),
    "insider, no weight for a new account": ("insider", {"INSIDER_NEW_WEIGHT": 0.0}),
}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.surveillance.explore")
    parser.add_argument("--out", type=Path, default=Path("var/surveillance-study"))
    args = parser.parse_args(argv)
    out: dict[str, Any] = {"fresh_seeds": [FRESH.start, FRESH.stop - 1], "variants": {}}
    for name, (typology, changed) in VARIANTS.items():
        with detectors.configured(changed):
            data = study.collect([*study.TUNING, *FRESH])
        threshold = study.tune(data, typology)
        counts = study.total(study.per_seed(data, typology, threshold, FRESH).values())
        out["variants"][name] = {
            "typology": typology,
            "parameters": changed,
            "threshold": threshold,
            "fresh": {"tp": counts[0], "fp": counts[1], "fn": counts[2], **study.rates(*counts)},
        }
        print(name, out["variants"][name])
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "explore.json").write_text(json.dumps(out, indent=1, sort_keys=True) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
