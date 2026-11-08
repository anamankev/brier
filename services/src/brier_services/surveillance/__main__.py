"""python -m brier_services.surveillance run --gateway URL --key-id N --key-file PATH --data DIR
[--core PATH] [--every SECONDS] (docs/surveillance.md)"""

from __future__ import annotations

import argparse
import subprocess
import sys
import time
from pathlib import Path

from brier_services.client import ApiError, RestClient, clock_ms
from brier_services.resolve import load_key
from brier_services.surveillance import service


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.surveillance")
    sub = parser.add_subparsers(dest="command", required=True)
    run = sub.add_parser("run", help="detect over the journal and open a case for each alert")
    run.add_argument("--gateway", required=True)
    run.add_argument("--key-id", type=int, required=True)
    run.add_argument("--key-file", type=Path, required=True)
    run.add_argument("--data", type=Path, required=True, help="the gateway's journal directory")
    run.add_argument("--core", type=Path, default=Path("target/release/brier-core"))
    run.add_argument("--every", type=float, default=0.0, help="repeat every so many seconds")
    args = parser.parse_args(argv)
    client = RestClient(args.gateway)
    client.use_key(args.key_id, load_key(args.key_file).sign, clock_ms)
    while True:
        try:
            service.run_once(client, service.export(args.core, args.data))
        except (ApiError, OSError, subprocess.CalledProcessError) as e:
            print(f'{{"event": "pass_failed", "error": "{type(e).__name__}"}}', flush=True)
            if not args.every:
                return 1
        if not args.every:
            return 0
        time.sleep(args.every)


if __name__ == "__main__":
    sys.exit(main())
