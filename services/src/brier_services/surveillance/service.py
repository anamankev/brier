"""The surveillance service on the live exchange (docs/surveillance.md).

    python -m brier_services.surveillance run --gateway URL --key-id N --key-file PATH --data DIR

Each pass:
1. ``brier-core export`` writes the journal out as wire commands, without touching it; the
   reference engine replays them into the tape.
2. The gateway's login links (``GET /v1/surveillance/links``) say which accounts are linked.
3. The detectors run with the live rules below, and every score at or above its threshold
   opens a case (``POST /v1/surveillance/cases``), with its evidence: the detector's numbers,
   the account's orders, cancels, and fills in the market with the book at each order, and
   its P&L there. A case already open for the same alert is not opened again.

It runs under its own account with the ``surveillance`` role and a signed key: it can read
links and open cases, and cannot trade, forecast, or close a case.
"""

from __future__ import annotations

import json
import subprocess
from collections.abc import Callable, Iterable, Mapping
from pathlib import Path
from typing import Any, Protocol

from brier_services.surveillance import detectors
from brier_services.surveillance.detectors import Alert, Links
from brier_services.surveillance.tape import MarketTape, Tape, build, replay

# The thresholds chosen on the tuning seeds (docs/notes/surveillance.md §4), and the spoofing
# rule measured in §5: only large cancelled orders paired with an opposite trade the account
# initiated count. Marking the close did not meet its registered precision: its cases are
# leads for a person to check, as every case is.
LIVE_RULES: dict[str, float] = {"SPOOF_UNPAIRED_WEIGHT": 0.0, "SPOOF_AGAINST_AS_TAKER_ONLY": 1.0}
LIVE_THRESHOLDS: dict[str, float] = {
    "wash": 29 / 97,
    "spoof": 1.0,
    "close": 2.489082969432314,
    "insider": 194.0,
}
DETECTOR = "brier surveillance v1 (docs/notes/surveillance.md)"
MAX_BODY = 7_000  # the gateway accepts 8 KiB bodies
MAX_LIST = 50  # entries kept in each evidence list
PAYOUT = 100


class Api(Protocol):
    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any: ...  # noqa: ANN401 - JSON


def export(core: Path, data: Path) -> list[dict[str, Any]]:
    """Every journalled command, as `brier-core export` writes it."""
    out = subprocess.run(  # noqa: S603 - a fixed program and arguments, no shell
        [str(core), "export", "--data", str(data)], capture_output=True, check=True, text=True
    )
    return [json.loads(line) for line in out.stdout.splitlines() if line.strip()]


def links(api: Api) -> dict[int, set[int]]:
    linked: dict[int, set[int]] = {}
    for link in api.request("GET", "/v1/surveillance/links")["links"]:
        a, b = link["accounts"]
        linked.setdefault(a, set()).add(b)
        linked.setdefault(b, set()).add(a)
    return linked


def _seqs(evidence: Mapping[str, Any]) -> list[int]:
    seqs = list(evidence.get("trade_seqs", []))
    for o in evidence.get("orders", []):
        seqs += [o["order_seq"], *o.get("against_seqs", [])]
    return seqs


def timeline(m: MarketTape, account: int, near: list[int], limit: int) -> list[list[Any]]:
    """The account's orders, cancels, and fills in the market, nearest the alert first kept.

    Entries: ``[seq, "place", side, price, qty, tif, best_bid, best_ask]`` (the book before
    the order), ``[seq, "cancel", order_seq, remaining_reason]``, and ``[seq, "fill", side,
    price, qty, "taker" | "maker", counterparty]``.
    """
    mine = [o for o in m.orders.values() if o.account == account]
    entries: list[list[Any]] = [
        [o.seq, "place", o.side, o.price, o.qty, o.tif, o.best_bid, o.best_ask] for o in mine
    ]
    for f in m.fills:
        if account in (f.buyer, f.seller):
            side = "buy" if f.buyer == account else "sell"
            other = f.seller if side == "buy" else f.buyer
            entries.append(
                [
                    f.seq,
                    "fill",
                    side,
                    f.price,
                    f.qty,
                    "taker" if f.taker == account else "maker",
                    other,
                ]
            )
    entries.extend(
        [o.cancel_seq, "cancel", o.seq, o.cancel_reason]
        for o in mine
        if o.cancel_reason is not None and o.filled < o.qty
    )
    if len(entries) > limit and near:
        entries.sort(key=lambda e: min(abs(e[0] - s) for s in near))
        entries = entries[:limit]
    entries.sort(key=lambda e: (e[0], e[1]))
    return entries


def pnl(m: MarketTape, account: int) -> dict[str, Any]:
    """Cash and position from the account's fills in the market, valued at settlement (or at
    the last trade price before it), in credit-cents."""
    cash = position = 0
    for f in m.fills:
        if f.buyer == account:
            cash -= f.price * f.qty
            position += f.qty
        elif f.seller == account:
            cash += f.price * f.qty
            position -= f.qty
    if m.outcome in {"yes", "no"}:
        mark, basis = (PAYOUT if m.outcome == "yes" else 0), m.outcome
    else:
        mark, basis = (m.fills[-1].price if m.fills else 0), "last trade"
    return {"cash": cash, "position": position, "value": cash + position * mark, "valued_at": basis}


def evidence(tape: Tape, alert: Alert) -> dict[str, Any]:
    m = tape.markets[alert.market]
    detail = {k: (v[-MAX_LIST:] if isinstance(v, list) else v) for k, v in alert.evidence.items()}
    out: dict[str, Any] = {**detail, "pnl": pnl(m, alert.account), "close_clock": m.close_clock}
    limit = 150
    while True:
        out["timeline"] = timeline(m, alert.account, _seqs(alert.evidence), limit)
        body = {
            "typology": alert.typology,
            "market": alert.market,
            "account": alert.account,
            "score": alert.score,
            "detector": DETECTOR,
            "evidence": out,
        }
        if len(json.dumps(body)) <= MAX_BODY or limit <= 5:
            return out
        limit //= 2


def run_once(
    api: Api, commands: Iterable[Mapping[str, Any]], say: Callable[[str], None] = print
) -> list[dict[str, Any]]:
    """Detect over the whole stream and open a case for each alert; returns what was opened."""
    tape = build(replay(commands))
    linked: Links = links(api)
    with detectors.configured(LIVE_RULES):
        found = detectors.alerts(detectors.scores(tape, linked), LIVE_THRESHOLDS)
    opened = []
    for a in found:
        body = {
            "typology": a.typology,
            "market": a.market,
            "account": a.account,
            "score": a.score,
            "detector": DETECTOR,
            "evidence": evidence(tape, a),
        }
        r = api.request("POST", "/v1/surveillance/cases", body)
        if r.get("opened"):
            opened.append(
                {
                    "case": r["case"],
                    "typology": a.typology,
                    "market": a.market,
                    "account": a.account,
                }
            )
            say(json.dumps({"event": "case_opened", **opened[-1]}, sort_keys=True))
    say(
        json.dumps(
            {
                "event": "pass",
                "markets": len(tape.markets),
                "alerts": len(found),
                "opened": len(opened),
            }
        )
    )
    return opened
