"""National Weather Service daily climate reports (CLI) through api.weather.gov: a station's
official daily maximum temperature, in whole degrees Fahrenheit.

Each office issues a city's report several times. A report issued during the day ("VALID
TODAY AS OF ...") is preliminary; the one issued the next morning summarizes the finished
day under "YESTERDAY". Only that second kind counts, and under the first-release rule, only
the first one issued: a later correction is ignored, as for every other series.

Reading takes several requests. The search (``/products?type=CLI&location=...``) lists report
IDs and issue times but not their text, so the reports are opened oldest first until the
first final summary of the observation day is found. The evidence names the deciding report
and the SHA-256 of the search response.

Days are local standard time, as the reports define them.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass
from fractions import Fraction

from brier_services.resolution.fetch import Fetcher, Request, Response
from brier_services.resolution.sources import (
    MalformedResponseError,
    Missing,
    Reading,
    field,
    strict_json,
)
from brier_services.rules import Rulebook

HOST = "api.weather.gov"
MAX_REPORTS = 12  # reports opened per reading; a day has two or three
MONTHS = [
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
]
_SUMMARY = re.compile(r"CLIMATE SUMMARY FOR ([A-Z]+) (\d{1,2}) (\d{4})")
_FINAL_MAX = re.compile(r"TEMPERATURE \(F\)[ \t]*\n[ \t]*YESTERDAY[ \t]*\n[ \t]*MAXIMUM[ \t]+(\S+)")
# The minimum is the line after the maximum in the same YESTERDAY block.
_FINAL_MIN = re.compile(
    r"TEMPERATURE \(F\)[ \t]*\n[ \t]*YESTERDAY[ \t]*\n[ \t]*MAXIMUM[^\n]*\n[ \t]*MINIMUM[ \t]+(\S+)"
)


class SourceError(Exception):
    """The source answered with an HTTP error; try again later."""


@dataclass(frozen=True)
class Read:
    """The deciding response (or the search, if no report decided), the reading, and the
    search response's hash for the evidence."""

    response: Response
    reading: Reading
    index_url: str
    index_sha256: str
    index_body: bytes


def _location(rule: Rulebook) -> str:
    return rule.series.id.split("/")[1]


def _day_words(observation: str) -> tuple[str, str, str]:
    year, month, day = observation.split("-")
    return MONTHS[int(month) - 1], str(int(day)), year


def report_maximum(text: str, observation: str, *, minimum: bool = False) -> Reading | None:
    """The day's maximum (or, with ``minimum``, minimum) from a final report of
    ``observation``; None if the text is not one (a preliminary report, or another day's)."""
    summary = _SUMMARY.search(text)
    if summary is None or summary.groups() != _day_words(observation):
        return None
    found = (_FINAL_MIN if minimum else _FINAL_MAX).search(text)
    if found is None:
        return None  # a "TODAY" report: preliminary
    raw = found.group(1).removesuffix("R")  # R marks a record
    if raw == "M":
        which = "minimum" if minimum else "maximum"
        return Missing(f"the {observation} report gives the {which} as missing")
    if not re.fullmatch(r"-?\d{1,3}", raw):
        msg = f"unreadable {'minimum' if minimum else 'maximum'} {found.group(1)!r}"
        raise MalformedResponseError(msg)
    return Fraction(int(raw))


def _get(fetcher: Fetcher, url: str) -> Response:
    response = fetcher(Request(url))
    if response.status != 200:
        msg = f"{response.url} answered HTTP {response.status}"
        raise SourceError(msg)
    return response


def read(rule: Rulebook, fetcher: Fetcher, now: str) -> Read:
    """Find the first final report of the observation day issued before ``now``."""
    index_url = (
        f"https://{HOST}/products?type=CLI&location={_location(rule)}"
        f"&start={rule.observation}T00:00:00Z&end={now}"
    )
    index = _get(fetcher, index_url)
    reports = field(strict_json(index.body), "@graph", list)
    listed = sorted((field(r, "issuanceTime", str), field(r, "id", str)) for r in reports)
    digest = hashlib.sha256(index.body).hexdigest()
    for _, report_id in listed[:MAX_REPORTS]:
        if not re.fullmatch(r"[0-9a-f-]{36}", report_id):
            msg = f"unexpected report id {report_id!r}"
            raise MalformedResponseError(msg)
        response = _get(fetcher, f"https://{HOST}/products/{report_id}")
        text = field(strict_json(response.body), "productText", str)
        reading = report_maximum(text, rule.observation, minimum=rule.series.id.endswith("/MIN"))
        if reading is not None:
            return Read(response, reading, index.url, digest, index.body)
    return Read(
        index, Missing(f"no final report of {rule.observation} yet"), index.url, digest, index.body
    )
