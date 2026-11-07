"""Tests for the reference engine, from the rules in docs/gates/G1-plan.md."""

from __future__ import annotations

from typing import Any

import pytest

from brier_services.reference_engine import (
    EngineHaltedError,
    MalformedCommandError,
    ReferenceEngine,
)


class Session:
    """Accounts 1-3 opened. `send` returns the matching events (ledger `entry` and `reserved`
    events are checked separately by the money tests, through `send_all`)."""

    def __init__(self) -> None:
        self.engine = ReferenceEngine()
        self.seq = 0
        for account in (1, 2, 3):
            self.send("open_account", account=account)

    def send_all(self, cmd: str, **fields: Any) -> list[dict[str, Any]]:  # noqa: ANN401
        self.seq += 1
        events = self.engine.apply({"seq": self.seq, "cmd": cmd, **fields})
        self.engine.check_invariants()
        for e in events:
            assert e.pop("seq") == self.seq
        return events

    def send(self, cmd: str, **fields: Any) -> list[dict[str, Any]]:  # noqa: ANN401
        return [e for e in self.send_all(cmd, **fields) if e["ev"] not in {"entry", "reserved"}]

    def open_market(self, market: int = 1) -> None:
        self.send("create_market", market=market)
        self.send("set_state", market=market, state="open")

    def order(
        self,
        account: int,
        side: str,
        price: int,
        qty: int,
        tif: str = "gtc",
        *,
        post_only: bool = False,
    ) -> list[dict[str, Any]]:
        return self.send(
            "place",
            account=account,
            market=1,
            side=side,
            price=price,
            qty=qty,
            tif=tif,
            post_only=post_only,
        )


def trade(
    maker: int, taker: int, ma: int, ta: int, side: str, price: int, qty: int
) -> dict[str, Any]:
    return {
        "ev": "trade",
        "market": 1,
        "maker": maker,
        "taker": taker,
        "maker_account": ma,
        "taker_account": ta,
        "taker_side": side,
        "price": price,
        "qty": qty,
    }


def test_price_then_time_priority_at_the_makers_price() -> None:
    s = Session()
    s.open_market()
    # Order IDs are sequence numbers; commands 1-3 opened the accounts, 4-5 the market.
    s.order(2, "sell", 55, 2)  # id 6
    s.order(3, "sell", 55, 2)  # id 7
    s.order(2, "sell", 54, 1)  # id 8, better price
    assert s.order(1, "buy", 60, 4) == [
        {"ev": "accepted", "order": 9},
        trade(8, 9, 2, 1, "buy", 54, 1),
        trade(6, 9, 2, 1, "buy", 55, 2),
        trade(7, 9, 3, 1, "buy", 55, 1),
    ]
    assert s.engine.final_book()["markets"][0]["asks"] == [[7, 3, 55, 1]]


def test_self_trade_prevention_keeps_earlier_trades_and_cancels_the_remainder() -> None:
    s = Session()
    s.open_market()
    s.order(2, "sell", 50, 1)  # id 6
    s.order(1, "sell", 50, 3)  # id 7, own
    assert s.order(1, "buy", 50, 4) == [
        {"ev": "accepted", "order": 8},
        trade(6, 8, 2, 1, "buy", 50, 1),
        {"ev": "cancelled", "order": 8, "remaining": 3, "reason": "self_trade"},
    ]


def test_fok_stops_counting_at_the_accounts_own_order() -> None:
    s = Session()
    s.open_market()
    s.order(2, "sell", 50, 1)
    s.order(1, "sell", 50, 5)
    s.order(3, "sell", 50, 5)
    assert s.order(1, "buy", 50, 2, "fok") == [{"ev": "rejected", "reason": "fok_unfillable"}]


def test_rejection_order_for_place() -> None:
    s = Session()
    s.send("create_market", market=1)
    assert s.order(1, "buy", 0, 0) == [{"ev": "rejected", "reason": "market_not_open"}]
    s.send("set_state", market=1, state="open")
    assert s.order(1, "buy", 0, 0) == [{"ev": "rejected", "reason": "invalid_price"}]
    assert s.order(1, "buy", 50, 0) == [{"ev": "rejected", "reason": "invalid_quantity"}]
    assert s.order(1, "buy", 50, 1, "ioc", post_only=True) == [
        {"ev": "rejected", "reason": "invalid_order"}
    ]


