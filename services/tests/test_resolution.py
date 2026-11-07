"""Resolvers on recorded and hand-written responses (G5 unit 5): parsing, exact arithmetic, the
decision, the evidence record and store, offline recomputation, and the CLI's pass."""

from __future__ import annotations

import json
from decimal import ROUND_HALF_UP, Decimal
from fractions import Fraction
from pathlib import Path
from typing import Any

import pytest
from hypothesis import given
from hypothesis import strategies as st

from brier_services import resolve as cli
from brier_services.resolution import bls, fred, noaa
from brier_services.resolution.evidence import EvidenceError, EvidenceStore, recompute
from brier_services.resolution.fetch import FetchError, Response, check_url, redact
from brier_services.resolution.resolver import Proposal, SourceError, Wait, resolve
from brier_services.resolution.sources import MalformedResponseError, Missing
from brier_services.resolution.values import format_decimal, holds, parse_decimal, round_half_up
from brier_services.rules import Op, Provider, Rulebook, find_series, instant, validate

FIXTURES = Path(__file__).parent / "fixtures" / "resolution"
BLS_CPI = (FIXTURES / "bls_cpi_2025_2026.json").read_bytes()
NOAA_JULY = (FIXTURES / "noaa_tmax_2025-07-01_03.json").read_bytes()
NOAA_NONE = (FIXTURES / "noaa_tmax_not_published.json").read_bytes()


def rulebook(
    market: int,
    provider: str,
    series: str,
    observation: str,
    transform: str,
    op: str,
    threshold: str,
    schedule: tuple[str, str, str, str],
) -> Rulebook:
    found = find_series(Provider(provider), series)
    assert found is not None
    open_, close, release, deadline = schedule
    return validate(
        {
            "schema": 1,
            "market": market,
            "title": f"Test market {market}",
            "question": f"Test question for market {market}?",
            "observation": observation,
            "revision": "first_release",
            "void": ["no_value_by_deadline", "source_discontinued"],
            "source": {"provider": provider, "series": series, "url": found.url},
            "condition": {"transform": transform, "op": op, "threshold": threshold},
            "schedule": {"open": open_, "close": close, "release": release, "deadline": deadline},
        }
    )


CPI_SEPT = rulebook(
    101,
    "bls",
    "CUUR0000SA0",
    "2026-09",
    "pct_change_12m",
    ">",
    "3.0",
    (
        "2026-09-01T13:30:00Z",
        "2026-10-15T12:00:00Z",
        "2026-10-15T12:30:00Z",
        "2026-12-15T12:30:00Z",
    ),
)
CPI_OCT_2025 = rulebook(
    102,
    "bls",
    "CUUR0000SA0",
    "2025-10",
    "pct_change_12m",
    ">",
    "3.0",
    (
        "2025-10-01T00:00:00Z",
        "2025-11-13T13:00:00Z",
        "2025-11-13T13:30:00Z",
        "2026-01-13T13:30:00Z",
    ),
)
TMAX = rulebook(
    103,
    "noaa",
    "USW00094728/TMAX",
    "2025-07-01",
    "level",
    ">=",
    "89",
    (
        "2025-06-01T00:00:00Z",
        "2025-07-01T00:00:00Z",
        "2025-07-03T00:00:00Z",
        "2025-09-01T00:00:00Z",
    ),
)
DGS10 = rulebook(
    104,
    "fred",
    "DGS10",
    "2025-07-01",
    "level",
    ">",
    "4.25",
    (
        "2025-06-01T00:00:00Z",
        "2025-07-01T00:00:00Z",
        "2025-07-02T00:00:00Z",
        "2025-08-01T00:00:00Z",
    ),
)
FED_UPPER = rulebook(
    105,
    "fred",
    "DFEDTARU",
    "2025-09-18",
    "level",
    "<",
    "4.50",
    (
        "2025-08-01T00:00:00Z",
        "2025-09-17T17:00:00Z",
        "2025-09-17T18:00:00Z",
        "2025-10-17T18:00:00Z",
    ),
)


def fred_body(*rows: tuple[str, str, str]) -> bytes:
    """A response in FRED's documented format: (realtime_start, date, value) rows."""
    return json.dumps(
        {
            "realtime_start": "2025-07-01",
            "realtime_end": "9999-12-31",
            "output_type": 4,
            "file_type": "json",
            "count": len(rows),
            "observations": [
                {"realtime_start": rs, "realtime_end": "9999-12-31", "date": d, "value": v}
                for rs, d, v in rows
            ],
        }
    ).encode()


def t(s: str) -> int:
    value = instant(s)
    assert value is not None
    return value


# ---- exact values --------------------------------------------------------------------------


