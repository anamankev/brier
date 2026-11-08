"""Market rulebooks: the Python twin of the Rust ``brier-rules`` crate (SPEC §7, G5 plan).

Resolvers need the same reading of a rulebook as the gateway that listed it, so every rule
here mirrors ``crates/rules/src/lib.rs`` exactly, including the order the checks run in and the
error codes. Two shared files hold the twins together: ``crates/rules/tests/vectors.json``
(inputs, expected errors, canonical forms, hashes) and ``crates/rules/registry.json`` (the
series markets may resolve on). Both test suites read both files.

- ``canonical``: RFC 8785 canonical JSON for the values rulebooks use (no floats; integers of
  at most 2^53 - 1 in magnitude), written out by hand rather than through ``json.dumps``.
- ``validate``: parse and check a rulebook given as decoded JSON, returning it with its
  canonical form and rules hash (SHA-256), or raising ``RuleError`` with a stable code.
- ``load_toml``: read a rulebook file from ``markets/``.
"""

from __future__ import annotations

import calendar
import datetime as dt
import hashlib
import itertools
import re
import tomllib
import unicodedata
from dataclasses import dataclass
from decimal import Decimal
from enum import StrEnum
from pathlib import Path
from typing import Any

SCHEMA = 2  # the newest schema read; schema 1 rulebooks stay valid with the same hash
MIN_SCHEMA = 1
MAX_SLUG_CHARS = 80
MAX_TAGS = 8
MAX_OUTCOME_CHARS = 40
MAX_TITLE_CHARS = 120
MAX_QUESTION_CHARS = 500
DAY = 86_400
MAX_DEADLINE_AFTER_RELEASE = 92 * DAY
MAX_INTEGER_DIGITS = 6
MAX_SAFE_INTEGER = 2**53 - 1
U32_MAX = 2**32 - 1


class RuleError(ValueError):
    """A rulebook was refused; ``code`` matches the Rust ``RuleError::code``."""

    def __init__(self, code: str, detail: str = "") -> None:
        super().__init__(f"rulebook refused: {code}" + (f" ({detail})" if detail else ""))
        self.code = code


class Provider(StrEnum):
    BLS = "bls"
    FRED = "fred"
    NOAA = "noaa"
    NWS = "nws"
    USGS = "usgs"
    SWPC = "swpc"


class Transform(StrEnum):
    LEVEL = "level"
    PCT_CHANGE_12M = "pct_change_12m"


class Op(StrEnum):
    ABOVE = ">"
    AT_LEAST = ">="
    BELOW = "<"
    AT_MOST = "<="
    BETWEEN = "between"


class Review(StrEnum):
    MANUAL = "manual"
    AUTOMATIC = "automatic"


class PeriodKind(StrEnum):
    MONTH = "month"
    DAY = "day"


VOID_CONDITIONS = ("no_value_by_deadline", "source_discontinued")  # in their sort order


@dataclass(frozen=True)
class Measure:
    transform: Transform
    precision: int
    unit: str


@dataclass(frozen=True)
class Series:
    provider: Provider
    id: str
    name: str
    url: str
    kind: PeriodKind
    measures: tuple[Measure, ...]
    category: str
    live: bool
    released_after_period: bool
    machine_checkable: bool = True


_NOAA_URL = (
    "https://www.ncei.noaa.gov/access/services/data/v1"
    "?dataset=daily-summaries&stations=USW00094728&dataTypes=TMAX"
)