def test_amend_and_cancel_only_reach_the_owner() -> None:
    s = Session()
    s.open_market()
    s.order(1, "buy", 40, 5)  # id 6
    assert s.send("amend", account=2, order=6, qty=1) == [
        {"ev": "rejected", "reason": "unknown_order"}
    ]
    assert s.send("amend", account=1, order=6, qty=5) == [
        {"ev": "rejected", "reason": "invalid_amend"}
    ]
    assert s.send("amend", account=1, order=6, qty=2) == [
        {"ev": "amended", "order": 6, "remaining": 2}
    ]
    assert s.send("cancel", account=2, order=6) == [{"ev": "rejected", "reason": "unknown_order"}]
    assert s.send("cancel", account=1, order=6) == [
        {"ev": "cancelled", "order": 6, "remaining": 2, "reason": "user"}
    ]


def test_closing_cancels_in_ascending_id_after_the_state_event() -> None:
    s = Session()
    s.open_market()
    s.order(2, "sell", 70, 1)  # 6
    s.order(1, "buy", 20, 2)  # 7
    assert s.send("set_state", market=1, state="closed") == [
        {"ev": "market_state", "market": 1, "state": "closed"},
        {"ev": "cancelled", "order": 6, "remaining": 1, "reason": "market_closed"},
        {"ev": "cancelled", "order": 7, "remaining": 2, "reason": "market_closed"},
    ]
    assert s.send("set_state", market=1, state="open") == [
        {"ev": "rejected", "reason": "invalid_transition"}
    ]


def test_sequence_gaps_halt_and_malformed_commands_are_errors() -> None:
    engine = ReferenceEngine()
    with pytest.raises(EngineHaltedError):
        engine.apply({"seq": 2, "cmd": "create_market", "market": 1})
    with pytest.raises(MalformedCommandError):
        engine.apply({"seq": 1, "cmd": "nope"})
    engine.apply({"seq": 1, "cmd": "create_market", "market": 1})
    with pytest.raises(MalformedCommandError):
        engine.apply(
            {
                "seq": 2,
                "cmd": "place",
                "account": 1,
                "market": 1,
                "side": "buy",
                "price": 300,
                "qty": 1,
                "tif": "gtc",
                "post_only": False,
            }
        )


def test_trade_cash_follows_the_g2_formulas() -> None:
    s = Session()
    s.open_market()
    # Account 2 sells YES it does not hold: opening NO costs 100 - 40 = 60 per unit.
    assert s.send_all(
        "place", account=2, market=1, side="sell", price=40, qty=10, tif="gtc", post_only=False
    )[-1] == {"ev": "reserved", "account": 2, "market": 1, "amount": 600}
    events = s.send_all(
        "place", account=1, market=1, side="buy", price=40, qty=10, tif="gtc", post_only=False
    )
    assert {
        "ev": "entry",
        "kind": "trade",
        "legs": [["cash:1", -400], ["cash:2", -600], ["escrow:1", 1000]],
    } in events
    # Account 1 now sells 6 of its YES to account 3: a transfer, escrow unchanged.
    s.send_all(
        "place", account=1, market=1, side="sell", price=45, qty=6, tif="gtc", post_only=False
    )
    events = s.send_all(
        "place", account=3, market=1, side="buy", price=45, qty=6, tif="gtc", post_only=False
    )
    assert {"ev": "entry", "kind": "trade", "legs": [["cash:3", -270], ["cash:1", 270]]} in events


def test_settlement_pays_winners_and_void_pays_50_each() -> None:
    for outcome, expected in (
        ("yes", [["cash:1", 1000]]),
        ("void", [["cash:1", 500], ["cash:2", 500]]),
    ):
        s = Session()
        s.open_market()
        s.send_all(
            "place", account=2, market=1, side="sell", price=40, qty=10, tif="gtc", post_only=False
        )
        s.send_all(
            "place", account=1, market=1, side="buy", price=40, qty=10, tif="gtc", post_only=False
        )
        s.send_all("set_state", market=1, state="closed")
        events = s.send_all("settle", market=1, outcome=outcome)
        assert events[0] == {"ev": "settled", "market": 1, "outcome": outcome}
        payouts = [e["legs"][1] for e in events if e["ev"] == "entry"]
        assert payouts == expected
        assert s.engine.final_book()["escrow"] == [[1, 0]]


def test_collateral_is_checked_against_available_cash() -> None:
    s = Session()
    s.open_market()
    # 20,000 x 50 = 1,000,000: exactly the grant.
    assert (
        s.send(
            "place",
            account=1,
            market=1,
            side="buy",
            price=50,
            qty=20_000,
            tif="gtc",
            post_only=False,
        )[0]["ev"]
        == "accepted"
    )
    assert s.send(
        "place", account=1, market=1, side="buy", price=1, qty=1, tif="ioc", post_only=False
    ) == [{"ev": "rejected", "reason": "insufficient_collateral"}]
