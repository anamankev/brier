"""The G9 live run: the recurring catalogue on a real-time exchange for 72 hours.

    BRIER_LIVE_DATABASE_URL=$(make -s db-url) uv run --project services python scripts/live_catalogue.py run [--part-hours H]
    uv run --project services python scripts/live_catalogue.py summary

The run lasts 72 hours from its first start and can be split into parts: each part runs at most
``--part-hours``, stops the gateway cleanly, and the next part (with the database and
var/live-g9/ restored) continues where it left off. GitHub Actions runs it this way, in
6-hour jobs (.github/workflows/live-run.yml), so no laptop has to stay on.

A release gateway runs on 127.0.0.1:18100 by the real clock, in its own database schema and
journal under var/live-g9/. In one process:
- the lister lists the next two days of every series from the real forecasts (every 30 minutes);
- the reference market maker quotes every open market it has a model for (every minute);
- the resolver proposes outcomes from the real sources and verifies them (every 10 minutes);
- every 5 minutes a metrics line records open markets by category, quote coverage, and
  settlements with their evidence.

`summary` evaluates the exit criteria of docs/gates/G9-plan.md from the metrics so far.
Credentials are generated here and kept in var/live-g9/ (git-ignored); the FRED key comes from
the environment.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import secrets
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

from brier_services import resolve
from brier_services.catalog.forecast import Forecasts
from brier_services.catalog.lister import run_once as list_catalogue
from brier_services.client import ApiError, RestClient, totp_code
from brier_services.marketmaker.service import FairValues, MarketMaker
from brier_services.resolution.evidence import EvidenceStore
from brier_services.rules import load_toml

ROOT = Path(__file__).resolve().parents[1]
LIVE = ROOT / "var" / "live-g9"
PORT = 18100
BASE = f"http://127.0.0.1:{PORT}"
HOURS = 72
CATEGORIES_WANTED = 5


class Session:
    """A signed-in user who signs in again when the session expires."""

    def __init__(self, name: str, password: str, totp_secret: str | None = None) -> None:
        self.name, self.password, self.totp_secret = name, password, totp_secret
        self.client = RestClient(BASE)
        self.login()

    def login(self) -> None:
        code = None
        if self.totp_secret:
            time.sleep(31)  # each TOTP step is accepted once
            code = totp_code(self.totp_secret, int(time.time()) // 30)
        self.client.login(self.name, self.password, code)

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401 - JSON
        try:
            return self.client.request(method, path, body)
        except ApiError as e:
            if e.status != 401:
                raise
            self.login()
            return self.client.request(method, path, body)


class SessionGateway(resolve.RestGateway):
    """The resolver's gateway calls over a session that renews itself."""

    def __init__(self, session: Session) -> None:
        super().__init__(session.client)
        self.session = session

    def markets(self) -> list[dict[str, Any]]:
        return list(self.session.request("GET", "/v1/markets")["markets"])

    def rules(self, market: int) -> dict[str, Any]:
        return dict(self.session.request("GET", f"/v1/markets/{market}/rules"))

    def resolutions(self, market: int) -> list[dict[str, Any]]:
        return list(self.session.request("GET", f"/v1/markets/{market}/resolution")["resolutions"])

    def propose(self, market: int, body: Any) -> dict[str, Any]:  # noqa: ANN401
        return dict(self.session.request("POST", f"/v1/markets/{market}/resolution", dict(body)))

    def verify(self, resolution: int, body: Any) -> dict[str, Any]:  # noqa: ANN401
        return dict(self.session.request("POST", f"/v1/resolutions/{resolution}/verify", dict(body)))


def log(kind: str, **fields: Any) -> None:  # noqa: ANN401 - JSON fields
    line = json.dumps({"at": dt.datetime.now(dt.UTC).isoformat(timespec="seconds"), "event": kind, **fields}, sort_keys=True)
    with (LIVE / "events.jsonl").open("a") as f:
        f.write(line + "\n")


