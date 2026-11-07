"""One rulebook from fetch to proposal (docs/resolution.md).

- Before the release time: wait.
- A value: round it to the series' precision, decide ``value op threshold``, propose YES or NO.
- No value, before the deadline: wait. At or after the deadline: propose void
  (``no_value_by_deadline``), with the response that showed no value as evidence.
- An HTTP error, or a response the parser cannot read: raise. Nothing is proposed.

``source_discontinued`` is never proposed automatically; an admin proposes it by hand.
"""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from types import ModuleType
from typing import Any

from brier_services.resolution import bls, fred, noaa
from brier_services.resolution.fetch import Fetcher, Response, fetch
from brier_services.resolution.sources import MalformedResponseError, Missing, Reading
from brier_services.resolution.values import format_decimal, holds, round_half_up
from brier_services.rules import Provider, Rulebook, instant

RESOLVER_VERSION = "brier-resolver 2"  # bump with any change to fetching, parsing, or deciding

SOURCES: dict[Provider, ModuleType] = {Provider.BLS: bls, Provider.FRED: fred, Provider.NOAA: noaa}


class SourceError(Exception):
    """The source answered with an HTTP error; try again later."""


@dataclass(frozen=True)
class Wait:
    """Nothing to propose yet."""

    reason: str


@dataclass(frozen=True)
class Proposal:
    """An outcome, its evidence record, and the raw response the record names."""

    outcome: str
    void_condition: str | None
    evidence: dict[str, Any]
    body: bytes


def decide(
    rule: Rulebook, reading: Reading, retrieved_at: int
) -> tuple[str, str | None, str | None] | None:
    """(outcome, void condition, value as published at the rule's precision), or ``None`` to
    wait. Pure: the same reading and time always give the same answer."""
    if isinstance(reading, Missing):
        if retrieved_at >= rule.times.deadline:
            return "void", "no_value_by_deadline", None
        return None
    value = round_half_up(reading, rule.measure.precision)
    outcome = "yes" if holds(value, rule.op, rule.threshold) else "no"
    return outcome, None, format_decimal(value, rule.measure.precision)


def evidence_record(
    rule: Rulebook,
    response: Response,
    reading: Reading,
    decided: tuple[str, str | None, str | None],
) -> dict[str, Any]:
    """The record the gateway checks (market, rules hash, outcome, retrieval time) and what a
    reader needs to check the decision: the request, the raw body's SHA-256, the value."""
    outcome, void_condition, value = decided
    return {
        "market": rule.market,
        "rules_hash": rule.hash,
        "outcome": outcome,
        "void_condition": void_condition,
        "retrieved_at": response.retrieved_at,
        "provider": str(rule.provider),
        "series": rule.series.id,
        "observation": rule.observation,
        "transform": str(rule.measure.transform),
        "op": str(rule.op),
        "threshold": rule.threshold,
        "value": value,
        "missing": reading.why if isinstance(reading, Missing) else None,
        "request_url": response.url,
        "request_body": response.request_body,
        "http_status": response.status,
        "body_sha256": hashlib.sha256(response.body).hexdigest(),
        "body_bytes": len(response.body),
        "resolver": RESOLVER_VERSION,
    }


def resolve(
    rule: Rulebook, now: int, api_key: str | None = None, fetcher: Fetcher = fetch
) -> Proposal | Wait:
    """Fetch the rule's source and propose an outcome, or say why not yet."""
    if now < rule.times.release:
        return Wait("before the release time")
    source = SOURCES[rule.provider]
    response = fetcher(source.request(rule, api_key, now))
    if response.status != 200:
        msg = f"{response.url} answered HTTP {response.status}"
        raise SourceError(msg)
    reading = source.parse(response.body, rule)
    retrieved = instant(response.retrieved_at)
    if retrieved is None:
        msg = f"unreadable retrieval time {response.retrieved_at!r}"
        raise MalformedResponseError(msg)
    decided = decide(rule, reading, retrieved)
    if decided is None:
        assert isinstance(reading, Missing)  # noqa: S101 - decide waits only on a missing value
        return Wait(reading.why)
    return Proposal(
        decided[0], decided[1], evidence_record(rule, response, reading, decided), response.body
    )
