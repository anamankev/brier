"""The four detectors of SPEC §9, each a score per (account, market) from the tape.

A higher score is more suspicious. A score of zero is never an alert; an alert is a score at
or above its typology's threshold, chosen on separate data (docs/notes/surveillance.md §3).
The parameters below were fixed before any evaluation ran (§3a) and are not tuned.

``links`` maps an account to the accounts it shares a login signal with (a keyed hash of the
client address, recorded by the gateway). It is weak evidence: households share addresses.
"""

from __future__ import annotations

import statistics
from collections import Counter, defaultdict
from collections.abc import Callable, Mapping, Set
from dataclasses import dataclass, field
from typing import Any

from brier_services.surveillance.tape import Fill, MarketTape, Tape

SPOOF_SIZE_MULTIPLE = 4
SPOOF_TOUCH_TICKS = 2
SPOOF_WINDOW = 30
SPOOF_UNPAIRED_WEIGHT = 0.25
CLOSE_WINDOW = 60
INSIDER_WINDOW = 180

Links = Mapping[int, Set[int]]


@dataclass(frozen=True)
class Alert:
    typology: str
    market: int
    account: int
    score: float
    evidence: dict[str, Any] = field(default_factory=dict, compare=False)


def wash(tape: Tape, links: Links) -> list[Alert]:
    """Contracts traded with linked accounts, as a share of the account's volume.

    Self-trade prevention stops an account trading with itself, so a wash trade needs two
    accounts; the only sign that two accounts are one person is a shared login signal.
    """
    out = []
    for m in tape.markets.values():
        volume: Counter[int] = Counter()
        linked: Counter[int] = Counter()
        with_whom: dict[int, Counter[int]] = defaultdict(Counter)
        seqs: dict[int, list[int]] = defaultdict(list)
        for f in m.fills:
            volume[f.buyer] += f.qty
            volume[f.seller] += f.qty
            if f.seller in links.get(f.buyer, ()):
                for a, b in ((f.buyer, f.seller), (f.seller, f.buyer)):
                    linked[a] += f.qty
                    with_whom[a][b] += f.qty
                    seqs[a].append(f.seq)
        for account, qty in linked.items():
            out.append(
                Alert(
                    "wash",
                    m.market,
                    account,
                    qty / volume[account],
                    {
                        "linked_contracts": qty,
                        "contracts": volume[account],
                        "counterparties": dict(with_whom[account]),
                        "trade_seqs": seqs[account],
                    },
                )
            )
    return out


def _is_on_side(f: Fill, account: int, side: str) -> bool:
    return (f.buyer if side == "buy" else f.seller) == account


def spoof(tape: Tape, links: Links) -> list[Alert]:
    """Large orders near the touch, cancelled unfilled soon after, around trades the other way.

    An order counts if it is GTC, at least ``SPOOF_SIZE_MULTIPLE`` times the market's median
    GTC size, within ``SPOOF_TOUCH_TICKS`` of its own side's best price when it arrived, and
    cancelled by its owner, unfilled, within ``SPOOF_WINDOW`` market commands. It scores 1 if
    the account traded on the other side within the window of the cancel, else
    ``SPOOF_UNPAIRED_WEIGHT``: a large order that is pulled is common; one that is pulled
    after the owner traded against the price it supported is the pattern.
    """
    out = []
    for m in tape.markets.values():
        sizes = [o.qty for o in m.orders.values() if o.tif == "gtc"]
        if not sizes:
            continue
        large = SPOOF_SIZE_MULTIPLE * statistics.median(sizes)
        fills_of: dict[int, list[Fill]] = defaultdict(list)
        for f in m.fills:
            fills_of[f.buyer].append(f)
            fills_of[f.seller].append(f)
        score: dict[int, float] = defaultdict(float)
        orders: dict[int, list[dict[str, Any]]] = defaultdict(list)
        for o in m.orders.values():
            touch = o.best_bid if o.side == "buy" else o.best_ask
            cancelled = o.cancelled_clock
            if (
                o.tif != "gtc"
                or o.qty < large
                or o.filled
                or o.cancel_reason != "user"
                or cancelled is None
                or cancelled - o.clock > SPOOF_WINDOW
                or touch is None
                or abs(o.price - touch) > SPOOF_TOUCH_TICKS
            ):
                continue
            other = "sell" if o.side == "buy" else "buy"
            against = [
                f.seq
                for f in fills_of[o.account]
                if _is_on_side(f, o.account, other) and abs(f.clock - cancelled) <= SPOOF_WINDOW
            ]
            score[o.account] += 1.0 if against else SPOOF_UNPAIRED_WEIGHT
            orders[o.account].append(
                {
                    "order_seq": o.seq,
                    "side": o.side,
                    "price": o.price,
                    "qty": o.qty,
                    "touch": touch,
                    "against_seqs": against,
                }
            )
        for account, s in score.items():
            out.append(
                Alert(
                    "spoof",
                    m.market,
                    account,
                    s,
                    {"median_gtc": large / SPOOF_SIZE_MULTIPLE, "orders": orders[account]},
                )
            )
    return out


