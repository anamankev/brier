"""Load the public site the way browsing visitors do, at rising concurrency, and report
throughput, latency percentiles and errors (docs/notes/load-test.md).

    python3 scripts/load_test.py https://playbrier.duckdns.org [--seconds 30] [--levels 5,20,50]

Each simulated visitor loops over a mix of what people open: the home page, a category page,
an event page, the events API through the web app, and the gateway's public API. Standard
library only; one machine, so every request comes from one address.
"""

from __future__ import annotations

import argparse
import json
import statistics
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor


def get(url: str) -> tuple[int, float]:
    start = time.perf_counter()
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "brier-load-test/1"})
        with urllib.request.urlopen(req, timeout=30) as res:  # noqa: S310 - the operator's own site
            res.read()
            status = res.status
    except urllib.error.HTTPError as e:
        status = e.code
    except (urllib.error.URLError, TimeoutError, OSError):
        status = 0
    return status, time.perf_counter() - start


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("site")
    p.add_argument("--seconds", type=float, default=30)
    p.add_argument("--levels", default="5,20,50")
    a = p.parse_args()
    events = json.loads(urllib.request.urlopen(f"{a.site}/v1/events?limit=5").read())["events"]  # noqa: S310
    slug = events[0]["slug"]
    paths = ["/", "/c/weather", f"/e/{slug}", "/api/v1/events?limit=60", "/v1/events?limit=60", "/v1/health"]
    print(f"{'users':>5} {'req/s':>7} {'p50 ms':>7} {'p95 ms':>7} {'p99 ms':>7} {'errors':>7}  by path p95 ms")
    for users in [int(x) for x in a.levels.split(",")]:
        results: list[tuple[str, int, float]] = []
        lock = threading.Lock()
        stop = time.monotonic() + a.seconds

        def visitor(i: int, stop: float = stop, results: list[tuple[str, int, float]] = results) -> None:
            k = i
            while time.monotonic() < stop:
                path = paths[k % len(paths)]
                status, secs = get(a.site + path)
                with lock:
                    results.append((path, status, secs))
                k += 1

        t0 = time.monotonic()
        with ThreadPoolExecutor(users) as pool:
            list(pool.map(visitor, range(users)))
        elapsed = time.monotonic() - t0
        ok = sorted(s for _, st, s in results if st == 200)
        errors = sum(1 for _, st, _ in results if st != 200)
        q = statistics.quantiles(ok, n=100) if len(ok) > 1 else [0] * 99
        per = {}
        for path in paths:
            xs = sorted(s for pth, st, s in results if pth == path and st == 200)
            per[path.split("?")[0]] = round(statistics.quantiles(xs, n=20)[18] * 1000) if len(xs) > 1 else None
        print(f"{users:>5} {len(results) / elapsed:>7.1f} {q[49] * 1000:>7.0f} {q[94] * 1000:>7.0f} {q[98] * 1000:>7.0f} {errors:>7}  {per}")


if __name__ == "__main__":
    main()
