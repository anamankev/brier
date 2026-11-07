"""Reference engine (SPEC §3.4), written from docs/gates/G1-plan.md, G2-plan.md, and SPEC §2-§4.

This engine exists to be obviously correct, not fast. Every resting order sits in one plain
list per market, and every decision re-scans and re-sorts that list. Collateral is recomputed
from the account's resting orders each time, not from running totals. Nothing here is derived
from the Rust engine's code. Differential tests feed both engines the same wire commands and
require identical events (``brier_services.differential``).

Input and output use the shared wire format: one dict per command, one dict per event.
"""

from __future__ import annotations

from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any, Final

from brier_services.units import MAX_ORDER_QTY, MAX_PRICE_TICKS, MIN_PRICE_TICKS, PAYOUT_TICKS

U8_MAX: Final = 2**8 - 1
U32_MAX: Final = 2**32 - 1
U64_MAX: Final = 2**64 - 1
SIGNUP_GRANT: Final = 1_000_000  # credit-cents (G2 plan)
MAX_RESTING_ORDERS: Final = 1_000  # GTC orders per account (G2 plan)

# set_state transitions; Resolved and Voided are reached only by `settle` (G2 plan).
TRANSITIONS: Final = {
    ("pre_open", "open"),
    ("pre_open", "closed"),
    ("open", "halted"),
    ("open", "closed"),
    ("halted", "open"),
    ("halted", "closed"),
}
STATES: Final = {"pre_open", "open", "halted", "closed", "resolved", "voided"}
COMMANDS: Final = {
    "create_market",
    "set_state",
    "place",
    "cancel",
    "amend",
    "open_account",
    "settle",
    "cancel_all",
    "halt_all",
}

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


def _optional_int_field(cmd: Mapping[str, Any], name: str, maximum: int) -> int | None:
    return None if cmd.get(name) is None else _int_field(cmd, name, maximum)


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


def _entry(kind: str, legs: list[tuple[str, int]]) -> Event:
    if sum(amount for _, amount in legs) != 0:
        msg = f"unbalanced {kind} entry: {legs}"
        raise EngineHaltedError(msg)
    return {"ev": "entry", "kind": kind, "legs": [[name, x] for name, x in legs if x != 0]}


