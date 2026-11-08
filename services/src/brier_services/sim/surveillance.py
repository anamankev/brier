"""Simulated markets with manipulators injected as ground truth (docs/notes/surveillance.md).

Each market runs on its own reference engine, with the market-maker study's hidden truth and
maker (quoting until the close, §3a), ordinary traders, innocent accounts built to look
suspicious, and each of four manipulators present with probability 0.5. The same seed always
gives the same commands, events, and labels.
"""

from __future__ import annotations

import random
from dataclasses import dataclass, field
from typing import Any

from brier_services.marketmaker.quoting import Params, quote
from brier_services.sim.market import LAG, MARKET, STEPS, Exchange, logistic
from brier_services.surveillance.tape import Record

MAKER = 1
NOISE = tuple(range(10, 20))
INFORMED = (20, 21, 22)
LINKED_NOISE = (30, 31)
LARGE_PASSIVE = 32
LATE_INFORMED = 33
WASH = (40, 41)
SPOOFER = 42
CLOSER = 43
INSIDER = 44
USUAL = 25  # the maker's quote size: the usual resting size

KIND = {
    MAKER: "maker",
    **dict.fromkeys(NOISE, "noise"),
    **dict.fromkeys(INFORMED, "informed"),
    **dict.fromkeys(LINKED_NOISE, "linked_noise"),
    LARGE_PASSIVE: "large_passive",
    LATE_INFORMED: "late_informed",
    **dict.fromkeys(WASH, "wash"),
    SPOOFER: "spoofer",
    CLOSER: "closer",
    INSIDER: "insider",
}
TYPOLOGY_OF_KIND = {"wash": "wash", "spoofer": "spoof", "closer": "close", "insider": "insider"}


@dataclass
class Recorder(Exchange):
    """The simulation exchange, keeping every command with its events."""

    records: list[Record] = field(default_factory=list)
    last: int | None = None  # last trade price

    def send(self, **cmd: Any) -> list[dict[str, Any]]:  # noqa: ANN401 - wire fields
        self.seq += 1
        self.commands += 1
        c = {"seq": self.seq, **cmd}
        events = self.engine.apply(c)
        self.records.append((c, events))
        for e in events:
            if e["ev"] == "trade":
                self.last = int(e["price"])
        return events

    def place(
        self, account: int, side: str, price: int | None, qty: int, tif: str
    ) -> list[dict[str, Any]]:
        if price is None or not 1 <= price <= 99 or qty <= 0:
            return []
        return self.send(
            cmd="place",
            account=account,
            market=MARKET,
            side=side,
            price=price,
            qty=qty,
            tif=tif,
            post_only=False,
        )

    def rest(self, account: int, side: str, price: int | None, qty: int) -> int | None:
        """A GTC order; its ID if it rests."""
        events = self.place(account, side, price, qty, "gtc")
        if any(e["ev"] == "rested" for e in events):
            return int(events[0]["order"])
        return None

    def cancel_if_resting(self, account: int, order: int) -> None:
        if order in self.resting(account):
            self.send(cmd="cancel", account=account, order=order)


@dataclass(frozen=True)
class Simulated:
    seed: int
    records: list[Record]
    links: dict[int, set[int]]
    truth: frozenset[tuple[str, int]]  # (typology, account) pairs that are manipulation
    kind: dict[int, str]
    outcome: str


def _filled(events: list[dict[str, Any]], account: int) -> int:
    return sum(
        int(e["qty"]) for e in events if e["ev"] == "trade" and e["taker_account"] == account
    )


