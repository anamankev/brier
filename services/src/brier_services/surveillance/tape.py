"""The tape: every order, cancel, and trade in each market, rebuilt from the command stream.

The input is the exchange's own record, one wire command with its events at a time: a live
journal exported by ``brier-core export`` and replayed through the reference engine, or a
simulated market's commands as they ran. Both give the same records, so the detectors cannot
tell them apart.

Time is a market's own clock: the count of commands that concern it (orders in it, cancels
and amends of its orders, its own state changes). Journal commands carry no wall-clock time
(docs/protocol.md §8), and counting only a market's own commands keeps one busy market from
shrinking another's windows.
"""

from __future__ import annotations

from collections.abc import Iterable, Iterator, Mapping
from dataclasses import dataclass, field
from typing import Any

from brier_services.reference_engine import ReferenceEngine

Record = tuple[Mapping[str, Any], list[dict[str, Any]]]


@dataclass
class Order:
    id: int
    account: int
    market: int
    side: str
    price: int
    qty: int
    tif: str
    seq: int
    clock: int
    best_bid: int | None  # the book before this order arrived
    best_ask: int | None
    filled: int = 0
    rested: bool = False
    cancelled_clock: int | None = None
    cancel_reason: str | None = None


@dataclass(frozen=True)
class Fill:
    seq: int
    clock: int
    price: int
    qty: int
    buyer: int
    seller: int
    taker: int
    taker_side: str
    maker_order: int
    taker_order: int


@dataclass
class MarketTape:
    market: int
    clock: int = 0
    open_seq: int | None = None
    close_clock: int | None = None
    outcome: str | None = None
    orders: dict[int, Order] = field(default_factory=dict)
    fills: list[Fill] = field(default_factory=list)
    # Resting orders by ID (the live book), kept in step with the events.
    book: dict[int, int] = field(default_factory=dict)  # order -> remaining

    def best(self, side: str) -> int | None:
        prices = [self.orders[o].price for o in self.book if self.orders[o].side == side]
        if not prices:
            return None
        return max(prices) if side == "buy" else min(prices)

    def accounts(self) -> set[int]:
        return {o.account for o in self.orders.values()}


@dataclass
class Tape:
    markets: dict[int, MarketTape] = field(default_factory=dict)
    account_opened: dict[int, int] = field(default_factory=dict)  # account -> seq
    market_of: dict[int, int] = field(default_factory=dict)  # order -> market

    def add(self, cmd: Mapping[str, Any], events: list[dict[str, Any]]) -> None:
        """Add one command and its events."""
        seq = int(cmd["seq"])
        kind = cmd["cmd"]
        if kind == "open_account":
            if any(e["ev"] == "account_opened" for e in events):
                self.account_opened[int(cmd["account"])] = seq
            return
        market = self._market_of_command(cmd)
        if market is None:  # halt_all, a cancel_all with no market, an unknown order
            for m in {
                self.market_of[e["order"]] for e in events if e.get("order") in self.market_of
            }:
                self._apply_events(self.markets[m], seq, events, None)
            return
        tape = self.markets.setdefault(market, MarketTape(market))
        tape.clock += 1
        placed = None
        if kind == "place" and events and events[0]["ev"] == "accepted":
            placed = Order(
                id=int(events[0]["order"]),
                account=int(cmd["account"]),
                market=market,
                side=str(cmd["side"]),
                price=int(cmd["price"]),
                qty=int(cmd["qty"]),
                tif=str(cmd["tif"]),
                seq=seq,
                clock=tape.clock,
                best_bid=tape.best("buy"),
                best_ask=tape.best("sell"),
            )
            tape.orders[placed.id] = placed
            self.market_of[placed.id] = market
        elif kind == "set_state":
            if cmd["state"] == "open" and tape.open_seq is None:
                tape.open_seq = seq
            if cmd["state"] == "closed" and tape.close_clock is None:
                tape.close_clock = tape.clock
        elif kind == "settle" and any(e["ev"] == "settled" for e in events):
            tape.outcome = str(cmd["outcome"])
        self._apply_events(tape, seq, events, placed)

    def _market_of_command(self, cmd: Mapping[str, Any]) -> int | None:
        if "market" in cmd:
            return int(cmd["market"])
        if cmd["cmd"] in {"cancel", "amend"}:
            return self.market_of.get(int(cmd["order"]))
        return None

    def _apply_events(
        self, tape: MarketTape, seq: int, events: list[dict[str, Any]], placed: Order | None
    ) -> None:
        for e in events:
            ev = e["ev"]
            if ev == "trade":
                maker = tape.orders[int(e["maker"])]
                qty = int(e["qty"])
                buyer, seller = (
                    (int(e["taker_account"]), int(e["maker_account"]))
                    if e["taker_side"] == "buy"
                    else (int(e["maker_account"]), int(e["taker_account"]))
                )
                tape.fills.append(
                    Fill(
                        seq=seq,
                        clock=tape.clock,
                        price=int(e["price"]),
                        qty=qty,
                        buyer=buyer,
                        seller=seller,
                        taker=int(e["taker_account"]),
                        taker_side=str(e["taker_side"]),
                        maker_order=maker.id,
                        taker_order=int(e["taker"]),
                    )
                )
                maker.filled += qty
                if placed is not None:
                    placed.filled += qty
                left = tape.book.get(maker.id, 0) - qty
                if left > 0:
                    tape.book[maker.id] = left
                else:
                    tape.book.pop(maker.id, None)
            elif ev == "rested":
                tape.book[int(e["order"])] = int(e["remaining"])
                tape.orders[int(e["order"])].rested = True
            elif ev == "amended":
                tape.book[int(e["order"])] = int(e["remaining"])
            elif ev == "cancelled":
                order = tape.orders.get(int(e["order"]))
                if order is not None:
                    order.cancelled_clock = tape.clock
                    order.cancel_reason = str(e["reason"])
                tape.book.pop(int(e["order"]), None)


def replay(commands: Iterable[Mapping[str, Any]]) -> Iterator[Record]:
    """Each wire command with the events the reference engine gives it (an exported journal)."""
    engine = ReferenceEngine()
    for cmd in commands:
        yield cmd, engine.apply(cmd)


def build(records: Iterable[Record]) -> Tape:
    tape = Tape()
    for cmd, events in records:
        tape.add(cmd, events)
    return tape
