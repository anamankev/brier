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
    def __init__(self) -> None:
        self.engine = ReferenceEngine()
        self.seq = 0

    def send(self, cmd: str, **fields: Any) -> list[dict[str, Any]]:  # noqa: ANN401
        self.seq += 1
        events = self.engine.apply({"seq": self.seq, "cmd": cmd, **fields})
        for e in events:
            assert e.pop("seq") == self.seq
        return events

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
    s.order(2, "sell", 55, 2)  # id 3
    s.order(3, "sell", 55, 2)  # id 4
    s.order(2, "sell", 54, 1)  # id 5, better price
    assert s.order(1, "buy", 60, 4) == [
        {"ev": "accepted", "order": 6},
        trade(5, 6, 2, 1, "buy", 54, 1),
        trade(3, 6, 2, 1, "buy", 55, 2),
        trade(4, 6, 3, 1, "buy", 55, 1),
    ]
    assert s.engine.final_book()["markets"][0]["asks"] == [[4, 3, 55, 1]]


def test_self_trade_prevention_keeps_earlier_trades_and_cancels_the_remainder() -> None:
    s = Session()
    s.open_market()
    s.order(2, "sell", 50, 1)  # id 3
    s.order(1, "sell", 50, 3)  # id 4, own
    assert s.order(1, "buy", 50, 4) == [
        {"ev": "accepted", "order": 5},
        trade(3, 5, 2, 1, "buy", 50, 1),
        {"ev": "cancelled", "order": 5, "remaining": 3, "reason": "self_trade"},
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
    s.order(1, "buy", 40, 5)  # id 3
    assert s.send("amend", account=2, order=3, qty=1) == [
        {"ev": "rejected", "reason": "unknown_order"}
    ]
    assert s.send("amend", account=1, order=3, qty=5) == [
        {"ev": "rejected", "reason": "invalid_amend"}
    ]
    assert s.send("amend", account=1, order=3, qty=2) == [
        {"ev": "amended", "order": 3, "remaining": 2}
    ]
    assert s.send("cancel", account=2, order=3) == [{"ev": "rejected", "reason": "unknown_order"}]
    assert s.send("cancel", account=1, order=3) == [
        {"ev": "cancelled", "order": 3, "remaining": 2, "reason": "user"}
    ]


def test_closing_cancels_in_ascending_id_after_the_state_event() -> None:
    s = Session()
    s.open_market()
    s.order(2, "sell", 70, 1)  # 3
    s.order(1, "buy", 20, 2)  # 4
    assert s.send("set_state", market=1, state="closed") == [
        {"ev": "market_state", "market": 1, "state": "closed"},
        {"ev": "cancelled", "order": 3, "remaining": 1, "reason": "market_closed"},
        {"ev": "cancelled", "order": 4, "remaining": 2, "reason": "market_closed"},
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
