"""python -m brier_services.marketmaker run --gateway URL --key-id N --key-file PATH"""

from __future__ import annotations

import argparse
import os
import sys
import time
from pathlib import Path

from brier_services.client import ApiError, RestClient, clock_ms
from brier_services.marketmaker.service import FairValues, MarketMaker
from brier_services.resolve import load_key


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.marketmaker")
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="quote every market with a fair-value model")
    run.add_argument("--gateway", required=True)
    run.add_argument("--key-id", type=int, required=True)
    run.add_argument(
        "--key-file",
        type=Path,
        required=True,
        help="made by python -m brier_services.resolve keygen",
    )
    run.add_argument("--interval", type=float, default=5.0, help="seconds between passes")
    run.add_argument("--metrics", type=Path, default=Path("var/marketmaker/metrics.jsonl"))
    args = parser.parse_args(argv)
    key = load_key(args.key_file)
    client = RestClient(args.gateway)
    client.use_key(args.key_id, key.sign, clock_ms)
    args.metrics.parent.mkdir(parents=True, exist_ok=True)
    maker = MarketMaker(client, FairValues(os.environ.get("FRED_API_KEY")), metrics=args.metrics)
    while True:
        try:
            maker.run_once()
        except (ApiError, OSError) as e:
            maker.say("pass_failed", error=f"{type(e).__name__}: {e}")
        time.sleep(args.interval)


if __name__ == "__main__":
    sys.exit(main())
