"""The surveillance tape and detectors on small hand-built streams."""

from __future__ import annotations

from typing import Any

import pytest

import brier_services.surveillance.detectors as d
from brier_services.reference_engine import ReferenceEngine
from brier_services.surveillance.tape import Tape, build, replay


class Stream:
    """Commands through the reference engine, kept as the records surveillance reads."""

    def __init__(
        self, accounts: tuple[int, ...] = (1, 2, 3, 4), markets: tuple[int, ...] = (1,)
    ) -> None:
        self.engine = ReferenceEngine()
        self.seq = 0
        self.records: list[tuple[dict[str, Any], list[dict[str, Any]]]] = []
        for a in accounts:
            self.send(cmd="open_account", account=a)
        for m in markets:
            self.send(cmd="create_market", market=m)
            self.send(cmd="set_state", market=m, state="open")

    def send(self, **cmd: Any) -> list[dict[str, Any]]:  # noqa: ANN401 - wire fields
        self.seq += 1
        c = {"seq": self.seq, **cmd}
        events = self.engine.apply(c)
        self.records.append((c, events))
        return events

    def order(
        self, account: int, side: str, price: int, qty: int, tif: str = "gtc", market: int = 1
    ) -> int:
        events = self.send(
            cmd="place",
            account=account,
            market=market,
            side=side,
            price=price,
            qty=qty,
            tif=tif,
            post_only=False,
        )
        return int(events[0]["order"])

    def cancel(self, account: int, order: int) -> None:
        self.send(cmd="cancel", account=account, order=order)

    def idle(self, n: int, market: int = 1) -> None:
        """n market commands that change nothing: rest and cancel a 1-lot far from the touch."""
        for _ in range(n // 2):
            self.cancel(4, self.order(4, "buy", 1, 1, market=market))

    def finish(self, outcome: str = "yes", market: int = 1) -> Tape:
        self.send(cmd="set_state", market=market, state="closed")
        self.send(cmd="settle", market=market, outcome=outcome)
        return build(self.records)


def by_account(alerts: list[d.Alert], typology: str) -> dict[int, float]:
    return {a.account: a.score for a in alerts if a.typology == typology}


def test_tape_rebuilds_orders_fills_and_the_book_before_each_order() -> None:
    s = Stream(markets=(1, 2))
    s.order(1, "sell", 60, 5)
    s.order(1, "buy", 40, 5, market=2)  # market 2's clock does not move market 1's
    taker = s.order(2, "buy", 60, 3, tif="ioc")
    tape = s.finish()
    m = tape.markets[1]
    o = m.orders[taker]
    # Market 1's clock: listed, opened, the resting sell, then this order.
    assert (o.best_bid, o.best_ask, o.filled, o.clock) == (None, 60, 3, 4)
    f = m.fills[0]
    assert (f.buyer, f.seller, f.taker, f.price, f.qty) == (2, 1, 2, 60, 3)
    assert m.close_clock == 5
    assert m.outcome == "yes"
    assert tape.markets[2].close_clock is None
    assert tape.account_opened == {1: 1, 2: 2, 3: 3, 4: 4}


def test_replaying_the_commands_gives_the_same_records() -> None:
    s = Stream()
    s.order(1, "sell", 55, 4)
    s.order(2, "buy", 55, 4, tif="ioc")
    s.finish()
    assert list(replay(c for c, _ in s.records)) == s.records


def test_wash_scores_the_share_of_volume_traded_with_linked_accounts() -> None:
    s = Stream()
    s.order(1, "sell", 50, 30)
    s.order(2, "buy", 50, 30, tif="ioc")  # 1 and 2 are linked
    s.order(3, "sell", 50, 10)
    s.order(2, "buy", 50, 10, tif="ioc")  # 3 is not
    tape = s.finish()
    links = {1: {2}, 2: {1}}
    assert by_account(d.wash(tape, links), "wash") == {1: 1.0, 2: 0.75}
    assert d.wash(tape, {}) == []


def spoof_episode(s: Stream, *, trade_against: bool, distance: int = 1) -> None:
    for _ in range(4):
        s.order(3, "buy", 48, 25)  # the usual size: median 25, so large means 100 or more
    s.order(3, "sell", 52, 25)
    spoof = s.order(1, "buy", 48 - distance, 150)
    if trade_against:
        s.order(1, "sell", 48, 10, tif="ioc")
    s.cancel(1, spoof)


@pytest.mark.parametrize(
    ("against", "distance", "score"),
    [(True, 1, 1.0), (False, 1, d.SPOOF_UNPAIRED_WEIGHT), (True, 3, None)],
)
def test_spoof_weighs_large_cancelled_orders_by_trades_the_other_way(
    against: bool,  # noqa: FBT001 - parametrized
    distance: int,
    score: float | None,
) -> None:
    s = Stream()
    spoof_episode(s, trade_against=against, distance=distance)
    found = by_account(d.spoof(s.finish(), {}), "spoof")
    assert found.get(1) == score


def test_spoof_ignores_large_orders_that_traded_or_were_cancelled_late() -> None:
    s = Stream()
    for _ in range(4):
        s.order(3, "buy", 48, 25)
    big = s.order(1, "buy", 49, 150)
    s.order(2, "sell", 49, 1, tif="ioc")  # it traded: not a spoof
    s.cancel(1, big)
    late = s.order(1, "buy", 47, 150)
    s.idle(d.SPOOF_WINDOW + 2)
    s.cancel(1, late)
    assert d.spoof(s.finish(), {}) == []


def test_close_scores_taker_share_times_the_move_in_the_final_window() -> None:
    s = Stream()
    for p in (50, 52, 54, 56):
        s.order(3, "sell", p, 10)
    s.order(2, "buy", 50, 5, tif="ioc")  # sets the last price before the window
    s.idle(d.CLOSE_WINDOW + 2)
    s.order(1, "buy", 54, 25, tif="ioc")  # 50 -> 54 in the window
    s.idle(4)
    found = by_account(d.close(s.finish(), {}), "close")
    assert found == {1: pytest.approx(4.0)}  # all window taker volume, 4 ticks


def test_close_ignores_the_same_trades_before_the_window() -> None:
    s = Stream()
    for p in (50, 52, 54, 56):
        s.order(3, "sell", p, 10)
    s.order(2, "buy", 50, 5, tif="ioc")
    s.order(1, "buy", 54, 25, tif="ioc")
    s.idle(d.CLOSE_WINDOW + 2)
    assert d.close(s.finish(), {}) == []


def insider_stream(outcome: str) -> Tape:
    s = Stream(accounts=(1, 2, 3, 4))
    s.order(3, "sell", 60, 200)
    s.order(2, "buy", 60, 10, tif="ioc")  # account 2 trades early, account 5 does not exist yet
    s.idle(10)
    s.send(cmd="open_account", account=5)
    s.idle(d.INSIDER_WINDOW)
    s.order(5, "buy", 63, 100, tif="ioc")
    s.order(2, "buy", 60, 10, tif="ioc")
    s.idle(4)
    return s.finish(outcome)


def test_insider_scores_new_concentrated_positions_on_the_winning_side() -> None:
    found = by_account(d.insider(insider_stream("yes"), {}), "insider")
    # Account 5: D = 100, E = 0, opened after the market: 100 * 1 * 2.
    # Account 2: D = 10, E = 10: 10 * 0.5 * 1.
    assert found == {5: pytest.approx(200.0), 2: pytest.approx(5.0)}
    linked = by_account(d.insider(insider_stream("yes"), {5: {9}}), "insider")
    assert linked[5] == pytest.approx(300.0)


def test_insider_needs_a_position_on_the_side_that_won() -> None:
    # NO won: the buyers lost, and the seller (account 3) built the winning position.
    assert set(by_account(d.insider(insider_stream("no"), {}), "insider")) == {3}
    assert d.insider(insider_stream("void"), {}) == []


def test_alerts_apply_each_typology_threshold() -> None:
    found = [d.Alert("wash", 1, 1, 0.5), d.Alert("wash", 1, 2, 0.2), d.Alert("close", 1, 1, 3.0)]
    assert d.alerts(found, {"wash": 0.5, "close": 4.0}) == [found[0]]


def test_taker_only_pairing_ignores_passive_fills_on_the_other_side(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    monkeypatch.setattr(d, "SPOOF_UNPAIRED_WEIGHT", 0.0)
    monkeypatch.setattr(d, "SPOOF_AGAINST_AS_TAKER_ONLY", True)
    s = Stream()
    for _ in range(4):
        s.order(3, "buy", 48, 25)
    s.order(1, "sell", 52, 10)  # account 1's resting sell...
    big = s.order(1, "buy", 47, 150)
    s.order(2, "buy", 52, 10, tif="ioc")  # ...is hit: a passive fill on the other side
    s.cancel(1, big)
    assert d.spoof(s.finish(), {}) == []
    s = Stream()
    spoof_episode(s, trade_against=True)  # it sells as the taker
    assert by_account(d.spoof(s.finish(), {}), "spoof") == {1: 1.0}