def start_gateway(db: str) -> tuple[subprocess.Popen[bytes], dict[str, str]]:
    subprocess.run(["cargo", "build", "--release", "--locked", "-p", "brier-gateway"], cwd=ROOT, check=True)  # noqa: S603, S607
    binary = str(ROOT / "target" / "release" / "brier-gateway")
    LIVE.mkdir(parents=True, exist_ok=True)
    keyfile = LIVE / "totp.key"
    if not keyfile.exists():
        keyfile.write_text(secrets.token_hex(32))
        keyfile.chmod(0o600)
    env = os.environ | {
        "BRIER_DATABASE_URL": db,
        "BRIER_DATABASE_SCHEMA": "live_g9",
        "BRIER_DATA_DIR": str(LIVE / "journal"),
        "BRIER_LISTEN": f"127.0.0.1:{PORT}",
        "BRIER_TOTP_KEY": keyfile.read_text().strip(),
    }
    gateway = subprocess.Popen([binary, "serve"], env=env, stderr=(LIVE / "gateway.log").open("a"), start_new_session=True)  # noqa: S603
    (LIVE / "gateway.pid").write_text(str(gateway.pid))
    for _ in range(150):
        try:
            RestClient(BASE, timeout=1).request("GET", "/v1/health")
            break
        except OSError:
            time.sleep(0.2)
    return gateway, env | {"binary": binary}


def users(env: dict[str, str]) -> dict[str, Session]:
    path = LIVE / "users.json"
    if path.exists():
        saved = json.loads(path.read_text())
    else:
        saved = {"password": secrets.token_urlsafe(18), "roles": {"lister": "lister", "rex": "resolver", "maker": None, "ada": "admin"}}
        for name, role in saved["roles"].items():
            RestClient(BASE).request("POST", "/v1/register", {"username": name, "password": saved["password"], "adult": True, "terms": "2026-11-08"})
            if role:
                subprocess.run([env["binary"], "role", name, role, "G9 live run"], env=env, check=True, capture_output=True)  # noqa: S603
        admin = RestClient(BASE)
        admin.login("ada", saved["password"])
        saved["ada_totp"], _ = admin.enable_totp()
        path.write_text(json.dumps(saved))
        path.chmod(0o600)
    pw = saved["password"]
    return {
        "lister": Session("lister", pw),
        "rex": Session("rex", pw),
        "maker": Session("maker", pw),
        "ada": Session("ada", pw, saved["ada_totp"]),
    }


def metrics(s: Session) -> dict[str, Any]:
    events = s.request("GET", "/v1/events?status=all&limit=200")["events"]
    markets = [(e, m) for e in events for m in e["markets"]]
    open_markets = [(e, m) for e, m in markets if m["state"] == "open"]
    by_category: dict[str, int] = {}
    for e, _ in open_markets:
        by_category[e["category"] or "none"] = by_category.get(e["category"] or "none", 0) + 1
    quoted = sum(1 for _, m in open_markets if m["best_bid"] is not None and m["best_ask"] is not None)
    settled = [m["market"] for _, m in markets if m["state"] in {"resolved", "voided"}]
    return {"open": len(open_markets), "by_category": by_category, "quoted": quoted, "settled": settled}


def first_start() -> float | None:
    path = LIVE / "events.jsonl"
    if not path.exists():
        return None
    for line in path.read_text().splitlines():
        x = json.loads(line)
        if x["event"] == "start":
            return dt.datetime.fromisoformat(x["at"]).timestamp()
    return None


