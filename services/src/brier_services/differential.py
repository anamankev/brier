"""Differential testing: the Rust engine against the reference engine (SPEC §3.4).

A seeded, state-aware generator produces wire commands. It runs the reference engine as it goes
and uses that engine's live state to aim most commands where they do something:
- cancels and amends at orders that are live, sent by their owner;
- orders at markets that are open;
- settlements at markets that are closed;
- state changes along allowed transitions.

A share of every kind is still aimed at random (unknown orders, other accounts' orders,
closed markets, out-of-range values), so every rejection path stays covered. The same commands
go to the Rust replay tool (``brier-replay``), and every event the Rust engine emits must equal
the reference engine's, in order. A reader thread drains the Rust output while this thread
generates and compares, so memory stays bounded at any stream length.

Run: ``python -m brier_services.differential --binary target/release/brier-replay
--seed 1 --commands 10000000 --check-every 100000``
"""

from __future__ import annotations

import argparse
import json
import queue
import random
import subprocess
import sys
import threading
import time
from collections import deque
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Any

from brier_services.reference_engine import MAX_RESTING_ORDERS, ReferenceEngine

BUSY_ACCOUNTS = 4  # few busy accounts, so the same account often meets itself (self-trade)
BURST_EVERY = 1_000_000  # a resting-order-limit burst every million commands
BURST_AT = 2_500  # the first burst starts after this many commands
BURST_SIZE = MAX_RESTING_ORDERS + 1
AIMED = 0.9  # share of cancels, amends, places, and settles aimed at a valid target
# Most markets trading at once. Above this the oldest is closed, so it settles and returns its
# escrow; without a cap, positions pile up in markets that never settle and lock the busy
# accounts' cash (docs/notes/differential.md, post-G2 audit).
MAX_LIVE_MARKETS = 6

# Weights for the next state of a market, from its current state (allowed transitions only).
NEXT_STATE: dict[str, tuple[list[str], list[int]]] = {
    "pre_open": (["open", "closed"], [9, 1]),
    "open": (["halted", "closed"], [6, 4]),
    "halted": (["open", "closed"], [8, 2]),
}


class _Pool:
    """A set with O(1) add, remove, and uniform choice, in a deterministic order."""

    def __init__(self) -> None:
        self.items: list[int] = []
        self.index: dict[int, int] = {}

    def __len__(self) -> int:
        return len(self.items)

    def add(self, x: int) -> None:
        if x not in self.index:
            self.index[x] = len(self.items)
            self.items.append(x)

    def remove(self, x: int) -> None:
        i = self.index.pop(x, None)
        if i is None:
            return
        last = self.items.pop()
        if last != x:
            self.items[i] = last
            self.index[last] = i


