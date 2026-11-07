"""The registered resolver study (docs/notes/resolution.md): R1 (values) and R2 (evidence
determines the decision).

    python -m brier_services.resolution.study fetch   # download every response into var/study
    python -m brier_services.resolution.study run     # R1 and R2 from the stored bytes

``fetch`` keeps each raw response under ``var/study/raw/<sha256>`` with its URL (credentials
redacted) and retrieval time in ``manifest.json``, and records the R2 decisions computed from the
responses as they arrived (``live_decisions.json``). ``run`` recomputes those decisions in a
fresh process with networking disabled, then computes R1 and writes ``results.json``.

Independence (R1): the resolver side uses the resolvers' own request builders and parsers. The
comparator side is parsed here by separate code that imports nothing from them.

An instance is a period that at least one of the two sources reports a value for; a period in
the window that neither reports (October 2025, weekends, holidays) is listed, not counted.
"""

from __future__ import annotations

import argparse
import calendar
import csv
import datetime as dt
import gzip
import hashlib
import io
import json
import os
import re
import socket
import statistics
import subprocess
import sys
import urllib.parse
import urllib.request
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from decimal import ROUND_HALF_UP, Decimal
from fractions import Fraction
from pathlib import Path
from typing import Any

from brier_services.resolution import bls, fred, noaa
from brier_services.resolution.fetch import Request, fetch, redact
from brier_services.resolution.resolver import decide
from brier_services.resolution.sources import Missing
from brier_services.resolution.values import format_decimal, round_half_up
from brier_services.rules import Provider, Rulebook, find_series, validate

STUDY_DIR = Path("var/study")
MISSING_LIMIT = Fraction(2, 100)
COMPARATOR_HOSTS = frozenset(
    {"api.stlouisfed.org", "home.treasury.gov", "www.federalreserve.gov", "www.ncei.noaa.gov"}
)
MAX_BYTES = 64 * 1024 * 1024

# The registered windows.
MONTHS = [f"{y}-{m:02d}" for y in range(2021, 2027) for m in range(1, 13)]
MONTHS = MONTHS[MONTHS.index("2021-10") : MONTHS.index("2026-09") + 1]
DAYS_2025 = [(dt.date(2025, 1, 1) + dt.timedelta(days=i)).isoformat() for i in range(365)]
FOMC_PAGES = (
    "https://www.federalreserve.gov/monetarypolicy/fomchistorical2019.htm",
    "https://www.federalreserve.gov/monetarypolicy/fomchistorical2020.htm",
    "https://www.federalreserve.gov/monetarypolicy/fomccalendars.htm",
)
FOMC_FIRST, FOMC_LAST = "20190101", "20261031"


# ---- downloads --------------------------------------------------------------------------------