def run(hours: float, part_hours: float) -> int:
    db = os.environ.get("BRIER_LIVE_DATABASE_URL")
    if not db:
        print("set BRIER_LIVE_DATABASE_URL (make db-url)", file=sys.stderr)
        return 2
    if (LIVE / "complete").exists():
        print("the live run is already complete")
        return 0
    resumed = first_start()
    gateway, env = start_gateway(db)
    who = users(env)
    if resumed is None:
        for path in sorted((ROOT / "markets").glob("*.toml")):  # economics and rates, by admin
            try:
                who["ada"].request("POST", "/v1/admin/markets", {"rulebook": load_toml(path).raw, "reason": "G9 live run"})
            except ApiError as e:
                log("launch_listing", market=path.stem, error=str(e))
    maker = MarketMaker(who["maker"], FairValues(os.environ.get("FRED_API_KEY")), say=lambda *a, **k: None)
    forecasts = Forecasts()
    store = EvidenceStore(LIVE / "evidence")
    verified: set[int] = set()
    started = resumed if resumed is not None else time.time()
    part_end = time.time() + part_hours * 3600
    end = min(started + hours * 3600, part_end)
    last = {"list": 0.0, "resolve": 0.0, "metrics": 0.0}
    log("resume" if resumed is not None else "start", hours=hours, gateway_pid=gateway.pid)
    while time.time() < end:
        now = time.time()
        try:
            if now - last["list"] >= 1800:
                listed = list_catalogue(who["lister"], dt.datetime.now(dt.UTC), forecasts.high, lambda *a, **k: None)
                log("listed", slugs=listed)
                last["list"] = now
            maker.run_once()
            if now - last["resolve"] >= 600:
                resolve.run_once(SessionGateway(who["rex"]), store, int(now), os.environ.get("FRED_API_KEY"),
                                 say=lambda kind, **f: log(f"resolver_{kind}", **f), verified=verified)  # fmt: skip
                last["resolve"] = now
            if now - last["metrics"] >= 300:
                log("metrics", **metrics(who["lister"]))
                last["metrics"] = now
        except (ApiError, OSError) as e:
            log("pass_failed", error=f"{type(e).__name__}: {e}")
        time.sleep(max(0.0, 60 - (time.time() - now)))
    gateway.terminate()  # the gateway stops cleanly on SIGTERM; the journal is already durable
    gateway.wait(timeout=60)
    if time.time() >= started + hours * 3600:
        log("end")
        (LIVE / "complete").write_text("72-hour live run complete\n")
    else:
        log("part_end")
    print(json.dumps(summary(), indent=1))
    return 0


def summary() -> dict[str, Any]:
    lines = [json.loads(x) for x in (LIVE / "events.jsonl").read_text().splitlines()]
    samples = [x for x in lines if x["event"] == "metrics"]
    listed_at: dict[str, str] = {}
    for x in lines:
        if x["event"] == "listed":
            for slug in x["slugs"]:
                listed_at.setdefault(slug, x["at"])
    first_settled: dict[int, str] = {}
    for x in samples:
        for m in x["settled"]:
            first_settled.setdefault(m, x["at"])
    start = next((x["at"] for x in lines if x["event"] == "start"), None)
    hours = (dt.datetime.fromisoformat(samples[-1]["at"]) - dt.datetime.fromisoformat(start)).total_seconds() / 3600 if samples and start else 0
    stamps = [dt.datetime.fromisoformat(x["at"]) for x in samples]
    gaps = [round((b - a).total_seconds() / 60, 1) for a, b in zip(stamps, stamps[1:], strict=False) if (b - a).total_seconds() > 600]
    coverage = [x["quoted"] / x["open"] for x in samples if x["open"]]
    return {
        "hours_observed": round(hours, 2),
        "gaps_over_10_minutes": gaps,
        "samples": len(samples),
        "min_open_markets": min((x["open"] for x in samples), default=0),
        "categories_open_throughout": sorted(set.intersection(*(set(x["by_category"]) for x in samples))) if samples else [],
        "categories_wanted": CATEGORIES_WANTED,
        "settled_markets": len(first_settled),
        "listed_events": len(listed_at),
        "quote_coverage_min": min(coverage, default=0),
        "share_of_samples_quoting_90pct": sum(c >= 0.9 for c in coverage) / len(coverage) if coverage else 0,
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="scripts/live_catalogue.py")
    sub = parser.add_subparsers(dest="command", required=True)
    r = sub.add_parser("run")
    r.add_argument("--hours", type=float, default=HOURS, help="the whole run, from its first start")
    r.add_argument("--part-hours", type=float, default=HOURS, help="this part's longest run")
    sub.add_parser("summary")
    args = parser.parse_args(argv)
    if args.command == "summary":
        print(json.dumps(summary(), indent=1))
        return 0
    return run(args.hours, args.part_hours)


if __name__ == "__main__":
    sys.exit(main())