def run(seed: int, params: Params = Params()) -> Simulated:  # noqa: B008 - frozen default
    def rng(name: str) -> random.Random:
        return random.Random(f"surveillance-{name}-{seed}")  # noqa: S311 - simulation

    presence, truth_rng, outcome_rng = rng("presence"), rng("truth"), rng("outcome")
    present = {k: presence.random() < 0.5 for k in ("wash", "spoofer", "closer", "insider")}
    noise_rng, informed_rng, late_rng = rng("noise"), rng("informed"), rng("late")
    linked_rng, large_rng, wash_rng = rng("linked"), rng("large"), rng("wash")
    spoof_rng, close_rng, insider_rng = rng("spoof"), rng("close"), rng("insider")

    # The hidden truth first: it does not depend on trading.
    logits = [truth_rng.gauss(0.0, 1.0)]
    for _ in range(STEPS):
        logits.append(logits[-1] + truth_rng.gauss(0.0, 0.08))
    outcome = "yes" if outcome_rng.random() < logistic(logits[-1]) else "no"

    wash_steps = sorted(wash_rng.sample(range(STEPS - 30), wash_rng.randint(3, 8)))
    spoof_starts = sorted(spoof_rng.sample(range(20, STEPS - 40, 4), spoof_rng.randint(3, 6)))
    closer_early = close_rng.randint(20, 100)
    insider_open = insider_rng.randint(400, 449)
    insider_target = insider_rng.randint(100, 300)

    ex = Recorder()
    accounts = [MAKER, *NOISE, *INFORMED, *LINKED_NOISE, LARGE_PASSIVE, LATE_INFORMED]
    accounts += [*WASH] * present["wash"] + [SPOOFER] * present["spoofer"]
    accounts += [CLOSER] * present["closer"]
    for a in accounts:
        ex.send(cmd="open_account", account=a)
    ex.send(cmd="create_market", market=MARKET)
    ex.send(cmd="set_state", market=MARKET, state="open")

    def take(account: int, r: random.Random) -> None:
        side = "buy" if r.random() < 0.5 else "sell"
        bid, ask = ex.best()
        ex.place(account, side, ask if side == "buy" else bid, r.randint(1, 20), "ioc")

    def informed(account: int, sd: float, step: int, r: random.Random) -> None:
        estimate = logistic(logits[step] + r.gauss(0.0, sd))
        qty = r.randint(1, 20)
        bid, ask = ex.best()
        if ask is not None and estimate > ask / 100 + 0.02:
            ex.place(account, "buy", ask, qty, "ioc")
        elif bid is not None and estimate < bid / 100 - 0.02:
            ex.place(account, "sell", bid, qty, "ioc")

    def behind_touch(side: str, ticks: int) -> int | None:
        bid, ask = ex.best()
        if side == "buy":
            return None if bid is None else bid - ticks
        return None if ask is None else ask + ticks

    expiring: list[tuple[int, int, int]] = []  # (step, account, order)
    pending_wash = list(wash_steps)
    spoof_order: int | None = None
    spoof_side = "buy"
    insider_got = 0
    for step in range(STEPS):
        final10, final30 = step >= STEPS - 10, step >= STEPS - 30
        for mine in ex.resting(MAKER):
            ex.send(cmd="cancel", account=MAKER, order=mine)
        fair = min(0.99, max(0.01, logistic(logits[max(0, step - LAG)])))
        q = quote(fair, ex.position(MAKER), (STEPS - step) / STEPS, params)
        ex.place(MAKER, "buy", q.bid, q.size, "gtc")
        ex.place(MAKER, "sell", q.ask, q.size, "gtc")
        if present["insider"] and step == insider_open:
            ex.send(cmd="open_account", account=INSIDER)

        # Ordinary traders.
        if noise_rng.random() < (1.0 if final10 else 0.5):
            take(noise_rng.choice(NOISE), noise_rng)
        if informed_rng.random() < 0.15:
            informed(informed_rng.choice(INFORMED), 1.0, step, informed_rng)

        # Innocent look-alikes.
        if late_rng.random() < (0.6 if final30 else 0.15):
            informed(LATE_INFORMED, 0.25, step, late_rng)
        for a in LINKED_NOISE:
            if linked_rng.random() < 0.05:
                if linked_rng.random() < 0.5:
                    side = "buy" if linked_rng.random() < 0.5 else "sell"
                    order = ex.rest(a, side, behind_touch(side, 0), linked_rng.randint(1, 20))
                    if order is not None:
                        expiring.append((step + 5, a, order))
                else:
                    take(a, linked_rng)
        if large_rng.random() < 0.1:
            side = "buy" if large_rng.random() < 0.5 else "sell"
            price = behind_touch(side, large_rng.randint(1, 2))
            order = ex.rest(LARGE_PASSIVE, side, price, USUAL * large_rng.randint(4, 8))
            if order is not None:
                expiring.append((step + large_rng.randint(1, 3), LARGE_PASSIVE, order))
        for due in [e for e in expiring if e[0] <= step]:
            expiring.remove(due)
            ex.cancel_if_resting(due[1], due[2])

        # Manipulators.
        if present["wash"]:
            for a in WASH:
                if wash_rng.random() < 0.05:
                    take(a, wash_rng)
            if pending_wash and pending_wash[0] <= step:
                bid, ask = ex.best()
                if bid is not None and ask is not None and ask - bid >= 2:
                    last = ex.last if ex.last is not None else 50
                    price = min(range(bid + 1, ask), key=lambda p: (abs(p - last), p))
                    resting, taking = WASH if wash_rng.random() < 0.5 else WASH[::-1]
                    side = "buy" if wash_rng.random() < 0.5 else "sell"
                    qty = wash_rng.randint(20, 100)
                    if ex.rest(resting, side, price, qty) is not None:
                        ex.place(taking, "sell" if side == "buy" else "buy", price, qty, "ioc")
                    pending_wash.pop(0)
        if present["spoofer"]:
            if step in spoof_starts:
                spoof_side = "buy" if spoof_rng.random() < 0.5 else "sell"
                price = behind_touch(spoof_side, spoof_rng.randint(1, 2))
                spoof_order = ex.rest(SPOOFER, spoof_side, price, USUAL * spoof_rng.randint(5, 10))
            elif step - 1 in spoof_starts and spoof_order is not None:
                bid, ask = ex.best()
                other = "sell" if spoof_side == "buy" else "buy"
                price = bid if other == "sell" else ask
                ex.place(SPOOFER, other, price, spoof_rng.randint(5, 20), "ioc")
            elif step - 2 in spoof_starts and spoof_order is not None:
                ex.cancel_if_resting(SPOOFER, spoof_order)
                spoof_order = None
        if present["closer"]:
            _, ask = ex.best()
            if step == closer_early and ask is not None:
                ex.place(CLOSER, "buy", ask + 2, close_rng.randint(20, 60), "ioc")
            elif final10 and close_rng.random() < 0.6 and ask is not None:
                ex.place(
                    CLOSER, "buy", ask + close_rng.randint(2, 5), close_rng.randint(10, 30), "ioc"
                )
        if (
            present["insider"]
            and final30
            and insider_got < insider_target
            and insider_rng.random() < 0.5
        ):
            bid, ask = ex.best()
            side = "buy" if outcome == "yes" else "sell"
            price = (ask + 3) if side == "buy" and ask is not None else None
            if side == "sell" and bid is not None:
                price = bid - 3
            if price is not None:
                price = min(99, max(1, price))
            qty = min(insider_rng.randint(20, 60), insider_target - insider_got)
            insider_got += _filled(ex.place(INSIDER, side, price, qty, "ioc"), INSIDER)

    ex.send(cmd="set_state", market=MARKET, state="closed")
    ex.send(cmd="settle", market=MARKET, outcome=outcome)

    links = {LINKED_NOISE[0]: {LINKED_NOISE[1]}, LINKED_NOISE[1]: {LINKED_NOISE[0]}}
    if present["wash"]:
        links |= {WASH[0]: {WASH[1]}, WASH[1]: {WASH[0]}}
    truth = frozenset(
        (TYPOLOGY_OF_KIND[KIND[a]], a)
        for a in [*WASH, SPOOFER, CLOSER, INSIDER]
        if present[KIND[a]]
    )
    return Simulated(seed, ex.records, links, truth, dict(KIND), outcome)


def commands_per_step(sim: Simulated) -> float:
    """Market commands per step: the detectors' clock against the simulation's."""
    market = sum(1 for c, _ in sim.records if c["cmd"] != "open_account")
    return market / STEPS
