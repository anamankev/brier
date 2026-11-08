"""One simulated market, run on its own copy of the reference engine.

The engine is the Python reference engine, held to the Rust engine event for event by the
differential tests, so a simulated market behaves exactly as the exchange would. Each market
gets a fresh engine; the same seed always produces the same orders and the same result.

Agents (docs/notes/market-maker.md §3):
- a hidden true probability: a random walk in log-odds; the outcome is drawn from its end;
- noise traders: arrive with probability 0.5 a step, trade 1-20 contracts at the best opposite
  price, immediate-or-cancel;
- informed traders (optional): arrive with probability 0.15, see the true log-odds plus noise of
  standard deviation ``signal_sd``, and trade when their estimate beats the quote by 0.02;
- the market maker: the quoting code the live service uses, with fair value equal to the true
  probability 30 steps ago (public information that lags), requoting every step.
"""

from __future__ import annotations

import math
import random
from dataclasses import dataclass, field
from typing import Any

from brier_services.marketmaker.quoting import Params, quote
from brier_services.reference_engine import ReferenceEngine

MARKET = 1
MAKER = 1
NOISE = 2
INFORMED = 3
STEPS = 480
LAG = 30
CLOSE_BUFFER = 15


def logistic(x: float) -> float:
    return 1.0 / (1.0 + math.exp(-x))


@dataclass
class Exchange:
    """The reference engine with a sequence counter and the views agents need."""

    engine: ReferenceEngine = field(default_factory=ReferenceEngine)
    seq: int = 0
    commands: int = 0

    def send(self, **cmd: Any) -> list[dict[str, Any]]:  # noqa: ANN401 - wire fields
        self.seq += 1
        self.commands += 1
        return self.engine.apply({"seq": self.seq, **cmd})

    def best(self) -> tuple[int | None, int | None]:
        book = self.engine.books.get(MARKET, [])
        bids = [o.price for o in book if o.side == "buy"]
        asks = [o.price for o in book if o.side == "sell"]
        return (max(bids, default=None), min(asks, default=None))

    def position(self, account: int) -> int:
        return self.engine.positions.get(MARKET, {}).get(account, 0)

    def resting(self, account: int) -> list[int]:
        return [o.id for o in self.engine.books.get(MARKET, []) if o.account == account]


@dataclass(frozen=True)
class Result:
    seed: int
    outcome: str
    maker_pnl: int  # credits (cents): cash after settlement minus cash before
    max_inventory: int
    steps_quoted: int
    quoted_spread_sum: int
    commands: int
    inventory_path: tuple[int, ...] = ()


def run(
    seed: int,
    signal_sd: float | None = None,
    params: Params = Params(),  # noqa: B008 - frozen default
    record_path: bool = False,  # noqa: FBT001, FBT002 - a plain option
) -> Result:
    """Simulate one market for ``STEPS`` steps and settle it."""
    truth_rng = random.Random(f"truth-{seed}")  # noqa: S311 - simulation, not cryptography
    noise_rng = random.Random(f"noise-{seed}")  # noqa: S311
    informed_rng = random.Random(f"informed-{seed}")  # noqa: S311
    ex = Exchange()
    for account in (MAKER, NOISE, INFORMED):
        ex.send(cmd="open_account", account=account)
    ex.send(cmd="create_market", market=MARKET)
    ex.send(cmd="set_state", market=MARKET, state="open")
    start_cash = ex.engine.cash[MAKER]

    logit = truth_rng.gauss(0.0, 1.0)
    path = [logistic(logit)]
    max_inventory = steps_quoted = spread_sum = 0
    inventory_path: list[int] = []
    for step in range(STEPS):
        # The maker requotes: cancel what rests, then quote around the lagged fair value.
        for order in ex.resting(MAKER):
            ex.send(cmd="cancel", account=MAKER, order=order)
        if step < STEPS - CLOSE_BUFFER:
            fair = min(0.99, max(0.01, path[max(0, step - LAG)]))
            inventory = ex.position(MAKER)
            q = quote(fair, inventory, (STEPS - step) / STEPS, params)
            if q.bid is not None:
                ex.send(
                    cmd="place",
                    account=MAKER,
                    market=MARKET,
                    side="buy",
                    price=q.bid,
                    qty=q.size,
                    tif="gtc",
                    post_only=False,
                )
            if q.ask is not None:
                ex.send(
                    cmd="place",
                    account=MAKER,
                    market=MARKET,
                    side="sell",
                    price=q.ask,
                    qty=q.size,
                    tif="gtc",
                    post_only=False,
                )
            if q.bid is not None and q.ask is not None:
                steps_quoted += 1
                spread_sum += q.ask - q.bid
        bid, ask = ex.best()
        if noise_rng.random() < 0.5:
            side = "buy" if noise_rng.random() < 0.5 else "sell"
            qty = noise_rng.randint(1, 20)
            price = ask if side == "buy" else bid
            if price is not None:
                ex.send(
                    cmd="place",
                    account=NOISE,
                    market=MARKET,
                    side=side,
                    price=price,
                    qty=qty,
                    tif="ioc",
                    post_only=False,
                )
        if signal_sd is not None and informed_rng.random() < 0.15:
            estimate = logistic(logit + informed_rng.gauss(0.0, signal_sd))
            qty = informed_rng.randint(1, 20)
            bid, ask = ex.best()
            if ask is not None and estimate > ask / 100 + 0.02:
                ex.send(
                    cmd="place",
                    account=INFORMED,
                    market=MARKET,
                    side="buy",
                    price=ask,
                    qty=qty,
                    tif="ioc",
                    post_only=False,
                )
            elif bid is not None and estimate < bid / 100 - 0.02:
                ex.send(
                    cmd="place",
                    account=INFORMED,
                    market=MARKET,
                    side="sell",
                    price=bid,
                    qty=qty,
                    tif="ioc",
                    post_only=False,
                )
        max_inventory = max(max_inventory, abs(ex.position(MAKER)))
        if record_path:
            inventory_path.append(ex.position(MAKER))
        logit += truth_rng.gauss(0.0, 0.08)
        path.append(logistic(logit))

    outcome = "yes" if truth_rng.random() < path[-1] else "no"
    ex.send(cmd="set_state", market=MARKET, state="closed")
    ex.send(cmd="settle", market=MARKET, outcome=outcome)
    return Result(
        seed=seed,
        outcome=outcome,
        maker_pnl=ex.engine.cash[MAKER] - start_cash,
        max_inventory=max_inventory,
        steps_quoted=steps_quoted,
        quoted_spread_sum=spread_sum,
        commands=ex.commands,
        inventory_path=tuple(inventory_path),
    )