REGISTRY: tuple[Series, ...] = (
    Series(
        Provider.BLS,
        "CUUR0000SA0",
        "CPI-U, U.S. city average, all items, not seasonally adjusted",
        "https://data.bls.gov/timeseries/CUUR0000SA0",
        PeriodKind.MONTH,
        (
            Measure(Transform.PCT_CHANGE_12M, 1, "percent"),
            Measure(Transform.LEVEL, 3, "index points (1982-84 = 100)"),
        ),
        "economics",
        live=False,
        released_after_period=True,
    ),
    Series(
        Provider.BLS,
        "LNS14000000",
        "Unemployment rate, seasonally adjusted",
        "https://data.bls.gov/timeseries/LNS14000000",
        PeriodKind.MONTH,
        (Measure(Transform.LEVEL, 1, "percent"),),
        "economics",
        live=False,
        released_after_period=True,
    ),
    Series(
        Provider.FRED,
        "DFEDTARU",
        "Federal funds target range, upper limit",
        "https://fred.stlouisfed.org/series/DFEDTARU",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 2, "percent"),),
        "rates",
        live=False,
        released_after_period=False,
    ),
    Series(
        Provider.FRED,
        "DGS10",
        "10-year Treasury constant maturity yield",
        "https://fred.stlouisfed.org/series/DGS10",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 2, "percent"),),
        "rates",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NOAA,
        "USW00094728/TMAX",
        "Daily maximum temperature, New York Central Park (GHCN-Daily USW00094728)",
        _NOAA_URL,
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/NYC",
        "Daily maximum temperature, New York Central Park (NWS daily climate report CLINYC)",
        "https://api.weather.gov/products/types/CLI/locations/NYC",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/ORD",
        "Daily maximum temperature, Chicago O'Hare (NWS daily climate report CLIORD)",
        "https://api.weather.gov/products/types/CLI/locations/ORD",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/LAX",
        "Daily maximum temperature, Los Angeles International Airport"
        " (NWS daily climate report CLILAX)",
        "https://api.weather.gov/products/types/CLI/locations/LAX",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/MIA",
        "Daily maximum temperature, Miami (NWS daily climate report CLIMIA)",
        "https://api.weather.gov/products/types/CLI/locations/MIA",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/BOS",
        "Daily maximum temperature, Boston (NWS daily climate report CLIBOS)",
        "https://api.weather.gov/products/types/CLI/locations/BOS",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/SEA",
        "Daily maximum temperature, Seattle-Tacoma Airport (NWS daily climate report CLISEA)",
        "https://api.weather.gov/products/types/CLI/locations/SEA",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/DEN",
        "Daily maximum temperature, Denver (NWS daily climate report CLIDEN)",
        "https://api.weather.gov/products/types/CLI/locations/DEN",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/ATL",
        "Daily maximum temperature, Atlanta (NWS daily climate report CLIATL)",
        "https://api.weather.gov/products/types/CLI/locations/ATL",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/PHX",
        "Daily maximum temperature, Phoenix (NWS daily climate report CLIPHX)",
        "https://api.weather.gov/products/types/CLI/locations/PHX",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/DCA",
        "Daily maximum temperature, Washington National (NWS daily climate report CLIDCA)",
        "https://api.weather.gov/products/types/CLI/locations/DCA",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/AUS",
        "Daily maximum temperature, Austin-Bergstrom (NWS daily climate report CLIAUS)",
        "https://api.weather.gov/products/types/CLI/locations/AUS",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.NWS,
        "CLI/DFW",
        "Daily maximum temperature, Dallas/Fort Worth (NWS daily climate report CLIDFW)",
        "https://api.weather.gov/products/types/CLI/locations/DFW",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "°F"),),
        "weather",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.USGS,
        "WORLD/M5",
        "Earthquakes of magnitude 5.0 or more worldwide, by UTC day (USGS ComCat)",
        "https://earthquake.usgs.gov/earthquakes/map/",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "earthquakes"),),
        "science",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.USGS,
        "WORLD/M6",
        "Earthquakes of magnitude 6.0 or more worldwide, by UTC day (USGS ComCat)",
        "https://earthquake.usgs.gov/earthquakes/map/",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 0, "earthquakes"),),
        "science",
        live=True,
        released_after_period=True,
    ),
    Series(
        Provider.SWPC,
        "KP/MAX",
        "Daily maximum planetary K-index, by UTC day (NOAA SWPC estimated 3-hour values)",
        "https://www.swpc.noaa.gov/products/planetary-k-index",
        PeriodKind.DAY,
        (Measure(Transform.LEVEL, 2, "Kp"),),
        "science",
        live=True,
        released_after_period=True,
    ),
)


