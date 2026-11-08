"""M4 of the registered market-maker study (docs/notes/market-maker.md): the real service on a
real exchange, through the public API, on the launch markets.

    BRIER_STUDY_DATABASE_URL=$(make -s db-url) FRED_API_KEY=... \\
        uv run python -m brier_services.marketmaker.live_study

Starts the test-clock gateway (as the G5 exit test does), lists the launch catalogue, and runs
the market maker under its own account and signed key for seven simulated days, one pass per
simulated minute. Fair-value inputs come from the real sources (BLS, FRED, NOAA). Each minute it
samples the exchange's own book: a market counts as quoted when its best bid and best ask are
both the maker's orders. Uptime is quoted minutes over the minutes the maker should quote (open,
more than 15 minutes from close, fair value fresh). Results: var/mm-live/results.json.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from brier_services.client import Ed25519Key, RestClient, clock_ms, totp_code
from brier_services.marketmaker.service import CLOSE_BUFFER, FairValues, MarketMaker
from brier_services.rules import instant, load_toml

ROOT = Path(__file__).resolve().parents[4]
OUT = ROOT / "var" / "mm-live"
PORT = 18090
BASE = f"http://127.0.0.1:{PORT}"
PASSWORD = "a long enough password"  # noqa: S105 - local study accounts
START = 1_794_234_600  # 2026-11-09T14:30:00Z, when the launch markets open
MINUTES = 7 * 1440
MARKETS = (101, 102, 104, 105)


def main() -> int:
    url = os.environ.get("BRIER_STUDY_DATABASE_URL")
    if not url or not os.environ.get("FRED_API_KEY"):
        print("set BRIER_STUDY_DATABASE_URL (make db-url) and FRED_API_KEY", file=sys.stderr)
        return 2
    target = ROOT / "target" / "test-clock"
    subprocess.run(  # noqa: S603 - fixed arguments
        [  # noqa: S607 - cargo from the developer's PATH
            "cargo",
            "build",
            "--release",
            "--locked",
            "-p",
            "brier-gateway",
            "--features",
            "test-clock",
            "--target-dir",
            str(target),
        ],
        cwd=ROOT,
        check=True,
    )
    binary = str(target / "release" / "brier-gateway")
    OUT.mkdir(parents=True, exist_ok=True)
    clock = OUT / "clock"
    clock.write_text(str(START))
    env = os.environ | {
        "BRIER_DATABASE_URL": url,
        "BRIER_DATABASE_SCHEMA": f"mm_live_{int(time.time())}",
        "BRIER_DATA_DIR": str(OUT / f"journal-{int(time.time())}"),
        "BRIER_LISTEN": f"127.0.0.1:{PORT}",
        "BRIER_TEST_CLOCK_FILE": str(clock),
        "BRIER_TOTP_KEY": "42" * 32,
        "BRIER_TEST_AUTH_PER_MINUTE": "10000",
    }
    gateway = subprocess.Popen([binary, "serve"], env=env, stderr=(OUT / "gateway.log").open("w"))  # noqa: S603
    try:
        return study(binary, env, clock)
    finally:
        gateway.terminate()
        gateway.wait(timeout=30)


def study(binary: str, env: dict[str, str], clock: Path) -> int:
    for _ in range(150):
        try:
            RestClient(BASE, timeout=1).request("GET", "/v1/health")
            break
        except OSError:
            time.sleep(0.2)

    def user(name: str, role: str | None = None) -> RestClient:
        RestClient(BASE).request("POST", "/v1/register", {"username": name, "password": PASSWORD})
        if role:
            subprocess.run(  # noqa: S603 - fixed arguments
                [binary, "role", name, role, "market-maker study"],
                env=env,
                check=True,
                capture_output=True,
            )
        c = RestClient(BASE)
        c.login(name, PASSWORD)
        return c

    admin = user("study_admin", "admin")
    secret, step = admin.enable_totp()
    admin.login("study_admin", PASSWORD, totp_code(secret, step + 1))
    for path in sorted((ROOT / "markets").glob("*.toml")):
        admin.request(
            "POST",
            "/v1/admin/markets",
            {"rulebook": load_toml(path).raw, "reason": "market-maker study"},
        )
    maker_session = user("reference_maker")
    key = Ed25519Key()
    key_id = maker_session.request("POST", "/v1/keys", {"public_key": key.public_key_b64()})[
        "key_id"
    ]
    api = RestClient(BASE)
    api.use_key(key_id, key.sign, clock_ms)
    for _ in range(50):  # the scheduler opens the markets within a second
        if all(m["state"] == "open" for m in api.request("GET", "/v1/markets")["markets"]):
            break
        time.sleep(0.2)

    log = (OUT / "maker.log").open("w")

    def say(event: str, **fields: Any) -> None:  # noqa: ANN401 - JSON fields
        log.write(json.dumps({"event": event, **fields}) + "\n")

    maker = MarketMaker(
        api,
        FairValues(os.environ["FRED_API_KEY"]),
        now=lambda: float(clock.read_text()),
        say=say,
    )
    closes = {
        m: instant(load_toml(next((ROOT / "markets").glob(f"{m}-*.toml"))).raw["schedule"]["close"])
        or 0
        for m in MARKETS
    }
    should = dict.fromkeys(MARKETS, 0)
    quoted = dict.fromkeys(MARKETS, 0)
    fairs: dict[int, list[float]] = {m: [] for m in MARKETS}
    started = time.time()
    for minute in range(MINUTES):
        now = START + 60 * minute
        clock.write_text(str(now))
        decided = maker.run_once()
        states = {m["market"]: m["state"] for m in api.request("GET", "/v1/markets")["markets"]}
        mine = api.request("GET", "/v1/portfolio")["open_orders"]
        for m in MARKETS:
            fresh = decided[m]["fair"] is not None
            if states[m] == "open" and closes[m] - now >= CLOSE_BUFFER and fresh:
                should[m] += 1
                book: dict[str, Any] = api.request("GET", f"/v1/markets/{m}/book")
                best_bid = book["bids"][0][0] if book["bids"] else None
                best_ask = book["asks"][0][0] if book["asks"] else None
                own = {(o["side"], o["price"]) for o in mine if o["market"] == m}
                if ("buy", best_bid) in own and ("sell", best_ask) in own:
                    quoted[m] += 1
            if decided[m]["fair"] is not None:
                fairs[m].append(decided[m]["fair"])
        if minute % 1440 == 0:
            print(f"day {minute // 1440 + 1}/7", flush=True)
    uptime = {m: (quoted[m] / should[m] if should[m] else None) for m in MARKETS}
    results = {
        "minutes": MINUTES,
        "should_quote_minutes": should,
        "quoted_minutes": quoted,
        "uptime": uptime,
        "fair_value_range": {m: [min(f), max(f)] if f else None for m, f in fairs.items()},
        "fomc_market_decision": "no model, no quotes",
        "seconds": round(time.time() - started, 1),
        "verdict": "passed"
        if all(u is not None and u >= 0.95 for u in uptime.values())
        else "refuted",
    }
    (OUT / "results.json").write_text(json.dumps(results, indent=1) + "\n")
    print(json.dumps(results, indent=1))
    return 0 if results["verdict"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