def _window(m: MarketTape, size: int) -> tuple[int, int] | None:
    """(start, close) clocks: fills with start < clock < close are in the final window."""
    if m.close_clock is None:
        return None
    return (m.close_clock - size, m.close_clock)


def close(tape: Tape, links: Links) -> list[Alert]:
    """In the final window, the account's share of taker volume times the price move it made.

    The move is the change in the last trade price across each of its orders that traded,
    summed with sign and taken in absolute value: buying the close up and selling it down
    both count.
    """
    out = []
    for m in tape.markets.values():
        window = _window(m, CLOSE_WINDOW)
        if window is None:
            continue
        start, end = window
        last = None
        taken: Counter[int] = Counter()
        push: Counter[int] = Counter()
        seqs: dict[int, list[int]] = defaultdict(list)
        by_order: dict[int, list[Fill]] = defaultdict(list)
        for f in m.fills:
            if f.clock <= start:
                last = f.price
            elif f.clock < end:
                by_order[f.taker_order].append(f)
        for fills in sorted(by_order.values(), key=lambda fs: fs[0].seq):
            taker = fills[0].taker
            taken[taker] += sum(f.qty for f in fills)
            if last is not None:
                push[taker] += fills[-1].price - last
            last = fills[-1].price
            seqs[taker].append(fills[0].seq)
        total = sum(taken.values())
        for account, qty in taken.items():
            moved = abs(push[account])
            if moved:
                out.append(
                    Alert(
                        "close",
                        m.market,
                        account,
                        qty / total * moved,
                        {
                            "taker_contracts": qty,
                            "window_taker_contracts": total,
                            "ticks_moved": push[account],
                            "trade_seqs": seqs[account],
                        },
                    )
                )
    return out


def insider(tape: Tape, links: Links) -> list[Alert]:
    """A position built in the final window on the side that won, out of character.

    ``D`` is the contracts of net position the account added toward the winning outcome in
    the final window, ``E`` its traded contracts in the market before the window. The score
    is ``D * D/(D+E)`` times a weight: 1, plus 1 if the account opened after the market did,
    plus 1 if it is linked to another account. Runs only on markets that settled YES or NO.
    """
    out = []
    for m in tape.markets.values():
        window = _window(m, INSIDER_WINDOW)
        if window is None or m.outcome not in {"yes", "no"}:
            continue
        start, end = window
        sign = 1 if m.outcome == "yes" else -1
        net: Counter[int] = Counter()
        before: Counter[int] = Counter()
        seqs: dict[int, list[int]] = defaultdict(list)
        for f in m.fills:
            for account, signed in ((f.buyer, f.qty), (f.seller, -f.qty)):
                if f.clock <= start:
                    before[account] += f.qty
                elif f.clock < end:
                    net[account] += signed
                    seqs[account].append(f.seq)
        for account, n in net.items():
            d = max(0, sign * n)
            if not d:
                continue
            new = tape.account_opened.get(account, 0) > (m.open_seq or 0)
            linked = bool(links.get(account))
            weight = 1 + int(new) + int(linked)
            out.append(
                Alert(
                    "insider",
                    m.market,
                    account,
                    d * d / (d + before[account]) * weight,
                    {
                        "winning_contracts": d,
                        "earlier_contracts": before[account],
                        "opened_after_market": new,
                        "linked": linked,
                        "trade_seqs": seqs[account],
                    },
                )
            )
    return out


DETECTORS: dict[str, Callable[[Tape, Links], list[Alert]]] = {
    "wash": wash,
    "spoof": spoof,
    "close": close,
    "insider": insider,
}


def scores(tape: Tape, links: Links) -> list[Alert]:
    """Every nonzero score from every detector."""
    return [a for detect in DETECTORS.values() for a in detect(tape, links)]


def alerts(found: list[Alert], thresholds: Mapping[str, float]) -> list[Alert]:
    """The scores at or above their typology's threshold."""
    return [a for a in found if a.score >= thresholds[a.typology]]