def _download(url: str) -> tuple[bytes, str]:
    """A comparator download: HTTPS to the comparator hosts only, bounded, no credentials kept."""
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != "https" or parts.hostname not in COMPARATOR_HOSTS:
        msg = f"not a comparator URL: {redact(url)}"
        raise ValueError(msg)
    req = urllib.request.Request(url, headers={"User-Agent": "brier-study/1"})  # noqa: S310 - checked
    retrieved = dt.datetime.now(dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")
    with urllib.request.urlopen(req, timeout=60) as res:  # noqa: S310 - checked
        body = res.read(MAX_BYTES + 1)
    if len(body) > MAX_BYTES:
        msg = f"{redact(url)} is larger than {MAX_BYTES} bytes"
        raise ValueError(msg)
    return body, retrieved


@dataclass
class Store:
    root: Path

    @property
    def manifest_path(self) -> Path:
        return self.root / "manifest.json"

    def load(self) -> dict[str, dict[str, Any]]:
        if not self.manifest_path.exists():
            return {}
        manifest: dict[str, dict[str, Any]] = json.loads(self.manifest_path.read_text())
        return manifest

    def save(self, manifest: dict[str, dict[str, Any]]) -> None:
        self.root.mkdir(parents=True, exist_ok=True)
        self.manifest_path.write_text(json.dumps(manifest, indent=1, sort_keys=True) + "\n")

    def put(
        self, manifest: dict[str, dict[str, Any]], name: str, url: str, body: bytes, at: str
    ) -> None:
        digest = hashlib.sha256(body).hexdigest()
        raw = self.root / "raw"
        raw.mkdir(parents=True, exist_ok=True)
        (raw / digest).write_bytes(body)
        manifest[name] = {
            "url": redact(url),
            "sha256": digest,
            "bytes": len(body),
            "retrieved_at": at,
        }

    def body(self, manifest: dict[str, dict[str, Any]], name: str) -> bytes:
        entry = manifest[name]
        data: bytes = (self.root / "raw" / str(entry["sha256"])).read_bytes()
        if hashlib.sha256(data).hexdigest() != entry["sha256"]:
            msg = f"stored {name} does not match its hash"
            raise ValueError(msg)
        return data


# ---- rulebooks for each instance --------------------------------------------------------------


def _instant(d: dt.datetime) -> str:
    return d.strftime("%Y-%m-%dT%H:%M:%SZ")


def _period(observation: str) -> tuple[dt.datetime, dt.datetime]:
    if len(observation) == 7:
        y, m = map(int, observation.split("-"))
        start = dt.datetime(y, m, 1, tzinfo=dt.UTC)
        return start, start + dt.timedelta(days=calendar.monthrange(y, m)[1])
    start = dt.datetime.fromisoformat(observation).replace(tzinfo=dt.UTC)
    return start, start + dt.timedelta(days=1)


def instance_rule(
    provider: str, series: str, observation: str, transform: str, threshold: str
) -> Rulebook:
    """A valid rulebook for one study instance, with a schedule the validator accepts."""
    found = find_series(Provider(provider), series)
    if found is None:
        msg = f"unknown series {series}"
        raise ValueError(msg)
    start, end = _period(observation)
    day = dt.timedelta(days=1)
    if found.live or not found.released_after_period:
        times = (start - 3 * day, start - 2 * day, end + day, end + 30 * day)
    else:
        times = (end + day, end + 2 * day, end + 3 * day, end + 30 * day)
    return validate(
        {
            "schema": 1,
            "market": 1,
            "title": "Study instance",
            "question": "Study instance?",
            "observation": observation,
            "revision": "first_release",
            "void": ["no_value_by_deadline"],
            "source": {"provider": provider, "series": series, "url": found.url},
            "condition": {"transform": transform, "op": ">", "threshold": threshold},
            "schedule": dict(
                zip(("open", "close", "release", "deadline"), map(_instant, times), strict=True)
            ),
        }
    )


# ---- the five market types --------------------------------------------------------------------


@dataclass(frozen=True)
class Kind:
    """One market type: how to read the resolver's values and the comparator's."""

    name: str
    provider: str
    series: str
    transform: str
    precision: int
    periods: list[str]
    resolver_source: str  # manifest name
    resolver: Callable[[bytes, Rulebook], object]
    comparator: Callable[[Store, dict[str, dict[str, Any]]], dict[str, Fraction]]
    # The resolver's value as compared: rounded to the series' precision as the resolver decides
    # on it, except TMAX, compared in Celsius before Brier's conversion and rounding.
    as_compared: Callable[[Fraction], Fraction] | None = None


def _dec(s: str) -> Fraction:
    return Fraction(Decimal(s.strip()))


def _half_up(x: Fraction, places: int) -> Fraction:
    q = Decimal(x.numerator) / Decimal(x.denominator)
    return Fraction(q.quantize(Decimal(1).scaleb(-places), ROUND_HALF_UP))


def _fred_observations(body: bytes) -> dict[str, str]:
    doc = json.loads(body)
    return {o["date"]: o["value"] for o in doc["observations"]}


def _cpi_pct(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    rows = _fred_observations(store.body(manifest, "fred_cpiaucns_pc1"))
    return {d[:7]: _half_up(_dec(v), 1) for d, v in rows.items() if v != "."}


def _cpi_level(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    rows = _fred_observations(store.body(manifest, "fred_cpiaucns_lin"))
    return {d[:7]: _dec(v) for d, v in rows.items() if v != "."}


def _unrate(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    rows = _fred_observations(store.body(manifest, "fred_unrate"))
    return {d[:7]: _dec(v) for d, v in rows.items() if v != "."}


def _treasury(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    reader = csv.DictReader(io.StringIO(store.body(manifest, "treasury_par_2025").decode()))
    out = {}
    for row in reader:
        month, day, year = row["Date"].split("/")
        if row["10 Yr"].strip():
            out[f"{year}-{month}-{day}"] = _dec(row["10 Yr"])
    return out


def _ghcn(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    text = gzip.decompress(store.body(manifest, "ghcn_by_station")).decode()
    out = {}
    for line in text.splitlines():
        fields = line.split(",")
        if len(fields) >= 4 and fields[2] == "TMAX" and fields[1].startswith("2025"):
            d = fields[1]
            out[f"{d[:4]}-{d[4:6]}-{d[6:]}"] = Fraction(int(fields[3]), 10)
    return out


_FRACTION = r"[0-9]+(?:-[0-9]+/[0-9]+)?|[0-9]+/[0-9]+"
_DECISION = re.compile(
    r"decided (?:today )?to (?:maintain|keep|raise|lower|increase|decrease|reduce) "
    r"the target range for the federal funds rate"
    rf"(?: by [0-9/\-]+ percentage points?,?)? (?:at|to) ({_FRACTION}) to ({_FRACTION}) percent"
)
# The Board's pages write fractions with non-breaking and other Unicode hyphens ("1\u20111/2").
_HYPHENS = str.maketrans(
    {"\u2010": "-", "\u2011": "-", "\u2012": "-", "\u2013": "-", "\u00a0": " "}
)


def _mixed(s: str) -> Fraction:
    if "/" not in s:
        return Fraction(int(s))
    whole, _, frac = s.rpartition("-") if "-" in s else ("0", "", s)
    num, den = frac.split("/")
    return int(whole) + Fraction(int(num), int(den))


def statements_without_a_decision(store: Store, manifest: dict[str, dict[str, Any]]) -> list[str]:
    """Statements fetched in which no target-range decision was found (listed in the results, so
    a reader can check each one is not a policy decision)."""
    found = set(_statements(store, manifest))
    out = []
    for name in sorted(n for n in manifest if n.startswith("fomc_statement_")):
        stamp = name[-8:]
        effective = dt.date(int(stamp[:4]), int(stamp[4:6]), int(stamp[6:])) + dt.timedelta(days=1)
        if effective.isoformat() not in found:
            out.append(f"{stamp[:4]}-{stamp[4:6]}-{stamp[6:]}")
    return out


def _statements(store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Fraction]:
    """Effective date (the day after the statement) -> the stated upper limit."""
    out = {}
    for name in sorted(n for n in manifest if n.startswith("fomc_statement_")):
        html = store.body(manifest, name).decode("utf-8", "replace")
        text = re.sub(r"\s+", " ", re.sub(r"<[^>]+>", " ", html).translate(_HYPHENS))
        m = _DECISION.search(text)
        if m is None:
            continue
        stated = dt.date.fromisoformat(f"{name[-8:-4]}-{name[-4:-2]}-{name[-2:]}")
        out[(stated + dt.timedelta(days=1)).isoformat()] = _mixed(m.group(2))
    return out


KINDS = (
    Kind(
        "cpi_12m_change",
        "bls",
        "CUUR0000SA0",
        "pct_change_12m",
        1,
        MONTHS,
        "bls_cpi",
        bls.parse,
        _cpi_pct,
    ),
    Kind(
        "cpi_index_level",
        "bls",
        "CUUR0000SA0",
        "level",
        3,
        MONTHS,
        "bls_cpi",
        bls.parse,
        _cpi_level,
    ),
    Kind(
        "unemployment_rate",
        "bls",
        "LNS14000000",
        "level",
        1,
        MONTHS,
        "bls_unemployment",
        bls.parse,
        _unrate,
    ),
    Kind(
        "fomc_upper_limit",
        "fred",
        "DFEDTARU",
        "level",
        2,
        [],
        "fred_dfedtaru",
        fred.parse,
        _statements,
    ),
    Kind(
        "treasury_10y", "fred", "DGS10", "level", 2, DAYS_2025, "fred_dgs10", fred.parse, _treasury
    ),
    Kind(
        "central_park_tmax_celsius",
        "noaa",
        "USW00094728/TMAX",
        "level",
        0,
        DAYS_2025,
        "ncei_ads_tmax",
        noaa.parse,
        _ghcn,
        lambda f: (f - 32) * Fraction(5, 9),
    ),
)


def _resolver_requests(api_key: str) -> dict[str, Request]:
    """The resolvers' own requests, widened from one period to the study window."""
    out = {
        "bls_cpi": bls.years("CUUR0000SA0", 2020, 2026),
        "bls_unemployment": bls.years("LNS14000000", 2021, 2026),
    }
    dgs10 = fred.request(instance_rule("fred", "DGS10", "2025-01-02", "level", "0"), api_key).url
    out["fred_dgs10"] = Request(
        dgs10.replace("observation_end=2025-01-02", "observation_end=2025-12-31")
        .replace("observation_start=2025-01-02", "observation_start=2025-01-01")
        .replace("&realtime_start=2025-01-02&realtime_end=9999-12-31&output_type=4", "")
    )
    rule = instance_rule("fred", "DFEDTARU", "2019-01-31", "level", "0")
    upper = fred.request(rule, api_key).url
    out["fred_dfedtaru"] = Request(
        upper.replace("observation_end=2019-01-31", "observation_end=2026-11-01").replace(
            "&realtime_start=2019-01-31&realtime_end=9999-12-31&output_type=4", ""
        )
    )
    tmax = noaa.request(instance_rule("noaa", "USW00094728/TMAX", "2025-01-01", "level", "0")).url
    out["ncei_ads_tmax"] = Request(tmax.replace("endDate=2025-01-01", "endDate=2025-12-31"))
    return out


def _comparator_urls(api_key: str) -> dict[str, str]:
    base = "https://api.stlouisfed.org/fred/series/observations?file_type=json&api_key=" + api_key
    window = "&observation_start=2021-10-01&observation_end=2026-09-01"
    return {
        "fred_cpiaucns_pc1": f"{base}&series_id=CPIAUCNS&units=pc1{window}",
        "fred_cpiaucns_lin": f"{base}&series_id=CPIAUCNS{window}",
        "fred_unrate": f"{base}&series_id=UNRATE{window}",
        "treasury_par_2025": "https://home.treasury.gov/resource-center/data-chart-center/interest-rates/daily-treasury-rates.csv/2025/all?type=daily_treasury_yield_curve&field_tdr_date_value=2025&page&_format=csv",
        "ghcn_by_station": "https://www.ncei.noaa.gov/pub/data/ghcn/daily/by_station/USW00094728.csv.gz",
    }


# ---- R2 decisions -----------------------------------------------------------------------------


def _resolver_values(kind: Kind, body: bytes, periods: Iterable[str]) -> dict[str, Fraction]:
    out = {}
    for p in periods:
        reading = kind.resolver(
            body, instance_rule(kind.provider, kind.series, p, kind.transform, "0")
        )
        if isinstance(reading, Fraction):
            out[p] = reading
        elif not isinstance(reading, Missing):
            msg = f"{kind.name} {p}: unexpected reading {reading!r}"
            raise TypeError(msg)
    return out


def _periods(kind: Kind, store: Store, manifest: dict[str, dict[str, Any]]) -> list[str]:
    if kind.periods:
        return kind.periods
    return sorted(_statements(store, manifest))


def decisions(kind: Kind, body: bytes, periods: list[str]) -> dict[str, str]:
    """R2: the decision for every period the resolver reads a value for, against the median
    threshold, from ``body`` alone."""
    values = _resolver_values(kind, body, periods)
    if not values:
        return {}
    median = round_half_up(Fraction(statistics.median(values.values())), kind.precision)
    threshold = format_decimal(median, kind.precision)
    out = {}
    for p in values:
        rule = instance_rule(kind.provider, kind.series, p, kind.transform, threshold)
        decided = decide(rule, values[p], rule.times.release)
        out[p] = "wait" if decided is None else decided[0]
    out["_threshold"] = threshold
    return out


# ---- commands ---------------------------------------------------------------------------------


def fetch_all(root: Path, api_key: str) -> None:
    store = Store(root)
    manifest = store.load()
    live: dict[str, dict[str, str]] = {}
    for name, request in _resolver_requests(api_key).items():
        response = fetch(request)
        if response.status != 200:
            msg = f"{response.url} answered {response.status}"
            raise RuntimeError(msg)
        store.put(manifest, name, request.url, response.body, response.retrieved_at)
        manifest[name]["request_body"] = response.request_body
    for name, url in _comparator_urls(api_key).items():
        body, at = _download(url)
        store.put(manifest, name, url, body, at)
    for page in FOMC_PAGES:
        html, at = _download(page)
        for stamp in sorted(
            set(re.findall(r"monetary(20[0-9]{6})a\.htm", html.decode("utf-8", "replace")))
        ):
            if FOMC_FIRST <= stamp <= FOMC_LAST and f"fomc_statement_{stamp}" not in manifest:
                url = (
                    f"https://www.federalreserve.gov/newsevents/pressreleases/monetary{stamp}a.htm"
                )
                body, at = _download(url)
                store.put(manifest, f"fomc_statement_{stamp}", url, body, at)
    store.save(manifest)
    # R2's first half: the decisions from each response as it arrived (here, the bytes just
    # downloaded, before anything reads them back from disk).
    for kind in KINDS:
        body = store.body(manifest, kind.resolver_source)
        live[kind.name] = decisions(kind, body, _periods(kind, store, manifest))
    (root / "live_decisions.json").write_text(json.dumps(live, indent=1, sort_keys=True) + "\n")


def _block_network() -> None:
    def refuse(*_a: object, **_k: object) -> socket.socket:
        msg = "the R2 recomputation runs without network access"
        raise OSError(msg)

    socket.socket = refuse  # type: ignore[assignment, misc]
    socket.create_connection = refuse


def recompute_offline(root: Path) -> None:
    """R2's second half, in its own process: decide again from the stored bytes only."""
    _block_network()
    store = Store(root)
    manifest = store.load()
    out = {
        k.name: decisions(k, store.body(manifest, k.resolver_source), _periods(k, store, manifest))
        for k in KINDS
    }
    (root / "recomputed_decisions.json").write_text(
        json.dumps(out, indent=1, sort_keys=True) + "\n"
    )


def r1(kind: Kind, store: Store, manifest: dict[str, dict[str, Any]]) -> dict[str, Any]:
    periods = _periods(kind, store, manifest)
    values = _resolver_values(kind, store.body(manifest, kind.resolver_source), periods)
    convert = kind.as_compared or (lambda v: round_half_up(v, kind.precision))
    ours = {p: convert(v) for p, v in values.items()}
    theirs = {p: v for p, v in kind.comparator(store, manifest).items() if p in set(periods)}
    instances = sorted(set(ours) | set(theirs))
    both = [p for p in instances if p in ours and p in theirs]
    mismatches = [
        {"period": p, "resolver": str(ours[p]), "comparator": str(theirs[p])}
        for p in both
        if ours[p] != theirs[p]
    ]
    only_resolver = [p for p in instances if p not in theirs]
    only_comparator = [p for p in instances if p not in ours]
    missing_share = (
        Fraction(len(only_resolver) + len(only_comparator), len(instances))
        if instances
        else Fraction(1)
    )
    verdict = (
        "untested" if missing_share > MISSING_LIMIT else ("refuted" if mismatches else "passed")
    )
    return {
        "instances": len(instances),
        "compared": len(both),
        "mismatches": mismatches,
        "only_resolver": only_resolver,
        "only_comparator": only_comparator,
        "missing_share": float(missing_share),
        "in_window_without_any_value": [p for p in periods if p not in ours and p not in theirs]
        if kind.periods
        else [],
        "verdict": verdict,
    }


def run(root: Path) -> dict[str, Any]:
    subprocess.run(  # noqa: S603 - fixed arguments, this interpreter
        [sys.executable, "-m", "brier_services.resolution.study", "recompute", "--dir", str(root)],
        check=True,
        env=os.environ | {"PYTHONNOUSERSITE": "1"},
    )
    store = Store(root)
    manifest = store.load()
    live = json.loads((root / "live_decisions.json").read_text())
    again = json.loads((root / "recomputed_decisions.json").read_text())
    results: dict[str, Any] = {"r1": {}, "r2": {}, "sources": manifest}
    for kind in KINDS:
        results["r1"][kind.name] = r1(kind, store, manifest)
        a, b = live[kind.name], again[kind.name]
        diffs = sorted(k for k in set(a) | set(b) if a.get(k) != b.get(k))
        results["r2"][kind.name] = {
            "threshold": a.get("_threshold"),
            "decisions": len([k for k in a if not k.startswith("_")]),
            "differences": diffs,
            "verdict": "refuted" if diffs else "passed",
        }
    results["r1"]["fomc_upper_limit"]["statements_without_a_decision"] = (
        statements_without_a_decision(store, manifest)
    )
    (root / "results.json").write_text(json.dumps(results, indent=1, sort_keys=True) + "\n")
    return results


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m brier_services.resolution.study")
    parser.add_argument("command", choices=["fetch", "run", "recompute"])
    parser.add_argument("--dir", type=Path, default=STUDY_DIR)
    args = parser.parse_args(argv)
    if args.command == "fetch":
        key = os.environ.get("FRED_API_KEY")
        if not key:
            print("set FRED_API_KEY (it is in .env)", file=sys.stderr)
            return 2
        fetch_all(args.dir, key)
        return 0
    if args.command == "recompute":
        recompute_offline(args.dir)
        return 0
    results = run(args.dir)
    for name, r in results["r1"].items():
        print(
            f"R1 {name}: {r['verdict']} ({r['compared']} compared, "
            f"{len(r['mismatches'])} mismatches, "
            f"{len(r['only_resolver']) + len(r['only_comparator'])} one-sided)"
        )
    for name, r in results["r2"].items():
        print(
            f"R2 {name}: {r['verdict']} ({r['decisions']} decisions at threshold {r['threshold']})"
        )
    failed = any(r["verdict"] != "passed" for part in ("r1", "r2") for r in results[part].values())
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