class StreamGenerator:
    """Yields ``(command, reference events)`` pairs, deterministic in the seed.

    The stream opens accounts 1-4, lists and opens markets 1-3, then mixes every command.
    Every million commands (first after command 2,500) it adds a burst: a fresh market and
    1,001 one-contract GTC orders from account 1, so the resting-order limit is reached.
    """

    def __init__(self, seed: int, count: int) -> None:
        self.rng = random.Random(seed)  # noqa: S311 - test data, not cryptography
        self.count = count
        self.engine = ReferenceEngine()
        self.seq = 0
        self.accounts = 0
        self.markets = 0
        # Live resting orders, for uniform choice: id -> (owner, remaining), plus a list.
        self.live: dict[int, tuple[int, int]] = {}
        self.live_ids: list[int] = []
        self.live_index: dict[int, int] = {}
        # Markets by state (only the states commands aim at), and each market's state.
        self.pools: dict[str, _Pool] = {
            s: _Pool() for s in ("pre_open", "open", "halted", "closed")
        }
        self.state_of: dict[int, str] = {}

    # ----- bookkeeping from the reference engine's events -----

    def _add_live(self, order: int, owner: int, remaining: int) -> None:
        self.live[order] = (owner, remaining)
        self.live_index[order] = len(self.live_ids)
        self.live_ids.append(order)

    def _drop_live(self, order: int) -> None:
        if order not in self.live:
            return
        del self.live[order]
        i = self.live_index.pop(order)
        last = self.live_ids.pop()
        if last != order:
            self.live_ids[i] = last
            self.live_index[last] = i

    def _observe(self, cmd: dict[str, Any], events: list[dict[str, Any]]) -> None:
        for e in events:
            ev = e["ev"]
            if ev == "rested":
                self._add_live(e["order"], cmd["account"], e["remaining"])
            elif ev == "trade" and e["maker"] in self.live:
                owner, remaining = self.live[e["maker"]]
                left = remaining - e["qty"]
                if left == 0:
                    self._drop_live(e["maker"])
                else:
                    self.live[e["maker"]] = (owner, left)
            elif ev == "amended":
                owner, _ = self.live[e["order"]]
                self.live[e["order"]] = (owner, e["remaining"])
            elif ev == "cancelled":
                self._drop_live(e["order"])
            elif ev == "account_opened":
                self.accounts = max(self.accounts, e["account"])
            elif ev == "market_created":
                self.markets = max(self.markets, e["market"])
                self._move(e["market"], "pre_open")
            elif ev == "market_state":
                self._move(e["market"], e["state"])
            elif ev == "settled":
                self._move(e["market"], "voided" if e["outcome"] == "void" else "resolved")

    def _move(self, market: int, state: str) -> None:
        old = self.state_of.get(market)
        if old in self.pools:
            self.pools[old].remove(market)
        self.state_of[market] = state
        if state in self.pools:
            self.pools[state].add(market)

    def _emit(self, cmd: dict[str, Any]) -> tuple[dict[str, Any], list[dict[str, Any]]]:
        self.seq += 1
        cmd = {"seq": self.seq, **cmd}
        events = self.engine.apply(cmd)
        self._observe(cmd, events)
        return cmd, events

    # ----- random choices -----

    def _account(self) -> int:
        # Mostly the busy accounts, then the newest accounts (fresh cash), then any account,
        # including one not yet opened.
        roll = self.rng.random()
        if roll < 0.7:
            return self.rng.randint(1, BUSY_ACCOUNTS)
        if roll < 0.9:
            return self.rng.randint(max(1, self.accounts - 9), max(1, self.accounts))
        return self.rng.randint(1, self.accounts + 1)

    def _market(self, states: tuple[str, ...]) -> int:
        """Usually a market in one of ``states``; otherwise any market, or one not yet listed."""
        pools = [self.pools[s] for s in states]
        total = sum(len(p) for p in pools)
        if total and self.rng.random() < AIMED:
            i = self.rng.randrange(total)
            for pool in pools:
                if i < len(pool):
                    return pool.items[i]
                i -= len(pool)
        return self.rng.randint(1, self.markets + 1)

    def _price(self) -> int:
        roll = self.rng.random()
        if roll < 0.8:
            return self.rng.randint(45, 55)
        if roll < 0.97:
            return self.rng.randint(1, 99)
        return self.rng.choice([0, 100, 255])

    def _qty(self) -> int:
        roll = self.rng.random()
        if roll < 0.975:
            return self.rng.randint(1, 30)
        if roll < 0.98:
            return self.rng.randint(1_000, 25_000)  # large enough to hit insufficient_collateral
        return self.rng.choice([0, 25_001])

    def _target_order(self) -> tuple[int, int]:
        """(account, order): usually a live order sent by its owner."""
        if self.live_ids and self.rng.random() < AIMED:
            order = self.rng.choice(self.live_ids)
            return self.live[order][0], order
        return self._account(), self.rng.randint(1, self.seq + 1)

    # ----- the stream -----

    def _next_commands(self) -> list[dict[str, Any]]:
        live = self.pools["open"].items + self.pools["halted"].items
        if len(live) > MAX_LIVE_MARKETS:
            return [{"cmd": "set_state", "market": min(live), "state": "closed"}]
        rng = self.rng
        roll = rng.random()
        if roll < 0.55:
            return [
                {
                    "cmd": "place",
                    "account": self._account(),
                    "market": self._market(("open",)),
                    "side": rng.choice(["buy", "sell"]),
                    "price": self._price(),
                    "qty": self._qty(),
                    "tif": rng.choices(["gtc", "ioc", "fok"], weights=[6, 2, 2])[0],
                    "post_only": rng.random() < 0.1,
                }
            ]
        if roll < 0.77:
            account, order = self._target_order()
            return [{"cmd": "cancel", "account": account, "order": order}]
        if roll < 0.85:
            account, order = self._target_order()
            remaining = self.live.get(order, (0, 0))[1]
            qty = (
                rng.randint(1, remaining - 1)
                if remaining > 1 and rng.random() < 0.7
                else self._qty()
            )
            return [{"cmd": "amend", "account": account, "order": order, "qty": qty}]
        if roll < 0.88:
            market = self._market(("pre_open", "open", "halted"))
            state = self.state_of.get(market)
            if state in NEXT_STATE and rng.random() < AIMED:
                targets, weights = NEXT_STATE[state]
                target = rng.choices(targets, weights=weights)[0]
            else:
                target = rng.choice(["pre_open", "open", "halted", "closed", "resolved", "voided"])
            return [{"cmd": "set_state", "market": market, "state": target}]
        if roll < 0.905:
            return [
                {
                    "cmd": "settle",
                    "market": self._market(("closed",)),
                    "outcome": rng.choice(["yes", "no", "void"]),
                }
            ]
        if roll < 0.91:
            filters: dict[str, Any] = {}
            if rng.random() < 0.6:
                filters["account"] = self._account()
            if rng.random() < 0.5:
                filters["market"] = self._market(("open", "halted"))
            return [{"cmd": "cancel_all", **filters}]
        if roll < 0.911:
            return [{"cmd": "halt_all"}]
        if roll < 0.916:
            new = self.accounts + 1 if rng.random() < 0.8 else rng.randint(1, max(1, self.accounts))
            return [{"cmd": "open_account", "account": new}]
        if roll < 0.931:
            market = (
                self.markets + 1 if rng.random() < 0.8 else rng.randint(1, max(1, self.markets))
            )
            out: list[dict[str, Any]] = [{"cmd": "create_market", "market": market}]
            if market > self.markets:
                out.append({"cmd": "set_state", "market": market, "state": "open"})
            return out
        # Reopen a halted market (keeps trading alive after halt_all).
        return [{"cmd": "set_state", "market": self._market(("halted",)), "state": "open"}]

    def __iter__(self) -> Iterator[tuple[dict[str, Any], list[dict[str, Any]]]]:
        setup: list[dict[str, Any]] = [
            {"cmd": "open_account", "account": a} for a in range(1, BUSY_ACCOUNTS + 1)
        ]
        for market in (1, 2, 3):
            setup += [
                {"cmd": "create_market", "market": market},
                {"cmd": "set_state", "market": market, "state": "open"},
            ]
        pending: deque[dict[str, Any]] = deque(setup)
        next_burst = BURST_AT
        while self.seq < self.count:
            if not pending:
                if self.seq >= next_burst and self.seq + BURST_SIZE + 2 <= self.count:
                    next_burst += BURST_EVERY
                    market = self.markets + 1
                    pending.append({"cmd": "create_market", "market": market})
                    pending.append({"cmd": "set_state", "market": market, "state": "open"})
                    pending.extend(
                        {
                            "cmd": "place",
                            "account": 1,
                            "market": market,
                            "side": "buy",
                            "price": 1,
                            "qty": 1,
                            "tif": "gtc",
                            "post_only": False,
                        }
                        for _ in range(BURST_SIZE)
                    )
                else:
                    pending.extend(self._next_commands())
            yield self._emit(pending.popleft())