# Canonical JSON (RFC 8785 subset) -----------------------------------------------------------

_SHORT_ESCAPES = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\t": "\\t", "\n": "\\n"}
_SHORT_ESCAPES |= {"\f": "\\f", "\r": "\\r"}


def _string(s: str) -> str:
    out = ['"']
    for c in s:
        if c in _SHORT_ESCAPES:
            out.append(_SHORT_ESCAPES[c])
        elif ord(c) < 0x20:
            out.append(f"\\u{ord(c):04x}")
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def canonical(value: object) -> str:
    """The canonical JSON text of ``value``; raises ``RuleError("canonical")`` for floats or
    integers beyond 2^53 - 1 in magnitude."""
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        if abs(value) > MAX_SAFE_INTEGER:
            raise RuleError("canonical", f"integer out of range: {value}")
        return str(value)
    if isinstance(value, str):
        return _string(value)
    if isinstance(value, list | tuple):
        return "[" + ",".join(canonical(v) for v in value) + "]"
    if isinstance(value, dict):
        keys = sorted(value, key=lambda k: str(k).encode("utf-16-be"))
        return "{" + ",".join(_string(str(k)) + ":" + canonical(value[k]) for k in keys) + "}"
    raise RuleError("canonical", f"no canonical form for {type(value).__name__}")


# Strict parsing, as serde with deny_unknown_fields reads it ----------------------------------


def _malformed(detail: str) -> RuleError:
    return RuleError("malformed", detail)


def _object(
    value: object, fields: tuple[str, ...], where: str, optional: tuple[str, ...] = ()
) -> dict[str, Any]:
    """The object, with its optional fields dropped when absent or null (serde reads a null
    ``Option`` as absent, and the canonical form omits it)."""
    if not isinstance(value, dict):
        raise _malformed(f"{where} must be an object")
    for key in value:
        if key not in fields and key not in optional:
            raise _malformed(f"unknown field {where}.{key}")
    for key in fields:
        if key not in value:
            raise _malformed(f"missing field {where}.{key}")
    return {k: v for k, v in value.items() if not (k in optional and v is None)}


def _bool(value: object, where: str) -> bool:
    if not isinstance(value, bool):
        raise _malformed(f"{where} must be a boolean")
    return value


_SLUG = re.compile(r"[a-z0-9]+(?:-[a-z0-9]+)*")


def _slug_ok(s: str) -> bool:
    return len(s) <= MAX_SLUG_CHARS and _SLUG.fullmatch(s) is not None


def _decimal_lt(a: str, b: str) -> bool:
    """Exact comparison of two canonical decimals."""
    return Decimal(a) < Decimal(b)


def _str(value: object, where: str) -> str:
    if not isinstance(value, str):
        raise _malformed(f"{where} must be a string")
    return value