@given(st.integers(-(10**9), 10**9), st.integers(0, 6), st.integers(0, 4))
def test_round_half_up_matches_decimal(n: int, scale: int, places: int) -> None:
    x = Fraction(n, 10**scale)
    expected = Decimal(n).scaleb(-scale).quantize(Decimal(1).scaleb(-places), ROUND_HALF_UP)
    assert round_half_up(x, places) == Fraction(expected)
    # Brier never writes "-0" (rulebook thresholds forbid it too); Decimal keeps the sign.
    unsigned_zero = expected.copy_abs() if expected.is_zero() else expected
    assert format_decimal(round_half_up(x, places), places) == str(unsigned_zero)


def test_decimals_are_parsed_exactly_and_strictly() -> None:
    assert parse_decimal("4.26") == Fraction(426, 100)
    assert parse_decimal(" -3.2 ") == Fraction(-32, 10)
    for bad in ["", ".", "1e3", "nan", "4,26", "1. 2", "+1", "0x10"]:
        assert parse_decimal(bad) is None, bad
    assert holds(Fraction(3), Op.AT_LEAST, "3.0")
    assert not holds(Fraction(3), Op.ABOVE, "3.0")
    assert holds(Fraction(29, 10), Op.BELOW, "3.0")
    assert holds(Fraction(3), Op.AT_MOST, "3")
    with pytest.raises(ValueError, match="more than"):
        format_decimal(Fraction(1, 3), 2)


# ---- parsers on recorded responses ---------------------------------------------------------


def test_bls_cpi_12_month_change_is_computed_from_the_published_index() -> None:
    value = bls.parse(BLS_CPI, CPI_SEPT)
    assert value == (Fraction("336.940") / Fraction("324.800") - 1) * 100
    assert isinstance(value, Fraction)
    assert format_decimal(round_half_up(value, 1), 1) == "3.7"
    level = rulebook(
        106,
        "bls",
        "CUUR0000SA0",
        "2026-09",
        "level",
        ">=",
        "336.940",
        tuple(CPI_SEPT.raw["schedule"].values()),
    )
    assert bls.parse(BLS_CPI, level) == Fraction("336.940")


def test_bls_months_not_published_or_published_as_unavailable_are_missing() -> None:
    assert bls.parse(BLS_CPI, CPI_OCT_2025) == Missing("2025-10 published as unavailable")
    november_2026 = rulebook(
        107,
        "bls",
        "CUUR0000SA0",
        "2026-11",
        "pct_change_12m",
        ">",
        "3.0",
        (
            "2026-11-01T00:00:00Z",
            "2026-12-10T13:00:00Z",
            "2026-12-10T13:30:00Z",
            "2027-02-10T13:30:00Z",
        ),
    )
    assert bls.parse(BLS_CPI, november_2026) == Missing("2026-11 not published")


def test_noaa_celsius_converts_exactly_to_whole_fahrenheit() -> None:
    value = noaa.parse(NOAA_JULY, TMAX)
    assert value == Fraction("31.7") * Fraction(9, 5) + 32
    assert isinstance(value, Fraction)
    assert round_half_up(value, 0) == 89
    assert noaa.parse(NOAA_NONE, TMAX) == Missing("2025-07-01 not published")


def test_fred_reads_the_first_release_not_a_revision() -> None:
    body = fred_body(("2025-07-02", "2025-07-01", "4.26"))
    assert fred.parse(body, DGS10) == Fraction("4.26")
    revised = fred_body(("2025-08-01", "2025-07-01", "4.31"), ("2025-07-02", "2025-07-01", "4.26"))
    assert fred.parse(revised, DGS10) == Fraction("4.26")
    assert fred.parse(fred_body(("2025-07-02", "2025-07-01", ".")), DGS10) == Missing(
        "2025-07-01 published as missing"
    )
    assert fred.parse(fred_body(), DGS10) == Missing("2025-07-01 not published")
    upper = fred_body(("2025-09-17", "2025-09-17", "4.50"), ("2025-09-18", "2025-09-18", "4.25"))
    assert fred.parse(upper, FED_UPPER) == Fraction("4.25")


