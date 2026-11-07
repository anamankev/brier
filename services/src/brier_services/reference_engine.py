"""Reference matching engine (SPEC §3.4), written from docs/gates/G1-plan.md and SPEC §2-§3.

This engine exists to be obviously correct, not fast. Every resting order sits in one plain
list per market; every decision re-scans and re-sorts that list. Nothing here is derived from
the Rust engine's code. Differential tests feed both engines the same wire commands and
require identical events (``brier_services.differential``).

Input and output use the shared wire format: one dict per command, one dict per event.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Final

from brier_services.units import MAX_ORDER_QTY, MAX_PRICE_TICKS, MIN_PRICE_TICKS

U8_MAX: Final = 2**8 - 1
U32_MAX: Final = 2**32 - 1
U64_MAX: Final = 2**64 - 1

TRANSITIONS: Final = {
    ("pre_open", "open"),
    ("pre_open", "closed"),
    ("open", "halted"),
    ("open", "closed"),
    ("halted", "open"),
    ("halted", "closed"),
    ("closed", "resolved"),
    ("closed", "voided"),
}
STATES: Final = {"pre_open", "open", "halted", "closed", "resolved", "voided"}

Event = dict[str, Any]


class MalformedCommandError(ValueError):
    """A wire command with a missing field, a wrong type, or a value outside its wire range."""


class EngineHaltedError(RuntimeError):
    """The input broke the sequence rule, or the engine found itself in an impossible state."""


@dataclass
class _Order:
    id: int
    account: int
    side: str
    price: int
    remaining: int


def _int_field(cmd: Mapping[str, Any], name: str, maximum: int) -> int:
    value = cmd.get(name)
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= maximum:
        msg = f"field {name!r} must be an integer in [0, {maximum}], got {value!r}"
        raise MalformedCommandError(msg)
    return value


def _choice_field(cmd: Mapping[str, Any], name: str, allowed: set[str]) -> str:
    value = cmd.get(name)
    if not isinstance(value, str) or value not in allowed:
        msg = f"field {name!r} must be one of {sorted(allowed)}, got {value!r}"
        raise MalformedCommandError(msg)
    return value


def _priority(taker_side: str) -> Any:  # noqa: ANN401 - a sort key function
    """Sort key for the makers an incoming order meets: best price first, then oldest."""
    if taker_side == "buy":
        return lambda o: (o.price, o.id)  # asks: lowest price first
    return lambda o: (-o.price, o.id)  # bids: highest price first


class ReferenceEngine:
    """Plain-list matching engine with the exact semantics of the G1 plan."""

    def __init__(self) -> None:
        self.next_seq = 1
        self.states: dict[int, str] = {}
        self.books: dict[int, list[_Order]] = {}

    # ----- dispatch -----

    def apply(self, cmd: Mapping[str, Any]) -> list[Event]:
        """Apply one wire command and return its events (each tagged with the command's seq)."""
        seq = _int_field(cmd, "seq", U64_MAX)
        kind = _choice_field(cmd, "cmd", {"create_market", "set_state", "place", "cancel", "amend"})
        if seq != self.next_seq:
            msg = f"sequence gap: expected {self.next_seq}, got {seq}"
            raise EngineHaltedError(msg)
        if kind == "create_market":
            events = self._create(_int_field(cmd, "market", U32_MAX))
        elif kind == "set_state":
            events = self._set_state(
                _int_field(cmd, "market", U32_MAX), _choice_field(cmd, "state", STATES)
            )
        elif kind == "place":
            events = self._place(seq, cmd)
        elif kind == "cancel":
            events = self._cancel(
                _int_field(cmd, "account", U64_MAX), _int_field(cmd, "order", U64_MAX)
            )
        else:
            events = self._amend(
                _int_field(cmd, "account", U64_MAX),
                _int_field(cmd, "order", U64_MAX),
                _int_field(cmd, "qty", U32_MAX),
            )
        self.next_seq = seq + 1
        for event in events:
            event["seq"] = seq
        return events

    # ----- commands -----

    def _create(self, market: int) -> list[Event]:
        if market in self.states:
            return [{"ev": "rejected", "reason": "market_exists"}]
        self.states[market] = "pre_open"
        self.books[market] = []
        return [{"ev": "market_created", "market": market}]

    def _set_state(self, market: int, state: str) -> list[Event]:
        if market not in self.states:
            return [{"ev": "rejected", "reason": "unknown_market"}]
        if (self.states[market], state) not in TRANSITIONS:
            return [{"ev": "rejected", "reason": "invalid_transition"}]
        self.states[market] = state
        events: list[Event] = [{"ev": "market_state", "market": market, "state": state}]
        if state == "closed":
            events.extend(
                {
                    "ev": "cancelled",
                    "order": order.id,
                    "remaining": order.remaining,
                    "reason": "market_closed",
                }
                for order in sorted(self.books[market], key=lambda o: o.id)
            )
            self.books[market] = []
        return events

    def _place(self, seq: int, cmd: Mapping[str, Any]) -> list[Event]:
        account = _int_field(cmd, "account", U64_MAX)
        market = _int_field(cmd, "market", U32_MAX)
        side = _choice_field(cmd, "side", {"buy", "sell"})
        price = _int_field(cmd, "price", U8_MAX)
        qty = _int_field(cmd, "qty", U32_MAX)
        tif = _choice_field(cmd, "tif", {"gtc", "ioc", "fok"})
        post_only = cmd.get("post_only")
        if not isinstance(post_only, bool):
            msg = f"field 'post_only' must be a boolean, got {post_only!r}"
            raise MalformedCommandError(msg)

        def reject(reason: str) -> list[Event]:
            return [{"ev": "rejected", "reason": reason}]

        if market not in self.states:
            return reject("unknown_market")
        if self.states[market] != "open":
            return reject("market_not_open")
        if not MIN_PRICE_TICKS <= price <= MAX_PRICE_TICKS:
            return reject("invalid_price")
        if not 1 <= qty <= MAX_ORDER_QTY:
            return reject("invalid_quantity")
        if post_only and tif != "gtc":
            return reject("invalid_order")

        book = self.books[market]

        def crossing_makers() -> list[_Order]:
            if side == "buy":
                makers = [o for o in book if o.side == "sell" and o.price <= price]
            else:
                makers = [o for o in book if o.side == "buy" and o.price >= price]
            return sorted(makers, key=_priority(side))

        if post_only and crossing_makers():
            return reject("post_only_would_cross")
        if tif == "fok":
            fillable = 0
            for maker in crossing_makers():
                if maker.account == account or fillable >= qty:
                    break
                fillable += maker.remaining
            if fillable < qty:
                return reject("fok_unfillable")

        order_id = seq
        events: list[Event] = [{"ev": "accepted", "order": order_id}]
        remaining = qty
        while remaining > 0:
            makers = crossing_makers()
            if not makers:
                break
            maker = makers[0]
            if maker.account == account:
                events.append(
                    {
                        "ev": "cancelled",
                        "order": order_id,
                        "remaining": remaining,
                        "reason": "self_trade",
                    }
                )
                return events
            fill = min(remaining, maker.remaining)
            events.append(
                {
                    "ev": "trade",
                    "market": market,
                    "maker": maker.id,
                    "taker": order_id,
                    "maker_account": maker.account,
                    "taker_account": account,
                    "taker_side": side,
                    "price": maker.price,
                    "qty": fill,
                }
            )
            remaining -= fill
            maker.remaining -= fill
            if maker.remaining == 0:
                book.remove(maker)
        if remaining == 0:
            return events
        if tif == "gtc":
            book.append(_Order(order_id, account, side, price, remaining))
            events.append({"ev": "rested", "order": order_id, "remaining": remaining})
        elif tif == "ioc":
            events.append(
                {
                    "ev": "cancelled",
                    "order": order_id,
                    "remaining": remaining,
                    "reason": "ioc_remainder",
                }
            )
        else:
            msg = "fill-or-kill passed its check but did not fill"
            raise EngineHaltedError(msg)
        return events

    def _find_own(self, account: int, order_id: int) -> tuple[int, _Order] | None:
        for market, book in self.books.items():
            for order in book:
                if order.id == order_id:
                    return (market, order) if order.account == account else None
        return None

    def _cancel(self, account: int, order_id: int) -> list[Event]:
        found = self._find_own(account, order_id)
        if found is None:
            return [{"ev": "rejected", "reason": "unknown_order"}]
        market, order = found
        self.books[market].remove(order)
        return [
            {"ev": "cancelled", "order": order.id, "remaining": order.remaining, "reason": "user"}
        ]

    def _amend(self, account: int, order_id: int, qty: int) -> list[Event]:
        found = self._find_own(account, order_id)
        if found is None:
            return [{"ev": "rejected", "reason": "unknown_order"}]
        market, order = found
        if self.states[market] != "open":
            return [{"ev": "rejected", "reason": "market_not_open"}]
        if not 1 <= qty <= MAX_ORDER_QTY:
            return [{"ev": "rejected", "reason": "invalid_quantity"}]
        if qty >= order.remaining:
            return [{"ev": "rejected", "reason": "invalid_amend"}]
        order.remaining = qty
        return [{"ev": "amended", "order": order.id, "remaining": qty}]

    # ----- output -----

    def final_book(self) -> Event:
        """The end-of-input record: every market's state and both sides in time priority."""
        markets = []
        for market in sorted(self.states):
            book = self.books[market]
            bids = sorted((o for o in book if o.side == "buy"), key=_priority("sell"))
            asks = sorted((o for o in book if o.side == "sell"), key=_priority("buy"))
            markets.append(
                {
                    "market": market,
                    "state": self.states[market],
                    "bids": [[o.id, o.account, o.price, o.remaining] for o in bids],
                    "asks": [[o.id, o.account, o.price, o.remaining] for o in asks],
                }
            )
        return {"ev": "final_book", "markets": markets}
