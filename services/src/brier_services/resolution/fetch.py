"""HTTPS fetches for resolvers: a fixed list of hosts, no redirects, a size limit, and a
timeout. The request URL kept in evidence has every credential replaced by ``REDACTED``."""

from __future__ import annotations

import datetime as dt
import urllib.error
import urllib.parse
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from email.message import Message
from typing import IO

ALLOWED_HOSTS = frozenset(
    {"api.bls.gov", "api.stlouisfed.org", "www.ncei.noaa.gov", "api.weather.gov"}
)
MAX_BODY_BYTES = 4 * 1024 * 1024
TIMEOUT_SECONDS = 30.0
SECRET_PARAMS = frozenset({"api_key", "registrationkey"})
USER_AGENT = "brier-resolver/1"


class FetchError(Exception):
    """The request was refused before sending, or the response could not be read."""


@dataclass(frozen=True)
class Request:
    """A GET, or a POST with a JSON body."""

    url: str
    body: bytes | None = None


@dataclass(frozen=True)
class Response:
    """One source response, as kept in evidence."""

    url: str  # credentials redacted
    status: int
    body: bytes
    retrieved_at: str  # YYYY-MM-DDTHH:MM:SSZ
    request_body: str | None = None  # the JSON a POST sent


Fetcher = Callable[[Request], Response]


def redact(url: str) -> str:
    """``url`` with the value of every credential parameter replaced by ``REDACTED``."""
    parts = urllib.parse.urlsplit(url)
    query = [
        (k, "REDACTED" if k.lower() in SECRET_PARAMS else v)
        for k, v in urllib.parse.parse_qsl(parts.query, keep_blank_values=True)
    ]
    return urllib.parse.urlunsplit(parts._replace(query=urllib.parse.urlencode(query, safe="/:")))


def check_url(url: str) -> None:
    """Refuse anything but HTTPS to an allowed host on the default port."""
    parts = urllib.parse.urlsplit(url)
    if parts.scheme != "https" or parts.hostname not in ALLOWED_HOSTS or parts.port is not None:
        msg = f"refusing to fetch {redact(url)}: only HTTPS to {sorted(ALLOWED_HOSTS)}"
        raise FetchError(msg)
    if parts.username is not None or parts.password is not None:
        msg = "refusing a URL with credentials in it"
        raise FetchError(msg)


class _NoRedirects(urllib.request.HTTPRedirectHandler):
    def redirect_request(
        self,
        req: urllib.request.Request,
        fp: IO[bytes],
        code: int,
        msg: str,
        headers: Message,
        newurl: str,
    ) -> urllib.request.Request | None:
        raise FetchError(f"refusing a redirect ({code}) to {redact(newurl)}")


_OPENER = urllib.request.build_opener(_NoRedirects)


def now_instant() -> str:
    """The current UTC time as ``YYYY-MM-DDTHH:MM:SSZ``."""
    return dt.datetime.now(dt.UTC).strftime("%Y-%m-%dT%H:%M:%SZ")


def _read_bounded(stream: IO[bytes]) -> bytes:
    body = stream.read(MAX_BODY_BYTES + 1)
    if len(body) > MAX_BODY_BYTES:
        msg = f"the response is larger than {MAX_BODY_BYTES} bytes"
        raise FetchError(msg)
    return body


def fetch(request: Request) -> Response:
    """Send ``request``. An HTTP error status is a response (kept as evidence), not an exception."""
    url = request.url
    check_url(url)
    retrieved_at = now_instant()
    headers = {"User-Agent": USER_AGENT}
    if request.body is not None:
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=request.body, headers=headers)  # noqa: S310 - scheme and host checked above
    sent = None if request.body is None else request.body.decode()
    try:
        with _OPENER.open(req, timeout=TIMEOUT_SECONDS) as res:
            return Response(redact(url), res.status, _read_bounded(res), retrieved_at, sent)
    except urllib.error.HTTPError as e:
        return Response(redact(url), e.code, _read_bounded(e), retrieved_at, sent)
    except (urllib.error.URLError, TimeoutError, OSError) as e:
        msg = f"fetching {redact(url)} failed: {e}"
        raise FetchError(msg) from e
