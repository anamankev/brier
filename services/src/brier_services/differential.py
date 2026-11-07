"""Differential testing: the Rust engine against the reference engine (SPEC §3.4).

A seeded generator produces a stream of wire commands that mixes every command and order type,
a few accounts (so self-trade prevention fires), prices clustered near 50 (so orders cross),
out-of-range prices and quantities, lifecycle changes, and new markets. The same stream goes to
the Rust replay tool (``brier-replay``) and to the reference engine, and every event must match.

Both sides stream: a writer thread feeds the Rust process while this thread regenerates the
same stream from the same seed for the reference engine. Memory stays constant, so the same
code runs 5,000 commands in a unit test and 10 million in the gate study.

Run: ``python -m brier_services.differential --binary target/release/brier-replay
--seed 1 --commands 10000000``
"""

from __future__ import annotations

import argparse
import contextlib
import json
import random
import subprocess
import sys
import threading
import time
from collections.abc import Iterator
from dataclasses import dataclass
from pathlib import Path
from typing import IO, Any

from brier_services.reference_engine import ReferenceEngine

RECENT_ORDERS = 200  # cancels and amends mostly target recent orders, which are often live
ACCOUNTS = 4  # few accounts, so the same account often meets itself (self-trade prevention)


def generate(seed: int, count: int) -> Iterator[dict[str, Any]]:
    """Yield ``count`` wire commands, deterministic in ``seed``.

    The stream starts by listing and opening markets 1-3, so trading begins at once.
    """
    rng = random.Random(seed)  # noqa: S311 - test data, not cryptography
    seq = 0
    markets = 0
    recent: list[int] = []

    def command(cmd: dict[str, Any]) -> dict[str, Any]:
        nonlocal seq
        seq += 1
        return {"seq": seq, **cmd}

    def price() -> int:
        roll = rng.random()
        if roll < 0.8:
            return rng.randint(45, 55)
        if roll < 0.97:
            return rng.randint(1, 99)
        return rng.choice([0, 100, 255])

    def qty() -> int:
        return rng.randint(1, 30) if rng.random() < 0.96 else rng.choice([0, 25_001])

    def order_id() -> int:
        if recent and rng.random() < 0.9:
            return rng.choice(recent)
        return rng.randint(1, seq + 1)

    for market in (1, 2, 3):
        if seq >= count:
            return
        yield command({"cmd": "create_market", "market": market})
        markets = market
        if seq >= count:
            return
        yield command({"cmd": "set_state", "market": market, "state": "open"})
    while seq < count:
        roll = rng.random()
        if roll < 0.60:
            cmd = command(
                {
                    "cmd": "place",
                    "account": rng.randint(1, ACCOUNTS),
                    "market": rng.randint(1, markets),
                    "side": rng.choice(["buy", "sell"]),
                    "price": price(),
                    "qty": qty(),
                    "tif": rng.choices(["gtc", "ioc", "fok"], weights=[6, 2, 2])[0],
                    "post_only": rng.random() < 0.1,
                }
            )
            recent.append(cmd["seq"])
            if len(recent) > RECENT_ORDERS:
                recent.pop(0)
            yield cmd
        elif roll < 0.85:
            yield command(
                {"cmd": "cancel", "account": rng.randint(1, ACCOUNTS), "order": order_id()}
            )
        elif roll < 0.93:
            yield command(
                {
                    "cmd": "amend",
                    "account": rng.randint(1, ACCOUNTS),
                    "order": order_id(),
                    "qty": qty(),
                }
            )
        elif roll < 0.99:
            state = rng.choices(
                ["open", "halted", "closed", "pre_open", "resolved", "voided"],
                weights=[40, 30, 8, 8, 7, 7],
            )[0]
            yield command(
                {"cmd": "set_state", "market": rng.randint(1, markets + 1), "state": state}
            )
        else:
            # Mostly a new market (then opened), sometimes a duplicate.
            market = markets + 1 if rng.random() < 0.8 else rng.randint(1, markets)
            yield command({"cmd": "create_market", "market": market})
            markets = max(markets, market)
            if seq < count and market == markets:
                yield command({"cmd": "set_state", "market": market, "state": "open"})


@dataclass(frozen=True)
class Divergence:
    """The first point where the two engines disagree."""

    seq: int | None
    rust: list[dict[str, Any]]
    reference: list[dict[str, Any]]


@dataclass(frozen=True)
class Report:
    """The result of one differential run."""

    seed: int
    commands: int
    events: int
    trades: int
    seconds: float
    divergence: Divergence | None


def _feed(process_stdin: IO[str], seed: int, count: int) -> None:
    try:
        for cmd in generate(seed, count):
            process_stdin.write(json.dumps(cmd, separators=(",", ":")))
            process_stdin.write("\n")
    except BrokenPipeError:
        return  # the comparison stopped early (a divergence) and killed the replay tool
    finally:
        with contextlib.suppress(BrokenPipeError):
            process_stdin.close()


def run(binary: Path, seed: int, count: int) -> Report:
    """Run ``count`` generated commands through both engines and compare every event."""
    start = time.perf_counter()
    process = subprocess.Popen(  # noqa: S603 - our own binary, fixed arguments
        [str(binary)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1 << 20,
    )
    if process.stdin is None or process.stdout is None or process.stderr is None:
        msg = "could not open pipes to the replay tool"
        raise RuntimeError(msg)
    writer = threading.Thread(target=_feed, args=(process.stdin, seed, count), daemon=True)
    writer.start()
    reference = ReferenceEngine()
    rust_lines = (json.loads(line) for line in process.stdout)
    pending: dict[str, Any] | None = None
    events = trades = 0
    divergence: Divergence | None = None
    try:
        for cmd in generate(seed, count):
            expected = reference.apply(cmd)
            got: list[dict[str, Any]] = []
            while True:
                line = pending if pending is not None else next(rust_lines, None)
                pending = None
                if line is None or line.get("seq") != cmd["seq"]:
                    pending = line
                    break
                got.append(line)
            if got != expected:
                divergence = Divergence(cmd["seq"], got, expected)
                break
            events += len(expected)
            trades += sum(1 for e in expected if e["ev"] == "trade")
        if divergence is None:
            final = pending if pending is not None else next(rust_lines, None)
            if final != reference.final_book():
                divergence = Divergence(None, [final] if final else [], [reference.final_book()])
    finally:
        process.stdout.close()
        process.kill()
        process.wait()
        writer.join(timeout=10)
    if divergence is None and process.returncode not in (0, -9):
        msg = f"replay tool exited {process.returncode}: {process.stderr.read()}"
        raise RuntimeError(msg)
    return Report(seed, count, events, trades, time.perf_counter() - start, divergence)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--commands", type=int, required=True)
    args = parser.parse_args(argv)
    report = run(args.binary, args.seed, args.commands)
    print(
        json.dumps(
            {
                "seed": report.seed,
                "commands": report.commands,
                "events": report.events,
                "trades": report.trades,
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
