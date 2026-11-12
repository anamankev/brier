"""A local exchange with activity, for looking at the web app (not a test).

    BRIER_DEMO_DATABASE_URL=$(make -s db-url) uv run --project services python scripts/demo_stack.py

Starts the test-clock gateway on 127.0.0.1:18080 (its clock read from var/demo/clock), lists the
launch catalogue, and simulates three hours of trading in fast-forward: two market makers quote
around a random-walk fair value and several traders take liquidity, one simulated minute per
second, so the price history, books, and trades are real exchange output. The lister lists the
recurring catalogue (city highs, earthquakes, geomagnetic storms) from today's real forecasts,
the reference market maker quotes it with its backtested models, and noise traders trade it. Two accounts that
share a login address wash-trade, and one account spoofs; at the end the surveillance service
runs once and opens cases for the admin (`ada`). It leaves the gateway running and prints how
to stop it. Sign in as `demo` with the password below.
"""

from __future__ import annotations

import contextlib
import datetime as dt
import json
import os
import random
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from brier_services.client import ApiError, RestClient, totp_code
from brier_services.rules import load_toml
from brier_services.catalog.forecast import Forecasts
from brier_services.catalog.lister import run_once as list_catalogue
from brier_services.marketmaker.service import FairValues, MarketMaker
from brier_services.surveillance import service

ROOT = Path(__file__).resolve().parents[1]
DEMO = ROOT / "var" / "demo"
PORT = 18080
BASE = f"http://127.0.0.1:{PORT}"
PASSWORD = "a long enough password"  # noqa: S105 - local demo accounts
START = 1_794_234_600  # 2026-11-09T14:30:00Z, when the launch markets open
MINUTES = 120
FAIR = {101: 55, 102: 42, 103: 31, 104: 63, 105: 48}
PROXY_KEY = "demo proxy key, at least thirty-two characters"
WASH_MINUTES = {20, 45, 70, 95}  # market 104
SPOOF_MINUTES = {30, 60, 90}  # market 101


