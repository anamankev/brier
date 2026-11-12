"""The market-maker service against a fake exchange and fake data."""

from __future__ import annotations

import datetime as dt
import json
from typing import Any

from brier_services.marketmaker.service import FairValues, MarketMaker
from brier_services.resolution.fetch import Request, Response
from brier_services.rules import instant

RULE = {
    "market": 104,
    "source": {"provider": "fred", "series": "DGS10"},
    "condition": {"transform": "level", "op": ">", "threshold": "4.75"},
    "observation": "2026-12-31",
    "schedule": {"open": "2026-11-09T14:30:00Z", "close": "2026-12-31T00:00:00Z"},
}
FOMC = {**RULE, "market": 103, "source": {"provider": "fred", "series": "DFEDTARU"}}


class FakeApi:
    def __init__(self) -> None:
        self.orders: dict[int, dict[str, Any]] = {}
        self.next = 100
        self.calls: list[tuple[str, str]] = []
        self.state = "open"

    def request(self, method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401
        self.calls.append((method, path))
        if path == "/v1/markets":
            return {
                "markets": [
                    {"market": 104, "state": self.state, "title": "10y"},
                    {"market": 103, "state": "open", "title": "Fed"},
                ]
            }
        if path.endswith("/rules"):
            return {"rulebook": RULE if "/104/" in path else FOMC}
        if path == "/v1/allowance":
            if method == "POST":
                self.claimed = True
                return {"granted": 10_000}
            return {"claimed_today": getattr(self, "claimed", False)}
        if path == "/v1/portfolio":
            return {"positions": [], "open_orders": list(self.orders.values())}
        if method == "POST" and path == "/v1/orders":
            assert body is not None
            self.next += 1
            self.orders[self.next] = {
                "order": self.next,
                "market": body["market"],
                "side": body["side"],
                "price": body["price"],
                "remaining": body["qty"],
            }
            return {"status": "resting", "order": self.next}
        if method == "DELETE":
            self.orders.pop(int(path.rsplit("/", 1)[1]))
            return {"status": "cancelled"}
        raise AssertionError(path)


def fred_history() -> bytes:
    start = dt.date(2025, 6, 1)
    obs = [
        {
            "date": (start + dt.timedelta(days=i)).isoformat(),
            "value": f"{4.5 + 0.03 * ((i * 7) % 5 - 2):.2f}",
        }
        for i in range(400)
    ]
    return json.dumps({"observations": obs}).encode()


def fetcher(request: Request) -> Response:
    assert "api.stlouisfed.org" in request.url
    return Response(request.url, 200, fred_history(), "2026-11-10T00:00:00Z")


def maker(api: FakeApi, now: str = "2026-11-10T00:00:00Z") -> MarketMaker:
    clock = instant(now) or 0
    return MarketMaker(
        api, FairValues("k", fetcher, wall=lambda: 1.0), now=lambda: clock, say=lambda *a, **k: None
    )


def test_it_quotes_both_sides_and_leaves_unmodelled_markets_alone() -> None:
    api = FakeApi()
    decided = maker(api).run_once()
    assert decided[103]["pulled"] == "no model"
    sides = sorted(o["side"] for o in api.orders.values())
    assert sides == ["buy", "sell"]
    assert all(o["market"] == 104 for o in api.orders.values())
    bid = next(o["price"] for o in api.orders.values() if o["side"] == "buy")
    ask = next(o["price"] for o in api.orders.values() if o["side"] == "sell")
    assert bid < ask


def test_a_second_pass_keeps_good_quotes_instead_of_churning() -> None:
    api = FakeApi()
    m = maker(api)
    m.run_once()
    before = dict(api.orders)
    posts = sum(1 for c in api.calls if c[0] == "POST")
    m.run_once()
    assert api.orders == before
    assert sum(1 for c in api.calls if c[0] == "POST") == posts


def test_quotes_are_pulled_near_the_close_and_when_the_market_halts() -> None:
    api = FakeApi()
    maker(api).run_once()
    assert api.orders
    late = maker(api, "2026-12-30T23:50:00Z")
    assert late.run_once()[104]["pulled"] == "close is near"
    assert not api.orders
    api2 = FakeApi()
    maker(api2).run_once()
    api2.state = "halted"
    assert maker(api2).run_once()[104]["pulled"] == "market halted"
    assert not api2.orders


def test_stale_inputs_pull_quotes() -> None:
    api = FakeApi()

    def broken(request: Request) -> Response:
        return Response(request.url, 503, b"", "2026-11-10T00:00:00Z")

    clock = instant("2026-11-10T00:00:00Z") or 0
    m = MarketMaker(
        api, FairValues("k", broken, wall=lambda: 1.0), now=lambda: clock, say=lambda *a, **k: None
    )
    assert "inputs unavailable" in str(m.run_once()[104]["pulled"])
    assert not api.orders


def test_quote_size_shrinks_to_fit_cash() -> None:
    api = FakeApi()
    real = api.request

    def poor(method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401
        out = real(method, path, body)
        return {**out, "cash": 1_000} if path == "/v1/portfolio" else out

    api.request = poor  # type: ignore[method-assign]
    decided = maker(api).run_once()
    assert decided[104]["size"] == 10, "1,000 credit-cents fit one two-sided quote of 10"
    assert {o["remaining"] for o in api.orders.values()} == {10}


def test_a_refused_quote_skips_that_side_and_the_pass_goes_on() -> None:
    from brier_services.client import ApiError

    api = FakeApi()
    real = api.request
    said: list[str] = []

    def refuse_buys(method: str, path: str, body: dict[str, Any] | None = None) -> Any:  # noqa: ANN401
        if method == "POST" and body is not None and body["side"] == "buy":
            raise ApiError(422, {"error": {"code": "insufficient_collateral"}})
        return real(method, path, body)

    api.request = refuse_buys  # type: ignore[method-assign]
    m = maker(api)
    m.say = lambda event, **_: said.append(event)
    m.run_once()
    assert "quote_refused" in said
    assert [o["side"] for o in api.orders.values()] == ["sell"]


def test_the_maker_claims_the_daily_allowance_once() -> None:
    api = FakeApi()
    m = maker(api)
    m.run_once()
    m.run_once()
    assert api.calls.count(("POST", "/v1/allowance")) == 1