def generate(seed: int, count: int) -> Iterator[dict[str, Any]]:
    """Just the commands of the stream for ``seed`` (see ``StreamGenerator``)."""
    for cmd, _ in StreamGenerator(seed, count):
        yield cmd


@dataclass(frozen=True)
class Divergence:
    """The first point where the two engines disagree."""

    seq: int | None
    rust: dict[str, Any] | None
    reference: dict[str, Any] | None


@dataclass(frozen=True)
class Report:
    """The result of one differential run."""

    seed: int
    commands: int
    events: int
    trades: int
    accepted: int
    rejected: int
    seconds: float
    divergence: Divergence | None


def _drain(stdout: IO[str], lines: queue.Queue[str | None]) -> None:
    for line in stdout:
        lines.put(line)
    lines.put(None)


def run(binary: Path, seed: int, count: int, check_every: int = 0) -> Report:
    """Run ``count`` generated commands through both engines and compare every event.

    With ``check_every > 0``, both engines also run their full invariant checks (I1-I5)
    every ``check_every`` commands and at the end.
    """
    start = time.perf_counter()
    process = subprocess.Popen(  # noqa: S603 - our own binary, fixed arguments
        [str(binary), *(["--check-every", str(check_every)] if check_every else [])],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1 << 20,
    )
    if process.stdin is None or process.stdout is None or process.stderr is None:
        msg = "could not open pipes to the replay tool"
        raise RuntimeError(msg)
    lines: queue.Queue[str | None] = queue.Queue(maxsize=1 << 16)
    reader = threading.Thread(target=_drain, args=(process.stdout, lines), daemon=True)
    reader.start()
    stream = StreamGenerator(seed, count)
    expected: deque[dict[str, Any]] = deque()
    events = trades = accepted = rejected = 0
    divergence: Divergence | None = None
    rust_done = False

    def compare(line: str | None) -> Divergence | None:
        nonlocal rust_done
        if line is None:
            rust_done = True
            head = expected[0] if expected else None
            return None if head is None else Divergence(head.get("seq"), None, head)
        got = json.loads(line)
        if not expected:
            return Divergence(got.get("seq"), got, None)
        want = expected.popleft()
        return None if got == want else Divergence(want.get("seq"), got, want)

    try:
        for cmd, cmd_events in stream:
            process.stdin.write(json.dumps(cmd, separators=(",", ":")) + "\n")
            if check_every and cmd["seq"] % check_every == 0:
                stream.engine.check_invariants()
            expected.extend(cmd_events)
            events += len(cmd_events)
            trades += sum(1 for e in cmd_events if e["ev"] == "trade")
            accepted += sum(1 for e in cmd_events if e["ev"] == "accepted")
            rejected += sum(1 for e in cmd_events if e["ev"] == "rejected")
            # Compare whatever the Rust side has produced so far; block if far behind.
            while not rust_done and (len(expected) > 100_000 or not lines.empty()):
                divergence = compare(lines.get())
                if divergence is not None:
                    break
            if divergence is not None:
                break
        if divergence is None:
            if check_every:
                stream.engine.check_invariants()
            expected.append(stream.engine.final_book())
            process.stdin.close()
            while divergence is None and not rust_done:
                divergence = compare(lines.get())
            if divergence is None and expected:
                divergence = Divergence(None, None, expected[0])
    except BrokenPipeError:
        divergence = Divergence(None, None, {"error": "the replay tool stopped reading input"})
    finally:
        process.kill()
        process.wait()
    if divergence is None and process.returncode not in (0, -9):
        msg = f"replay tool exited {process.returncode}: {process.stderr.read()}"
        raise RuntimeError(msg)
    return Report(
        seed, count, events, trades, accepted, rejected, time.perf_counter() - start, divergence
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--commands", type=int, required=True)
    parser.add_argument("--check-every", type=int, default=0)
    parser.add_argument(
        "--emit",
        action="store_true",
        help="print the stream's commands (without seq) instead of comparing engines",
    )
    args = parser.parse_args(argv)
    if args.emit:
        out = sys.stdout
        for cmd in generate(args.seed, args.commands):
            out.write(
                json.dumps({k: v for k, v in cmd.items() if k != "seq"}, separators=(",", ":"))
            )
            out.write("\n")
        return 0
    if args.binary is None:
        parser.error("--binary is required unless --emit is given")
    report = run(args.binary, args.seed, args.commands, args.check_every)
    print(
        json.dumps(
            {
                "seed": report.seed,
                "commands": report.commands,
                "events": report.events,
                "trades": report.trades,
                "accepted": report.accepted,
                "rejected": report.rejected,
                "seconds": round(report.seconds, 1),
                "divergence": None
                if report.divergence is None
                else {
                    "seq": report.divergence.seq,
                    "rust": report.divergence.rust,
                    "reference": report.divergence.reference,
                },
            }
        )
    )
    return 0 if report.divergence is None else 1


if __name__ == "__main__":
    sys.exit(main())