class ReferenceEngine:
    """Plain-list engine with the exact semantics of the G1 and G2 plans."""

    def __init__(self) -> None:
        self.next_seq = 1
        self.states: dict[int, str] = {}
        self.books: dict[int, list[_Order]] = {}
        # Which market each resting order is in, so a cancel or amend searches one market's
        # list instead of every market's. Kept in step with the books at every add and removal.
        self.market_of: dict[int, int] = {}
        self.cash: dict[int, int] = {}
        self.issuance = 0
        self.escrow: dict[int, int] = {}
        self.positions: dict[int, dict[int, int]] = {}  # market -> account -> contracts
        self.reserve: dict[tuple[int, int], int] = {}  # (account, market) -> last charged
        self.reserved_by_account: dict[int, int] = {}  # sum of `reserve` per account
        self.resting_count: dict[int, int] = {}
        self.touched: set[tuple[int, int]] = set()  # (account, market) this command

    # ----- dispatch -----

    def apply(self, cmd: Mapping[str, Any]) -> list[Event]:
        """Apply one wire command and return its events (each tagged with the command's seq)."""
        seq = _int_field(cmd, "seq", U64_MAX)
        kind = _choice_field(cmd, "cmd", COMMANDS)
        if seq != self.next_seq:
            msg = f"sequence gap: expected {self.next_seq}, got {seq}"
            raise EngineHaltedError(msg)
        self.touched = set()
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
        elif kind == "amend":
            events = self._amend(
                _int_field(cmd, "account", U64_MAX),
                _int_field(cmd, "order", U64_MAX),
                _int_field(cmd, "qty", U32_MAX),
            )
        elif kind == "open_account":
            events = self._open_account(_int_field(cmd, "account", U64_MAX))
        elif kind == "settle":
            events = self._settle(
                _int_field(cmd, "market", U32_MAX),
                _choice_field(cmd, "outcome", {"yes", "no", "void"}),
            )
        elif kind == "cancel_all":
            events = self._cancel_all(
                _optional_int_field(cmd, "account", U64_MAX),
                _optional_int_field(cmd, "market", U32_MAX),
            )
        else:
            events = self._halt_all()
        events.extend(self._finish_command())
        self.next_seq = seq + 1
        for event in events:
            event["seq"] = seq
        return events

    # ----- collateral (G2 plan, "Collateral"; ADR-0005) -----

    def _reserve_of(self, account: int, market: int, extra: _Order | None = None) -> int:
        """Worst-case cash the account's resting orders in this market could still need."""
        orders = [o for o in self.books.get(market, []) if o.account == account]
        if extra is not None:
            orders.append(extra)
        x = self.positions.get(market, {}).get(account, 0)
        total = 0
        free = max(0, -x)  # NO contracts held cover the cheapest buy units
        for o in sorted((o for o in orders if o.side == "buy"), key=lambda o: o.price):
            covered = min(o.remaining, free)
            free -= covered
            total += (o.remaining - covered) * o.price
        free = max(0, x)  # YES contracts held cover the cheapest sell units (highest prices)
        for o in sorted((o for o in orders if o.side == "sell"), key=lambda o: -o.price):
            covered = min(o.remaining, free)
            free -= covered
            total += (o.remaining - covered) * (PAYOUT_TICKS - o.price)
        return total

    def _reserved_total(self, account: int) -> int:
        return self.reserved_by_account.get(account, 0)

    def _finish_command(self) -> list[Event]:
        events: list[Event] = []
        for account, market in sorted(self.touched):
            new = self._reserve_of(account, market)
            old = self.reserve.get((account, market), 0)
            if new != old:
                self.reserve[(account, market)] = new
                self.reserved_by_account[account] = self._reserved_total(account) + new - old
                events.append(
                    {"ev": "reserved", "account": account, "market": market, "amount": new}
                )
        for account, _ in self.touched:
            if self.cash[account] < self._reserved_total(account):
                msg = f"account {account} has cash below its reserve"
                raise EngineHaltedError(msg)
        return events

    # ----- resting orders -----

    def _rest(self, market: int, order: _Order) -> None:
        self.books[market].append(order)
        self.market_of[order.id] = market
        self.resting_count[order.account] = self.resting_count.get(order.account, 0) + 1
        self.touched.add((order.account, market))

    def _unrest(self, market: int, order: _Order) -> None:
        self.books[market].remove(order)
        del self.market_of[order.id]
        self.resting_count[order.account] -= 1
        self.touched.add((order.account, market))

    # ----- commands -----

    def _create(self, market: int) -> list[Event]:
        if market in self.states:
            return [{"ev": "rejected", "reason": "market_exists"}]
        self.states[market] = "pre_open"
        self.books[market] = []
        self.escrow[market] = 0
        return [{"ev": "market_created", "market": market}]

    def _open_account(self, account: int) -> list[Event]:
        if account in self.cash:
            return [{"ev": "rejected", "reason": "account_exists"}]
        self.cash[account] = SIGNUP_GRANT
        self.issuance -= SIGNUP_GRANT
        return [
            {"ev": "account_opened", "account": account, "cash": SIGNUP_GRANT},
            _entry("grant", [("issuance", -SIGNUP_GRANT), (f"cash:{account}", SIGNUP_GRANT)]),
        ]

    def _set_state(self, market: int, state: str) -> list[Event]:
        if market not in self.states:
            return [{"ev": "rejected", "reason": "unknown_market"}]
        if (self.states[market], state) not in TRANSITIONS:
            return [{"ev": "rejected", "reason": "invalid_transition"}]
        self.states[market] = state
        events: list[Event] = [{"ev": "market_state", "market": market, "state": state}]
        if state == "closed":
            for order in sorted(self.books[market], key=lambda o: o.id):
                self._unrest(market, order)
                events.append(
                    {
                        "ev": "cancelled",
                        "order": order.id,
                        "remaining": order.remaining,
                        "reason": "market_closed",
                    }
                )
        return events

    def _settle(self, market: int, outcome: str) -> list[Event]:
        if market not in self.states:
            return [{"ev": "rejected", "reason": "unknown_market"}]
        if self.states[market] != "closed":
            return [{"ev": "rejected", "reason": "invalid_transition"}]
        self.states[market] = "voided" if outcome == "void" else "resolved"
        events: list[Event] = [{"ev": "settled", "market": market, "outcome": outcome}]
        held = self.positions.get(market, {})
        for account in sorted(held):
            x = held[account]
            if outcome == "void":
                amount = 50 * abs(x)
            elif (outcome == "yes" and x > 0) or (outcome == "no" and x < 0):
                amount = PAYOUT_TICKS * abs(x)
            else:
                amount = 0
            held[account] = 0
            if amount:
                self.escrow[market] -= amount
                self.cash[account] += amount
                events.append(
                    _entry("settle", [(f"escrow:{market}", -amount), (f"cash:{account}", amount)])
                )
        if self.escrow[market] != 0:
            msg = f"escrow of market {market} is {self.escrow[market]} after settlement"
            raise EngineHaltedError(msg)
        # A settled market never trades again; drop its per-account state.
        self.positions.pop(market, None)
        for pair in [pair for pair in self.reserve if pair[1] == market]:
            if self.reserve.pop(pair) != 0:
                msg = f"settled market {market} still charges a reserve"
                raise EngineHaltedError(msg)
        return events

    def _cancel_all(self, account: int | None, market: int | None) -> list[Event]:
        if account is not None and account not in self.cash:
            return [{"ev": "rejected", "reason": "unknown_account"}]
        if market is not None and market not in self.states:
            return [{"ev": "rejected", "reason": "unknown_market"}]
        targets = [
            (o, m)
            for m, book in self.books.items()
            if market is None or m == market
            for o in book
            if account is None or o.account == account
        ]
        events: list[Event] = []
        for order, m in sorted(targets, key=lambda t: t[0].id):
            self._unrest(m, order)
            events.append(
                {
                    "ev": "cancelled",
                    "order": order.id,
                    "remaining": order.remaining,
                    "reason": "kill_switch",
                }
            )
        return events

    def _halt_all(self) -> list[Event]:
        events: list[Event] = []
        for market in sorted(self.states):
            if self.states[market] == "open":
                self.states[market] = "halted"
                events.append({"ev": "market_state", "market": market, "state": "halted"})
        return events

    def _trade(self, market: int, buyer: int, seller: int, price: int, qty: int) -> Event:
        """Cash and positions for one trade (G2 plan, "Positions and trade cash")."""
        held = self.positions.setdefault(market, {})
        xb = held.get(buyer, 0)
        xs = held.get(seller, 0)
        buyer_closing = min(qty, max(0, -xb))
        seller_opening = qty - min(qty, max(0, xs))
        buyer_cash = -price * qty + PAYOUT_TICKS * buyer_closing
        seller_cash = price * qty - PAYOUT_TICKS * seller_opening
        escrow_change = PAYOUT_TICKS * seller_opening - PAYOUT_TICKS * buyer_closing
        self.cash[buyer] += buyer_cash
        self.cash[seller] += seller_cash
        self.escrow[market] += escrow_change
        held[buyer] = xb + qty
        held[seller] = xs - qty
        self.touched.add((buyer, market))
        self.touched.add((seller, market))
        return _entry(
            "trade",
            [
                (f"cash:{buyer}", buyer_cash),
                (f"cash:{seller}", seller_cash),
                (f"escrow:{market}", escrow_change),
            ],
        )

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
        if account not in self.cash:
            return reject("unknown_account")
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
        if tif == "gtc" and self.resting_count.get(account, 0) >= MAX_RESTING_ORDERS:
            return reject("too_many_orders")
        hypothetical = _Order(seq, account, side, price, qty)
        increase = self._reserve_of(account, market, hypothetical) - self.reserve.get(
            (account, market), 0
        )
        if increase > self.cash[account] - self._reserved_total(account):
            return reject("insufficient_collateral")

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
            buyer, seller = (account, maker.account) if side == "buy" else (maker.account, account)
            events.append(self._trade(market, buyer, seller, maker.price, fill))
            remaining -= fill
            maker.remaining -= fill
            self.touched.add((maker.account, market))
            if maker.remaining == 0:
                self._unrest(market, maker)
        if remaining == 0:
            return events
        if tif == "gtc":
            self._rest(market, _Order(order_id, account, side, price, remaining))
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
        market = self.market_of.get(order_id)
        if market is None:
            return None
        for order in self.books[market]:
            if order.id == order_id:
                return (market, order) if order.account == account else None
        msg = f"order {order_id} is indexed in market {market} but not in its book"
        raise EngineHaltedError(msg)

    def _cancel(self, account: int, order_id: int) -> list[Event]:
        found = self._find_own(account, order_id)
        if found is None:
            return [{"ev": "rejected", "reason": "unknown_order"}]
        market, order = found
        self._unrest(market, order)
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
        self.touched.add((account, market))
        return [{"ev": "amended", "order": order.id, "remaining": qty}]

    # ----- checks and output -----

    def check_invariants(self) -> None:
        """I1-I4 from SPEC §4.3, recomputed from scratch; raise if any fails."""
        if sum(self.cash.values()) + sum(self.escrow.values()) + self.issuance != 0:
            msg = "I1: credits are not conserved"
            raise EngineHaltedError(msg)
        for market, escrow in self.escrow.items():
            held = self.positions.get(market, {}).values()
            long = sum(x for x in held if x > 0)
            short = sum(-x for x in held if x < 0)
            if long != short or escrow != PAYOUT_TICKS * long:
                msg = f"I2: escrow of market {market} is not 100 x open interest"
                raise EngineHaltedError(msg)
        # Every (account, market) that holds an order, a position, or a charged reserve.
        pairs = {(o.account, m) for m, book in self.books.items() for o in book}
        pairs |= {(a, m) for m, held in self.positions.items() for a, x in held.items() if x}
        pairs |= {pair for pair, r in self.reserve.items() if r}
        needed: dict[int, int] = {}
        for account, market in pairs:
            required = self._reserve_of(account, market)
            if required != self.reserve.get((account, market), 0):
                msg = f"I4: reserve of account {account} in market {market} is stale"
                raise EngineHaltedError(msg)
            needed[account] = needed.get(account, 0) + required
        for account, cash in self.cash.items():
            if needed.get(account, 0) != self._reserved_total(account) or cash < needed.get(
                account, 0
            ):
                msg = f"I3/I4: account {account} reserve total is wrong or exceeds its cash"
                raise EngineHaltedError(msg)

    def final_book(self) -> Event:
        """The end-of-input record: markets, accounts, and escrow (G1 and G2 plans)."""
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
        accounts = [
            {
                "account": account,
                "cash": self.cash[account],
                "reserved": self._reserved_total(account),
                "positions": sorted(
                    [m, held[account]]
                    for m, held in self.positions.items()
                    if held.get(account, 0) != 0
                ),
            }
            for account in sorted(self.cash)
        ]
        escrow = [[m, self.escrow[m]] for m in sorted(self.escrow)]
        return {"ev": "final_book", "markets": markets, "accounts": accounts, "escrow": escrow}
