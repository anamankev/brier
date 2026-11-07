"""Crash harness for ``brier-core`` (docs/notes/durability.md, D1 and D2).

Each cycle starts the core on the same data directory, checks its recovery, drives it with
commands from the state-aware stream generator, and kills it with SIGKILL at a random moment.
After every restart:
- D1: the recovered journal ends at or after the highest sequence number ever acknowledged,
  so no acknowledged command was lost.
- D2: the state hash the core reports after recovery (newest snapshot plus journal tail)
  equals the hash from ``brier-core verify``, a clean read-only replay of the whole journal
  from empty, which also runs every invariant check.

Small segments and frequent snapshots make kills land during segment rotation and during
snapshot writes, not only mid-batch. SIGKILL never tears a write (the kernel finishes each
`write` call), so after some kills the harness appends random bytes to the last segment,
imitating a power cut in the middle of an unacknowledged batch. Recovery must cut them off
without losing anything acknowledged. A final restart after the last cycle checks that
cycle's acknowledgments too.

Run: ``python -m brier_services.crash --core target/release/brier-core --data /tmp/x
--cycles 1000 --seed 1``
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
from dataclasses import dataclass, field
from pathlib import Path
from typing import IO, Any

from brier_services.differential import StreamGenerator

SEGMENT_BYTES = 64 * 1024  # small, so rotation happens every few hundred commands
SNAPSHOT_EVERY = 2_000
KILL_WINDOW_SECONDS = 0.2  # each kill lands uniformly in [0, 0.2] s after the core is ready
TEAR_PROBABILITY = 0.3  # share of kills followed by a simulated torn write
MAX_TEAR_BYTES = 4_096  # garbage appended; well under one batch (MAX_BATCH_BYTES, 1 MiB)
STREAM_LENGTH = 10**9  # the generator runs as long as the harness needs


@dataclass
class Summary:
    """What the harness observed."""

    cycles: int = 0
    acknowledged: int = 0  # highest sequence number acknowledged in any cycle
    commands_journaled: int = 0
    truncations: int = 0
    tears_injected: int = 0
    snapshot_recoveries: int = 0
    skipped_snapshots: int = 0
    failures: list[str] = field(default_factory=list)


def _feed(stdin: IO[str], stream: Any, stop: threading.Event) -> None:  # noqa: ANN401
    """Write commands until the core dies or ``stop`` is set."""
    with contextlib.suppress(BrokenPipeError, OSError, ValueError):
        for cmd, _ in stream:
            if stop.is_set():
                return
            body = {k: v for k, v in cmd.items() if k != "seq"}  # the core assigns sequence
            stdin.write(json.dumps(body, separators=(",", ":")) + "\n")
            stdin.flush()


def _read_acks(stdout: IO[str], acks: list[int]) -> None:
    with contextlib.suppress(ValueError, OSError):
        acks.extend(int(json.loads(line)["ack"]) for line in stdout if line.startswith('{"ack":'))


def _verify(core: Path, data: Path) -> dict[str, Any]:
    out = subprocess.run(  # noqa: S603 - our own binary, fixed arguments
        [str(core), "verify", "--data", str(data)],
        capture_output=True,
        text=True,
        check=False,
    )
    if out.returncode != 0:
        return {"error": out.stderr.strip()}
    result: dict[str, Any] = json.loads(out.stdout)["verified"]
    return result


def run(core: Path, data: Path, cycles: int, seed: int) -> Summary:
    """Run ``cycles`` kill-and-recover cycles; every check failure is recorded, not raised."""
    rng = random.Random(seed)  # noqa: S311 - test timing, not cryptography
    stream = iter(StreamGenerator(seed, STREAM_LENGTH))
    summary = Summary()
    data.mkdir(parents=True, exist_ok=True)
    for cycle in range(1, cycles + 1):
        proc = subprocess.Popen(  # noqa: S603 - our own binary, fixed arguments
            [
                str(core),
                "serve",
                "--data",
                str(data),
                "--snapshot-every",
                str(SNAPSHOT_EVERY),
                "--segment-bytes",
                str(SEGMENT_BYTES),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        if proc.stdin is None or proc.stdout is None:
            msg = "could not open pipes to brier-core"
            raise RuntimeError(msg)
        first = proc.stdout.readline()
        if not first.startswith('{"ready":'):
            err = proc.stderr.read() if proc.stderr else ""
            summary.failures.append(f"cycle {cycle}: core did not start: {first!r} {err.strip()}")
            proc.kill()
            proc.wait()
            break
        ready = json.loads(first)["ready"]
        recovered_last = ready["next_seq"] - 1
        if recovered_last < summary.acknowledged:
            summary.failures.append(
                f"cycle {cycle}: D1 lost acknowledged commands: acknowledged up to "
                f"{summary.acknowledged}, recovered only to {recovered_last}"
            )
        verified = _verify(core, data)
        if "error" in verified:
            summary.failures.append(f"cycle {cycle}: D2 verify failed: {verified['error']}")
        elif verified["state_hash"] != ready["state_hash"] or verified["last_seq"] not in (
            recovered_last,
            None if recovered_last == 0 else recovered_last,
        ):
            summary.failures.append(
                f"cycle {cycle}: D2 recovered state {ready['state_hash']} at {recovered_last} "
                f"differs from clean replay {verified['state_hash']} at {verified['last_seq']}"
            )
        summary.truncations += ready["truncation"] is not None
        summary.snapshot_recoveries += ready["snapshot"] is not None
        summary.skipped_snapshots += len(ready["skipped_snapshots"])
        summary.commands_journaled = recovered_last

        acks: list[int] = []
        stop = threading.Event()
        writer = threading.Thread(target=_feed, args=(proc.stdin, stream, stop), daemon=True)
        reader = threading.Thread(target=_read_acks, args=(proc.stdout, acks), daemon=True)
        writer.start()
        reader.start()
        time.sleep(rng.uniform(0, KILL_WINDOW_SECONDS))
        proc.kill()  # SIGKILL: no chance to flush or clean up
        proc.wait()
        stop.set()
        reader.join(timeout=5)
        writer.join(timeout=5)
        with contextlib.suppress(OSError):
            proc.stdin.close()
        if acks:
            summary.acknowledged = max(summary.acknowledged, max(acks))
        if rng.random() < TEAR_PROBABILITY:
            _tear(data, rng)
            summary.tears_injected += 1
        summary.cycles = cycle
        if summary.failures:
            break
    if not summary.failures:
        _final_check(core, data, summary)
    return summary


def _tear(data: Path, rng: random.Random) -> None:
    """Append garbage to the last journal segment, as a power cut mid-batch would leave."""
    segments = sorted(data.glob("journal-*.log"))
    if segments:
        with segments[-1].open("ab") as f:
            f.write(rng.randbytes(rng.randint(1, MAX_TEAR_BYTES)))


def _final_check(core: Path, data: Path, summary: Summary) -> None:
    """Restart once more without load, so the last cycle's acknowledgments are checked too."""
    proc = subprocess.run(  # noqa: S603 - our own binary, fixed arguments
        [str(core), "serve", "--data", str(data), "--segment-bytes", str(SEGMENT_BYTES)],
        input="",
        capture_output=True,
        text=True,
        check=False,
    )
    first = proc.stdout.splitlines()[0] if proc.stdout else ""
    if not first.startswith('{"ready":'):
        summary.failures.append(f"final restart: core did not start: {proc.stderr.strip()}")
        return
    ready = json.loads(first)["ready"]
    summary.truncations += ready["truncation"] is not None
    recovered_last = ready["next_seq"] - 1
    summary.commands_journaled = recovered_last
    if recovered_last < summary.acknowledged:
        summary.failures.append(
            f"final restart: D1 acknowledged up to {summary.acknowledged}, "
            f"recovered to {recovered_last}"
        )
    verified = _verify(core, data)
    if verified.get("state_hash") != ready["state_hash"]:
        summary.failures.append(f"final restart: D2 mismatch: {verified} vs {ready['state_hash']}")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--core", type=Path, required=True)
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--cycles", type=int, required=True)
    parser.add_argument("--seed", type=int, required=True)
    args = parser.parse_args(argv)
    start = time.perf_counter()
    s = run(args.core, args.data, args.cycles, args.seed)
    print(
        json.dumps(
            {
                "cycles": s.cycles,
                "highest_acknowledged": s.acknowledged,
                "commands_journaled": s.commands_journaled,
                "tears_injected": s.tears_injected,
                "torn_tails_truncated": s.truncations,
                "recoveries_from_snapshot": s.snapshot_recoveries,
                "damaged_snapshots_skipped": s.skipped_snapshots,
                "failures": s.failures,
                "seconds": round(time.perf_counter() - start, 1),
            }
        )
    )
    return 0 if not s.failures else 1


if __name__ == "__main__":
    sys.exit(main())