def main() -> int:
    url = os.environ.get("BRIER_DEMO_DATABASE_URL")
    if not url:
        print("set BRIER_DEMO_DATABASE_URL (make db-url)", file=sys.stderr)
        return 2
    target = ROOT / "target" / "test-clock"
    subprocess.run(  # noqa: S603 - fixed arguments
        ["cargo", "build", "--release", "--locked", "-p", "brier-gateway", "--features", "test-clock", "--target-dir", str(target)],
        cwd=ROOT, check=True,
    )  # fmt: skip
    subprocess.run(  # noqa: S603 - fixed arguments
        ["cargo", "build", "--release", "--locked", "-p", "brier-core", "--target-dir", str(target)],
        cwd=ROOT, check=True,
    )  # fmt: skip
    binary = str(target / "release" / "brier-gateway")
    DEMO.mkdir(parents=True, exist_ok=True)
    clock = DEMO / "clock"
    clock.write_text(str(START))
    env = os.environ | {
        "BRIER_DATABASE_URL": url,
        "BRIER_DATABASE_SCHEMA": f"demo_{int(time.time())}",
        "BRIER_DATA_DIR": str(DEMO / f"journal-{int(time.time())}"),
        "BRIER_LISTEN": f"127.0.0.1:{PORT}",
        "BRIER_TEST_CLOCK_FILE": str(clock),
        "BRIER_TOTP_KEY": "42" * 32,
        "BRIER_DISPUTE_WINDOW_SECONDS": "3600",
        "BRIER_TEST_AUTH_PER_MINUTE": "10000",
        "BRIER_PROXY_KEY": PROXY_KEY,
    }
    log = (DEMO / "gateway.log").open("w")
    gateway = subprocess.Popen([binary, "serve"], env=env, stderr=log, start_new_session=True)  # noqa: S603
    (DEMO / "pid").write_text(str(gateway.pid))
    for _ in range(150):
        try:
            RestClient(BASE, timeout=1).request("GET", "/v1/health")
            break
        except OSError:
            time.sleep(0.2)

    def user(name: str, role: str | None = None) -> RestClient:
        RestClient(BASE).request("POST", "/v1/register", {"username": name, "password": PASSWORD, "adult": True, "terms": "2026-11-12"})
        if role:
            subprocess.run([binary, "role", name, role, "demo"], env=env, check=True, capture_output=True)  # noqa: S603
        c = RestClient(BASE)
        c.login(name, PASSWORD)
        return c

    def from_address(name: str, address: str) -> RestClient:
        """Sign in as the web server would, forwarding the client's address."""
        RestClient(BASE).request("POST", "/v1/register", {"username": name, "password": PASSWORD, "adult": True, "terms": "2026-11-12"})
        req = urllib.request.Request(  # noqa: S310 - the local demo gateway
            f"{BASE}/v1/login",
            data=json.dumps({"username": name, "password": PASSWORD}).encode(),
            method="POST",
            headers={"content-type": "application/json", "x-brier-proxy-key": PROXY_KEY, "x-brier-client-address": address},
        )
        with urllib.request.urlopen(req, timeout=5) as res:  # noqa: S310
            c = RestClient(BASE)
            c.token = json.loads(res.read())["token"]
        return c

    admin = user("ada", "admin")
    secret, step = admin.enable_totp()
    admin.login("ada", PASSWORD, totp_code(secret, step + 1))
    makers = [user("maker_one"), user("maker_two")]  # each starts with 10,000 credits
    takers = [user(n) for n in ("ana", "ben", "chloe", "dev", "emil")]
    demo = user("demo")
    wes, wyn = from_address("wes", "198.51.100.23"), from_address("wyn", "198.51.100.23")  # one household
    sam = user("sam")
    for path in sorted((ROOT / "markets").glob("*.toml")):
        admin.request("POST", "/v1/admin/markets", {"rulebook": load_toml(path).raw, "reason": "demo"})
    time.sleep(1.5)  # the scheduler opens them

    # The recurring catalogue, listed as the lister service lists it, and quoted by the reference
    # market maker with its backtested models (docs/notes/catalog-models.md).
    lister = user("lister", "lister")
    listed = list_catalogue(lister, dt.datetime.fromtimestamp(START, dt.UTC), Forecasts().high, lambda *a, **k: None)
    print(f"listed {len(listed)} catalogue events", flush=True)
    time.sleep(1.5)
    fred_key = os.environ.get("FRED_API_KEY")
    clock_now = [float(START)]
    maker = MarketMaker(user("brier_maker"), FairValues(fred_key), now=lambda: clock_now[0], say=lambda *a, **k: None)
    catalogue: list[int] = []

    rng = random.Random(7)
    fair = dict(FAIR)
    quotes: dict[int, list[tuple[RestClient, int]]] = {m: [] for m in fair}
    for minute in range(MINUTES):
        for m in fair:
            fair[m] = min(94, max(6, fair[m] + rng.gauss(0, 1.3)))
            f = round(fair[m])
            for c, order in quotes[m]:
                try:
                    c.request("DELETE", f"/v1/orders/{order}")
                except ApiError:
                    pass  # already filled
            quotes[m] = []
            for level in range(1, 6):
                qty = rng.randint(4, 30) * level
                for c, side, price in ((makers[0], "buy", f - level), (makers[1], "sell", f + level)):
                    try:
                        r = c.request("POST", "/v1/orders", {"market": m, "side": side, "price": price, "qty": qty})
                    except ApiError:
                        continue  # out of play credits for now: quote less
                    if r.get("status") == "resting":
                        quotes[m].append((c, r["order"]))
            if rng.random() < 0.55:
                taker = rng.choice(takers)
                side = "buy" if rng.random() < 0.5 + (fair[m] - f) else "sell"
                price = f + 1 if side == "buy" else f - 1
                try:
                    taker.request("POST", "/v1/orders", {"market": m, "side": side, "price": price, "qty": rng.randint(3, 40), "tif": "ioc"})
                except ApiError:
                    pass
        if minute % 15 == 0:
            try:
                decided = maker.run_once()
            except ApiError as e:
                decided = {}
                print(f"\nmaker pass failed: {e}", flush=True)
            catalogue = [m for m, d in decided.items() if d["bid"] is not None and m >= 10_000_000]
        for _ in range(8 if catalogue else 0):  # noise traders on the catalogue
            m = rng.choice(catalogue)
            book = takers[0].request("GET", f"/v1/markets/{m}/book")
            side = "buy" if rng.random() < 0.5 else "sell"
            levels = book["asks"] if side == "buy" else book["bids"]
            if levels:
                with contextlib.suppress(ApiError):
                    rng.choice(takers).request(
                        "POST", "/v1/orders", {"market": m, "side": side, "price": levels[0][0], "qty": rng.randint(2, 30), "tif": "ioc"}
                    )
        f104 = round(fair[104])
        if minute in WASH_MINUTES:  # rest inside the spread, then the linked account takes it
            wes.request("POST", "/v1/orders", {"market": 104, "side": "sell", "price": f104, "qty": 60})
            wyn.request("POST", "/v1/orders", {"market": 104, "side": "buy", "price": f104, "qty": 60, "tif": "ioc"})
        if minute in SPOOF_MINUTES:  # a large bid behind the touch, a sale into the bid, then pull
            bids = sam.request("GET", "/v1/markets/101/book")["bids"]
            if bids:
                best = bids[0][0]
                big = sam.request("POST", "/v1/orders", {"market": 101, "side": "buy", "price": best - 1, "qty": 300})
                sam.request("POST", "/v1/orders", {"market": 101, "side": "sell", "price": best, "qty": 15, "tif": "ioc"})
                if big.get("status") == "resting":
                    sam.request("DELETE", f"/v1/orders/{big['order']}")
        clock.write_text(str(START + 60 * (minute + 1)))
        clock_now[0] = float(START + 60 * (minute + 1))
        time.sleep(1.05)
        print(f"\rminute {minute + 1}/{MINUTES}", end="", flush=True)

    for m in (101, 104):
        demo.request("POST", "/v1/orders", {"market": m, "side": "buy", "price": round(fair[m]) + 1, "qty": 25, "tif": "ioc"})
        demo.request("PUT", f"/v1/markets/{m}/forecast", {"probability": min(95, round(fair[m]) + 8)})
    demo.request("POST", "/v1/orders", {"market": 102, "side": "buy", "price": round(fair[102]) - 6, "qty": 40})
    watch = user("watch", "surveillance")
    commands = service.export(target / "release" / "brier-core", Path(env["BRIER_DATA_DIR"]))
    service.run_once(watch, commands)
    info = {"web": f"BRIER_GATEWAY_URL=http://127.0.0.1:18080 BRIER_WS_URL=ws://127.0.0.1:18080/v1/ws BRIER_PROXY_KEY='{PROXY_KEY}'", "admin": "ada", "admin_totp_secret": secret, "user": "demo", "password": PASSWORD}
    (DEMO / "info.json").write_text(json.dumps(info, indent=1))
    print(f"\nexchange running (pid {gateway.pid}); sign in as demo / {PASSWORD}; stop with: kill {gateway.pid}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