def _u32(value: object, where: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= U32_MAX:
        raise _malformed(f"{where} must be an integer in [0, 2^32 - 1]")
    return value


def _enum[E: StrEnum](cls: type[E], value: object, where: str) -> E:
    try:
        return cls(_str(value, where))
    except ValueError:
        raise _malformed(f"unknown variant for {where}: {value!r}") from None


# Checks ---------------------------------------------------------------------------------------


def _text_ok(s: str, max_chars: int) -> bool:
    return (
        s != ""
        and s.strip() == s
        and len(s) <= max_chars
        and not any(unicodedata.category(c) == "Cc" for c in s)
    )


_DECIMAL = re.compile(r"-?([0-9]+)(?:\.([0-9]+))?")


def _threshold_ok(s: str, precision: int) -> bool:
    m = _DECIMAL.fullmatch(s)
    if m is None:
        return False
    integer, frac = m.group(1), m.group(2)
    int_ok = len(integer) <= MAX_INTEGER_DIGITS and (integer == "0" or not integer.startswith("0"))
    frac_ok = frac is None or len(frac) <= precision
    zero = set(integer) == {"0"} and (frac is None or set(frac) == {"0"})
    return int_ok and frac_ok and not (s.startswith("-") and zero)


_INSTANT = re.compile(r"([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})Z")
_MONTH = re.compile(r"([0-9]{4})-([0-9]{2})")
_DAY = re.compile(r"([0-9]{4})-([0-9]{2})-([0-9]{2})")


def _year_ok(year: int) -> bool:
    return 2000 <= year <= 2100


def instant(s: str) -> int | None:
    """``YYYY-MM-DDTHH:MM:SSZ`` (years 2000-2100) as Unix seconds, or None."""
    m = _INSTANT.fullmatch(s)
    if m is None or not _year_ok(int(m.group(1))):
        return None
    try:
        year, month, day, hour, minute, second = (int(g) for g in m.groups())
        t = dt.datetime(year, month, day, hour, minute, second, tzinfo=dt.UTC)
    except ValueError:
        return None
    return calendar.timegm(t.utctimetuple())


def period(kind: PeriodKind, s: str) -> tuple[int, int] | None:
    """A period's start and exclusive end as Unix seconds at UTC midnight, or None."""
    try:
        if kind is PeriodKind.MONTH:
            m = _MONTH.fullmatch(s)
            if m is None or not _year_ok(int(m.group(1))):
                return None
            year, month = int(m.group(1)), int(m.group(2))
            start = dt.date(year, month, 1)
            days = calendar.monthrange(year, month)[1]
        else:
            m = _DAY.fullmatch(s)
            if m is None or not _year_ok(int(m.group(1))):
                return None
            start = dt.date(int(m.group(1)), int(m.group(2)), int(m.group(3)))
            days = 1
    except ValueError:
        return None
    first = calendar.timegm(start.timetuple())
    return first, first + days * DAY


def find_series(provider: Provider, series_id: str) -> Series | None:
    """The registry entry for a provider and series, if any."""
    return next((s for s in REGISTRY if s.provider is provider and s.id == series_id), None)


@dataclass(frozen=True)
class Times:
    open: int
    close: int
    release: int
    deadline: int


@dataclass(frozen=True)
class Rulebook:
    """A validated rulebook: the decoded JSON, what it refers to, its canonical form and hash."""

    raw: dict[str, Any]
    market: int
    provider: Provider
    series: Series
    measure: Measure
    observation: str
    op: Op
    threshold: str
    upper: str | None
    event: dict[str, Any] | None
    review: Review | None
    times: Times
    void: tuple[str, ...]
    canonical: str
    hash: str  # lowercase hex SHA-256 of ``canonical``


def validate(value: object) -> Rulebook:
    """Parse and check a rulebook; raises ``RuleError`` for the first rule it breaks."""
    top = _object(
        value,
        (
            "schema",
            "market",
            "title",
            "question",
            "source",
            "observation",
            "condition",
            "schedule",
            "revision",
            "void",
        ),
        "rulebook",
        optional=("event", "review"),
    )
    schema = _u32(top["schema"], "schema")
    market = _u32(top["market"], "market")
    title = _str(top["title"], "title")
    question = _str(top["question"], "question")
    source = _object(top["source"], ("provider", "series", "url"), "source")
    provider = _enum(Provider, source["provider"], "source.provider")
    series_id = _str(source["series"], "source.series")
    url = _str(source["url"], "source.url")
    observation = _str(top["observation"], "observation")
    condition = _object(
        top["condition"], ("transform", "op", "threshold"), "condition", optional=("upper",)
    )
    top = {**top, "condition": condition}
    transform = _enum(Transform, condition["transform"], "condition.transform")
    op = _enum(Op, condition["op"], "condition.op")
    threshold = _str(condition["threshold"], "condition.threshold")
    upper = _str(condition["upper"], "condition.upper") if "upper" in condition else None
    schedule = _object(top["schedule"], ("open", "close", "release", "deadline"), "schedule")
    times_raw = [
        _str(schedule[k], f"schedule.{k}") for k in ("open", "close", "release", "deadline")
    ]
    if _str(top["revision"], "revision") != "first_release":
        raise _malformed(f"unknown variant for revision: {top['revision']!r}")
    void_raw = top["void"]
    if not isinstance(void_raw, list):
        raise _malformed("void must be an array")
    void = tuple(_str(v, "void[]") for v in void_raw)
    for v in void:
        if v not in VOID_CONDITIONS:
            raise _malformed(f"unknown variant for void: {v!r}")
    event = None
    if "event" in top:
        event = _object(
            top["event"], ("slug", "title", "tags", "exclusive", "outcome"), "event", ("series",)
        )
        top = {**top, "event": event}
        for k in ("slug", "title", "outcome"):
            _str(event[k], f"event.{k}")
        if "series" in event:
            _str(event["series"], "event.series")
        if not isinstance(event["tags"], list):
            raise _malformed("event.tags must be an array")
        for t in event["tags"]:
            _str(t, "event.tags[]")
        _bool(event["exclusive"], "event.exclusive")
    review = _enum(Review, top["review"], "review") if "review" in top else None

    if not MIN_SCHEMA <= schema <= SCHEMA:
        raise RuleError("unsupported_schema")
    has_v2 = (event is not None, review is not None)
    if (schema == 1 and (has_v2 != (False, False) or op is Op.BETWEEN)) or (
        schema == 2 and has_v2 != (True, True)
    ):
        raise RuleError("schema_fields")
    if market == 0:
        raise RuleError("market_id")
    if not _text_ok(title, MAX_TITLE_CHARS):
        raise RuleError("title")
    if not _text_ok(question, MAX_QUESTION_CHARS):
        raise RuleError("question")
    series = find_series(provider, series_id)
    if series is None:
        raise RuleError("unknown_series")
    if url != series.url:
        raise RuleError("source_url")
    measure = next((m for m in series.measures if m.transform is transform), None)
    if measure is None:
        raise RuleError("transform")
    if not _threshold_ok(threshold, measure.precision):
        raise RuleError("threshold")
    if op is Op.BETWEEN:
        if not (
            upper is not None
            and _threshold_ok(upper, measure.precision)
            and _decimal_lt(threshold, upper)
        ):
            raise RuleError("range")
    elif upper is not None:
        raise RuleError("range")
    bounds = period(series.kind, observation)
    if bounds is None:
        raise RuleError("observation")
    parsed = [instant(t) for t in times_raw]
    if any(p is None for p in parsed):
        raise RuleError("time")
    open_, close, release, deadline = (p for p in parsed if p is not None)
    times = Times(open_, close, release, deadline)
    if not (open_ < close < release < deadline):
        raise RuleError("schedule_order")
    if series.live and close > bounds[0]:
        raise RuleError("close_after_observable")
    if series.released_after_period and release < bounds[1]:
        raise RuleError("release_before_period_end")
    if deadline - release > MAX_DEADLINE_AFTER_RELEASE:
        raise RuleError("deadline_too_far")
    order = [VOID_CONDITIONS.index(v) for v in void]
    if any(a >= b for a, b in itertools.pairwise(order)) or ("no_value_by_deadline" not in void):
        raise RuleError("void_conditions")
    if event is not None and not (
        _slug_ok(event["slug"])
        and _text_ok(event["title"], MAX_TITLE_CHARS)
        and _text_ok(event["outcome"], MAX_OUTCOME_CHARS)
        and len(event["tags"]) <= MAX_TAGS
        and all(_slug_ok(t) for t in event["tags"])
        and all(a < b for a, b in itertools.pairwise(event["tags"]))
        and ("series" not in event or _slug_ok(event["series"]))
    ):
        raise RuleError("event")
    if review is Review.AUTOMATIC and not series.machine_checkable:
        raise RuleError("review")
    text = canonical(top)
    return Rulebook(
        raw=top,
        market=market,
        provider=provider,
        series=series,
        measure=measure,
        observation=observation,
        op=op,
        threshold=threshold,
        upper=upper,
        event=event,
        review=review,
        times=times,
        void=void,
        canonical=text,
        hash=hashlib.sha256(text.encode()).hexdigest(),
    )


def load_toml(path: Path) -> Rulebook:
    """Read and validate a rulebook file (TOML, the same fields as the JSON form)."""
    with path.open("rb") as f:
        return validate(tomllib.load(f))
