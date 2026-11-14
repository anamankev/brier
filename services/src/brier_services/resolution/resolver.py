"""One rulebook from fetch to proposal (docs/resolution.md).

- Before the release time: wait.
- A value: round it to the series' precision, decide ``value op threshold``, propose YES or NO.
- No value, before the deadline: wait. At or after the deadline: propose void
  (``no_value_by_deadline``), with the response that showed no value as evidence.
- An HTTP error, or a response the parser cannot read: raise. Nothing is proposed.

``source_discontinued`` is never proposed automatically; an admin proposes it by hand.
"""

from __future__ import annotations

import datetime as dt
import hashlib
from dataclasses import dataclass
from types import ModuleType
from typing import Any

from brier_services.resolution import (
    balldontlie,
    bls,
    football_data,
    fred,
    noaa,
    nws,
    second_source,
    swpc,
    usgs,
)
from brier_services.resolution.fetch import Fetcher, Response, fetch
from brier_services.resolution.sources import MalformedResponseError, Missing, Reading
from brier_services.resolution.values import format_decimal, holds, round_half_up
from brier_services.rules import Provider, Rulebook, instant

RESOLVER_VERSION = "brier-resolver 4"  # bump with any change to fetching, parsing, or deciding

SOURCES: dict[Provider, ModuleType] = {
    Provider.BLS: bls,
    Provider.FRED: fred,
    Provider.NOAA: noaa,
    Provider.USGS: usgs,
    Provider.SWPC: swpc,
    Provider.BALLDONTLIE: balldontlie,
    Provider.FOOTBALL_DATA: football_data,
}


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
    extra: tuple[bytes, ...] = ()  # other responses the record names by hash (NWS: the search)


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
    outcome = "yes" if holds(value, rule.op, rule.threshold, rule.upper) else "no"
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
    record = {
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
    if rule.upper is not None:  # schema 2 ranges; earlier records keep their exact shape
        record["upper"] = rule.upper
    return record


def _resolve_nws(rule: Rulebook, now: int, fetcher: Fetcher) -> Proposal | Wait:
    try:
        found = nws.read(
            rule, fetcher, dt.datetime.fromtimestamp(now, dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
        )
    except nws.SourceError as e:
        raise SourceError(str(e)) from e
    retrieved = instant(found.response.retrieved_at)
    if retrieved is None:
        msg = f"unreadable retrieval time {found.response.retrieved_at!r}"
        raise MalformedResponseError(msg)
    decided = decide(rule, found.reading, retrieved)
    if decided is None:
        assert isinstance(found.reading, Missing)  # noqa: S101 - decide waits only on a missing value
        return Wait(found.reading.why)
    record = evidence_record(rule, found.response, found.reading, decided)
    record["index_url"] = found.index_url
    record["index_sha256"] = found.index_sha256
    return Proposal(decided[0], decided[1], record, found.response.body, (found.index_body,))


def resolve(
    rule: Rulebook, now: int, api_key: str | None = None, fetcher: Fetcher = fetch
) -> Proposal | Wait:
    """Fetch the rule's source and propose an outcome, or say why not yet."""
    if now < rule.times.release:
        return Wait("before the release time")
    if rule.provider is Provider.NWS:
        return _resolve_nws(rule, now, fetcher)
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


def verify_source(
    rule: Rulebook, now: int, api_key: str | None = None, fetcher: Fetcher = fetch
) -> Proposal | Wait:
    """The fetch that verifies a proposal. For balldontlie games it is an independent source
    (second_source), recorded under that source's own provider name, so the gateway can check
    the two are different; for everything else, a fresh fetch of the rule's own source."""
    if rule.provider is not Provider.BALLDONTLIE:
        return resolve(rule, now, api_key, fetcher)
    if now < rule.times.release:
        return Wait("before the release time")
    response = fetcher(second_source.request(rule))
    if response.status != 200:
        msg = f"{response.url} answered HTTP {response.status}"
        raise SourceError(msg)
    reading = second_source.parse(response.body, rule)
    retrieved = instant(response.retrieved_at)
    if retrieved is None:
        msg = f"unreadable retrieval time {response.retrieved_at!r}"
        raise MalformedResponseError(msg)
    decided = decide(rule, reading, retrieved)
    if decided is None:
        assert isinstance(reading, Missing)  # noqa: S101 - decide waits only on a missing value
        return Wait(reading.why)
    record = evidence_record(rule, response, reading, decided)
    record["provider"] = second_source.PROVIDER[rule.series.id]
    record["verifies"] = str(rule.provider)
    return Proposal(decided[0], decided[1], record, response.body)
