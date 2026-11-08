"""python -m brier_services.catalog run --gateway URL --key-id N --key-file PATH [--every S]"""

from __future__ import annotations

import argparse
import datetime as dt
import sys
import time
from pathlib import Path

from brier_services.catalog.forecast import Forecasts
from brier_services.catalog.lister import run_once
from brier_services.client import RestClient, clock_ms
from brier_services.resolve import load_key


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.catalog")
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="list the next two days of every recurring series")
    run.add_argument("--gateway", required=True)
    run.add_argument("--key-id", type=int, required=True)
    run.add_argument("--key-file", type=Path, required=True)
    run.add_argument("--every", type=float, default=0.0, help="repeat every so many seconds")
    args = parser.parse_args(argv)
    client = RestClient(args.gateway)
    client.use_key(args.key_id, load_key(args.key_file).sign, clock_ms)
    forecasts = Forecasts()
    while True:
        run_once(client, dt.datetime.now(dt.UTC), forecasts.high)
        if not args.every:
            return 0
        time.sleep(args.every)


if __name__ == "__main__":
    sys.exit(main())
