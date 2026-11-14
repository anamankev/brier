"""The exchange's operator: the long-running process that keeps a public Brier deployment
alive next to its gateway (docs/runbooks/deploy.md).

    python -m brier_services.exchange_operator --gateway http://127.0.0.1:8080 \\
        --gateway-binary /opt/brier/bin/brier-gateway --core /opt/brier/bin/brier-core \\
        --data /var/lib/brier/journal --state /var/lib/brier/operator

Every minute the reference market maker quotes; every 30 minutes the lister lists the recurring
catalogue two days ahead; every 10 minutes the resolver proposes and verifies outcomes; every hour
surveillance runs over the journal. On first start it creates the service accounts (lister,
resolver, maker, surveillance), gives them their roles with the gateway's CLI, and keeps their
generated password in ``--state`` (mode 600). Nothing here grants credits or trades for users.

Each pass logs one JSON line to stdout (systemd's journal). A failed pass is logged and retried
on the next tick; the process exits only on a signal.
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
from brier_services.catalog import economy, sports_catalog
from brier_services.catalog.forecast import Forecasts
from brier_services.catalog.lister import run_once as list_catalogue
from brier_services.client import ApiError, RestClient
from brier_services.marketmaker.service import FairValues, MarketMaker
from brier_services.resolution.evidence import EvidenceStore
from brier_services.surveillance import service as surveillance

TERMS = "2026-11-12"
ROLES = {"lister": "lister", "resolver": "resolver", "maker": None, "watch": "surveillance"}
EVERY = {"list": 1800.0, "sports": 3 * 3600.0, "resolve": 600.0, "surveil": 3600.0}
TICK = 60.0
TASKS = ("list", "maker", "resolve", "surveil")


def log(kind: str, **fields: Any) -> None:  # noqa: ANN401 - JSON fields
    stamp = dt.datetime.now(dt.UTC).isoformat(timespec="seconds")
    print(json.dumps({"at": stamp, "event": kind, **fields}, sort_keys=True), flush=True)


class Session:
    """A service account that signs in again when its session expires."""

    def __init__(self, base: str, name: str, password: str) -> None:
        self.name, self.password = name, password
        self.client = RestClient(base)
        self.client.login(name, password)

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401 - JSON
        try:
            return self.client.request(method, path, body)
        except ApiError as e:
            if e.status != 401:
                raise
            self.client.login(self.name, self.password)
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
        return dict(
            self.session.request("POST", f"/v1/resolutions/{resolution}/verify", dict(body))
        )


def accounts(base: str, binary: Path | None, state: Path) -> dict[str, Session]:
    """The service accounts, created and given their roles on first start (which needs the
    gateway binary; later starts read the saved password)."""
    path = state / "accounts.json"
    if path.exists():
        saved = json.loads(path.read_text())
    elif binary is None:
        msg = f"{path} does not exist; the first start needs --gateway-binary"
        raise SystemExit(msg)
    else:
        saved = {"password": secrets.token_urlsafe(24)}
        for name, role in ROLES.items():
            body = {"username": name, "password": saved["password"], "adult": True, "terms": TERMS}
            try:
                RestClient(base).request("POST", "/v1/register", body)
            except ApiError as e:
                if e.code != "username_taken":
                    raise
            if role:
                subprocess.run(  # noqa: S603 - the operator's own gateway binary
                    [str(binary), "role", name, role, "operator service account"],
                    check=True,
                    capture_output=True,
                )
        state.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(saved))
        path.chmod(0o600)
    sessions = {name: Session(base, name, saved["password"]) for name in ROLES}
    # Accounts registered under an earlier version of the terms accept the current one, or the
    # gateway refuses their orders (TERMS ships with the gateway it matches).
    for session in sessions.values():
        session.request("POST", "/v1/terms", {"terms": TERMS})
    return sessions


# Maker lines worth keeping in the journal; routine quotes and cancels are not.
MAKER_LOGGED = frozenset({"quote_refused", "allowance", "allowance_failed"})


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="python -m brier_services.exchange_operator")
    p.add_argument("--gateway", default="http://127.0.0.1:8080")
    p.add_argument("--gateway-binary", type=Path, help="needed only to create the accounts")
    p.add_argument("--core", type=Path, help="brier-core, for surveillance's journal export")
    p.add_argument("--data", type=Path, help="the gateway's journal directory (surveillance)")
    p.add_argument("--state", type=Path, required=True)
    p.add_argument(
        "--tasks",
        default=",".join(TASKS),
        help=f"comma-separated subset of {','.join(TASKS)}: surveillance reads the journal, so "
        "it runs beside the gateway; the rest can run on another server",
    )
    args = p.parse_args(argv)
    tasks = set(args.tasks.split(","))
    if not tasks or not tasks <= set(TASKS):
        p.error(f"--tasks must be a subset of {','.join(TASKS)}")
    if "surveil" in tasks and (args.core is None or args.data is None):
        p.error("surveillance needs --core and --data")

    who = accounts(args.gateway, args.gateway_binary, args.state)
    fred = os.environ.get("FRED_API_KEY")
    ratings = sports_catalog.Ratings(args.state)
    sports_on = bool(
        os.environ.get("BALLDONTLIE_API_KEY") and os.environ.get("FOOTBALL_DATA_API_KEY")
    )
    maker = MarketMaker(
        who["maker"],
        FairValues(fred, sports=ratings.lookup if sports_on else None),
        say=lambda event, **k: log(f"maker_{event}", **k) if event in MAKER_LOGGED else None,
    )
    forecasts = Forecasts()
    store = EvidenceStore(args.state / "evidence")
    verified: set[int] = set()
    last = dict.fromkeys(EVERY, 0.0)
    log("start", gateway=args.gateway, tasks=sorted(tasks))
    while True:
        now = time.time()
        try:
            if "list" in tasks and now - last["list"] >= EVERY["list"]:
                at = dt.datetime.now(dt.UTC)
                listed = list_catalogue(
                    who["lister"], at, forecasts.high, lambda *a, **k: None, forecasts.low
                )
                log("listed", slugs=listed)
                if fred:
                    log("economy_listed", slugs=economy.run_once(who["lister"], at, fred, log))
                last["list"] = now
            if "list" in tasks and sports_on and now - last["sports"] >= EVERY["sports"]:
                at = dt.datetime.now(dt.UTC)
                games = sports_catalog.run_once(who["lister"], ratings, at, log)
                log("sports_listed", slugs=games)
                last["sports"] = now
            if "maker" in tasks:
                maker.run_once()
            if "resolve" in tasks and now - last["resolve"] >= EVERY["resolve"]:
                resolve.run_once(
                    SessionGateway(who["resolver"]),
                    store,
                    int(now),
                    fred,
                    say=lambda kind, **f: log(f"resolver_{kind}", **f),
                    verified=verified,
                )
                last["resolve"] = now
            if "surveil" in tasks and now - last["surveil"] >= EVERY["surveil"]:
                surveillance.run_once(who["watch"], surveillance.export(args.core, args.data))
                last["surveil"] = now
        except (ApiError, OSError, subprocess.CalledProcessError) as e:
            log("pass_failed", error=f"{type(e).__name__}: {e}")
        time.sleep(max(0.0, TICK - (time.time() - now)))


if __name__ == "__main__":
    sys.exit(main())