@pytest.mark.parametrize(
    ("module", "rule", "body"),
    [
        (bls, CPI_SEPT, BLS_CPI[: len(BLS_CPI) // 2]),  # truncated
        (bls, CPI_SEPT, b'{"status": "REQUEST_NOT_PROCESSED", "message": ["daily threshold"]}'),
        (bls, CPI_SEPT, BLS_CPI.replace(b"CUUR0000SA0", b"CUUR0000SA1")),  # another series
        (bls, CPI_SEPT, BLS_CPI.replace(b'"period":"M08"', b'"period":"M09"')),  # month twice
        (bls, CPI_SEPT, BLS_CPI.replace(b'"336.940"', b'"3.4e2"')),  # unreadable value
        (noaa, TMAX, b'{"DATE": "2025-07-01"}'),  # not a list
        (noaa, TMAX, NOAA_JULY.replace(b'"31.7"', b'"NaN"')),
        (noaa, TMAX, NOAA_JULY.replace(b"2025-07-02", b"2025-07-01")),  # day twice
        (noaa, TMAX, NOAA_JULY.replace(b'"31.7"', b"NaN")),  # a JSON constant
        (fred, DGS10, b'{"error_code": 400, "error_message": "Bad Request."}'),
        (fred, DGS10, fred_body(("2025-07-02", "2025-07-01", "4.2 6"))),
        (
            fred,
            DGS10,
            fred_body(("2025-07-02", "2025-07-01", "4.26"), ("2025-07-02", "2025-07-01", "4.27")),
        ),
        (fred, DGS10, b"<html>maintenance</html>"),
    ],
)
def test_malformed_responses_are_refused(module: Any, rule: Rulebook, body: bytes) -> None:  # noqa: ANN401
    with pytest.raises(MalformedResponseError):
        module.parse(body, rule)


# ---- requests ------------------------------------------------------------------------------


def test_requests_go_only_to_the_allowed_hosts_and_hide_the_key() -> None:
    for rule, module in [(CPI_SEPT, bls), (TMAX, noaa), (DGS10, fred)]:
        url = module.request_url(rule, "secret-key", t("2026-11-07T00:00:00Z"))
        check_url(url)
        assert "secret-key" not in redact(url)
    assert "realtime_start=2025-07-01" in fred.request_url(DGS10, "k", t("2026-11-07T00:00:00Z"))
    # FRED refuses a real-time start after today, so a not-yet-reached date starts today.
    early = fred.request_url(FED_UPPER, "k", t("2025-09-17T18:00:00Z"))
    assert "realtime_start=2025-09-17" in early
    with pytest.raises(ValueError, match="FRED_API_KEY"):
        fred.request_url(DGS10, None)
    for bad in [
        "http://api.bls.gov/x",
        "https://example.com/x",
        "https://api.bls.gov:8443/x",
        "https://user:pw@api.bls.gov/x",
    ]:
        with pytest.raises(FetchError):
            check_url(bad)
    assert redact("https://h/p?a=1&api_key=s3cret&b=2") == "https://h/p?a=1&api_key=REDACTED&b=2"


# ---- resolve and evidence ------------------------------------------------------------------


def fetcher(body: bytes, retrieved_at: str, status: int = 200) -> Any:  # noqa: ANN401
    def get(url: str) -> Response:
        check_url(url)
        return Response(redact(url), status, body, retrieved_at)

    return get


def test_resolve_waits_proposes_and_voids_at_the_deadline() -> None:
    assert resolve(CPI_SEPT, t("2026-10-15T12:29:59Z"), fetcher=fetcher(BLS_CPI, "x")) == Wait(
        "before the release time"
    )
    p = resolve(
        CPI_SEPT, t("2026-10-15T12:31:00Z"), fetcher=fetcher(BLS_CPI, "2026-10-15T12:31:00Z")
    )
    assert isinstance(p, Proposal)
    assert (p.outcome, p.void_condition, p.evidence["value"]) == ("yes", None, "3.7")
    assert p.evidence["rules_hash"] == CPI_SEPT.hash
    assert p.evidence["market"] == CPI_SEPT.market
    assert p.evidence["retrieved_at"] == "2026-10-15T12:31:00Z"
    # Unavailable before the deadline: wait. At the deadline: void.
    before = resolve(
        CPI_OCT_2025, t("2026-01-13T13:29:59Z"), fetcher=fetcher(BLS_CPI, "2026-01-13T13:29:59Z")
    )
    assert before == Wait("2025-10 published as unavailable")
    void = resolve(
        CPI_OCT_2025, t("2026-11-07T00:00:00Z"), fetcher=fetcher(BLS_CPI, "2026-11-07T00:00:00Z")
    )
    assert isinstance(void, Proposal)
    assert (void.outcome, void.void_condition, void.evidence["value"]) == (
        "void",
        "no_value_by_deadline",
        None,
    )
    # 89 >= 89 (31.7 °C is 89.06 °F); and the FRED markets.
    tmax = resolve(
        TMAX, t("2025-07-03T00:00:00Z"), fetcher=fetcher(NOAA_JULY, "2025-07-03T00:00:00Z")
    )
    assert isinstance(tmax, Proposal)
    assert (tmax.outcome, tmax.evidence["value"]) == ("yes", "89")
    dgs = resolve(
        DGS10,
        t("2025-07-02T00:00:00Z"),
        "k",
        fetcher(fred_body(("2025-07-02", "2025-07-01", "4.26")), "2025-07-02T00:00:00Z"),
    )
    assert isinstance(dgs, Proposal)
    assert (dgs.outcome, dgs.evidence["value"], dgs.evidence["request_url"].count("REDACTED")) == (
        "yes",
        "4.26",
        1,
    )


def test_http_errors_and_unreadable_responses_propose_nothing() -> None:
    with pytest.raises(SourceError):
        resolve(
            CPI_SEPT,
            t("2026-10-16T00:00:00Z"),
            fetcher=fetcher(b"busy", "2026-10-16T00:00:00Z", 503),
        )
    with pytest.raises(MalformedResponseError):
        resolve(CPI_SEPT, t("2026-10-16T00:00:00Z"), fetcher=fetcher(b"{", "2026-10-16T00:00:00Z"))


def test_the_stored_evidence_alone_decides_again(tmp_path: Path) -> None:
    store = EvidenceStore(tmp_path / "evidence")
    for rule, body, at in [
        (CPI_SEPT, BLS_CPI, "2026-10-15T12:31:00Z"),
        (CPI_OCT_2025, BLS_CPI, "2026-11-07T00:00:00Z"),
        (TMAX, NOAA_JULY, "2025-07-03T00:00:00Z"),
    ]:
        p = resolve(rule, t(at), fetcher=fetcher(body, at))
        assert isinstance(p, Proposal)
        assert store.put(p.body) == p.evidence["body_sha256"]
        assert store.put(p.body) == p.evidence["body_sha256"], "storing twice is a no-op"
        assert recompute(p.evidence, rule, store) == (p.outcome, p.void_condition)
    p = resolve(TMAX, t("2025-07-03T00:00:00Z"), fetcher=fetcher(NOAA_JULY, "2025-07-03T00:00:00Z"))
    assert isinstance(p, Proposal)
    with pytest.raises(EvidenceError):
        recompute(p.evidence, CPI_SEPT, store)  # another rulebook
    (tmp_path / "evidence" / p.evidence["body_sha256"]).write_bytes(NOAA_NONE)
    with pytest.raises(EvidenceError, match="does not match"):
        recompute(p.evidence, TMAX, store)
    with pytest.raises(EvidenceError, match="no stored body"):
        store.get("00" * 32)


# ---- the CLI's pass ------------------------------------------------------------------------


class FakeGateway:
    def __init__(self, rules: dict[int, Rulebook], live: set[int], tampered: set[int]) -> None:
        self.rules_by_market = rules
        self.live = live
        self.tampered = tampered
        self.proposals: list[tuple[int, dict[str, Any]]] = []

    def markets(self) -> list[dict[str, Any]]:
        return [{"market": 1, "state": "open"}] + [
            {"market": m, "state": "closed"} for m in sorted(self.rules_by_market)
        ]

    def rules(self, market: int) -> dict[str, Any]:
        r = self.rules_by_market[market]
        journaled = "00" * 32 if market in self.tampered else r.hash
        return {
            "rulebook": r.raw,
            "rules_hash": r.hash,
            "journal": {"listed_seq": 1, "rules_hash": journaled, "matches": journaled == r.hash},
        }

    def resolutions(self, market: int) -> list[dict[str, Any]]:
        return [{"state": "confirmed"}] if market in self.live else [{"state": "withdrawn"}]

    def propose(self, market: int, body: Any) -> dict[str, Any]:  # noqa: ANN401
        self.proposals.append((market, dict(body)))
        return {"resolution": len(self.proposals), "evidence_hash": "x"}


def test_one_pass_proposes_only_where_it_should(tmp_path: Path) -> None:
    rules = {101: CPI_SEPT, 102: CPI_OCT_2025, 103: TMAX, 104: DGS10}
    gw = FakeGateway(rules, live={103}, tampered={104})
    said: list[tuple[str, dict[str, Any]]] = []
    store = EvidenceStore(tmp_path)
    proposed = cli.run_once(
        gw,
        store,
        t("2026-11-07T00:00:00Z"),
        None,
        fetcher(BLS_CPI, "2026-11-07T00:00:00Z"),
        lambda event, **f: said.append((event, f)),
    )
    assert proposed == [101, 102]
    assert [(m, b["outcome"], b.get("void_condition")) for m, b in gw.proposals] == [
        (101, "yes", None),
        (102, "void", "no_value_by_deadline"),
    ]
    assert ("rules_mismatch", {"market": 104}) in said, "a journal mismatch is never resolved"
    assert all(store.get(b["evidence"]["body_sha256"]) == BLS_CPI for _, b in gw.proposals)


def test_keys_round_trip_and_are_private(tmp_path: Path) -> None:
    path = tmp_path / "resolver.key"
    public = cli.keygen(path)
    assert path.stat().st_mode & 0o077 == 0
    assert cli.load_key(path).public_key_b64() == public
    with pytest.raises(FileExistsError):
        cli.keygen(path)
