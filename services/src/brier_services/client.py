"""Reference client for the Brier API (SPEC §6; docs/api/openapi.yaml and asyncapi.yaml).

This module is the protocol's executable statement for client authors. It holds the parts
every client needs and gets wrong most often:

- ``signing_payload``: the exact bytes a bot signs for a request.
- ``BookTracker``: a book maintained from a ``book:<market>`` channel. It applies a snapshot,
  then only the delta whose sequence number is the previous plus one. A duplicate or stale
  message is ignored; a gap stops the book (it is never shown half-updated) and asks for a
  resubscription, whose snapshot restores it.
- ``SequenceTracker``: the same gap rule for ``trades:<market>`` and ``account``, which have no
  snapshot: after a gap the client re-reads state over REST.
- ``RestClient``: REST over the standard library, with session tokens or signed requests.

The trackers are pure: they take decoded messages and return what to do, so any transport
can drive them, and the tests drive them with dropped, duplicated, and reordered messages.
"""

from __future__ import annotations

import base64
import hashlib
import json
import urllib.error
import urllib.request
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from enum import Enum
from typing import Any


def signing_payload(timestamp_ms: int, method: str, path_and_query: str, body: bytes) -> bytes:
    """The bytes a signed request's Ed25519 signature covers (openapi.yaml, ``signed``)."""
    digest = hashlib.sha256(body).hexdigest()
    return f"{timestamp_ms}\n{method.upper()}\n{path_and_query}\n{digest}".encode()


class Action(Enum):
    """What the caller should do after handing a message to a tracker."""

    APPLIED = "applied"  # the message was applied; the state is current to its seq
    IGNORED = "ignored"  # a duplicate or stale message, or one before the first snapshot
    RESUBSCRIBE = "resubscribe"  # a gap: unsubscribe and subscribe again for a fresh start
    REFETCH = "refetch"  # messages were lost upstream: re-read the state over REST


class BookTracker:
    """A market's book from its ``book:<market>`` channel."""

    def __init__(self) -> None:
        self.seq: int | None = None  # None until a snapshot, and again after a gap
        self.journal_seq = 0
        self.state = ""
        self.bids: dict[int, int] = {}
        self.asks: dict[int, int] = {}
        self.gaps = 0

    @property
    def synced(self) -> bool:
        """Whether the book is complete and current to ``seq``."""
        return self.seq is not None

    def on_message(self, m: Mapping[str, Any]) -> Action:
        """Apply one decoded book message."""
        seq = int(m["seq"])
        kind = m["type"]
        if kind == "snapshot":
            if self.seq is not None and seq <= self.seq:
                return Action.IGNORED  # a stale snapshot: the book is already newer
            self.bids, self.asks = {}, {}
        elif kind == "delta":
            if self.seq is None or seq <= self.seq:
                return Action.IGNORED
            if seq != self.seq + 1:
                self.seq = None
                self.gaps += 1
                return Action.RESUBSCRIBE
        else:
            raise ValueError(f"not a book message: {kind!r}")
        for side, levels in ((self.bids, m["bids"]), (self.asks, m["asks"])):
            for price, qty in levels:
                if qty == 0:
                    side.pop(price, None)
                else:
                    side[price] = qty
        self.seq = seq
        self.journal_seq = int(m["journal_seq"])
        self.state = str(m["state"])
        return Action.APPLIED

    def best(self) -> tuple[int | None, int | None]:
        """Best bid and best ask, if any (only meaningful while ``synced``)."""
        return (max(self.bids, default=None), min(self.asks, default=None))


@dataclass
class SequenceTracker:
    """Gap detection for a channel without snapshots (``trades:<market>``, ``account``).

    The first message (``subscribed``, or ``resync`` after the server rebuilt its state) sets
    the sequence number. After a gap, or a ``resync``, the caller re-reads over REST and
    discards messages whose ``journal_seq`` is at or below that answer's ``as_of``.
    """

    seq: int | None = None
    gaps: int = 0
    refetched: int = 0  # gaps already answered by a refetch
    messages: list[dict[str, Any]] = field(default_factory=list)

    def on_message(self, m: Mapping[str, Any]) -> Action:
        """Take one decoded channel message."""
        seq = int(m["seq"])
        kind = m["type"]
        lost = self.gaps > self.refetched or (self.seq is not None and seq != self.seq)
        if kind == "resync" or (kind == "subscribed" and lost):
            # The server rebuilt its state, this subscriber lagged and was resubscribed, or it
            # resubscribed after a gap: messages in between are gone.
            self.seq = seq
            self.refetched = self.gaps
            return Action.REFETCH
        if kind == "subscribed":
            self.seq = seq
            return Action.APPLIED
        if self.seq is None or seq <= self.seq:
            return Action.IGNORED
        if seq != self.seq + 1:
            self.seq = None
            self.gaps += 1
            return Action.RESUBSCRIBE
        self.seq = seq
        self.messages.append(dict(m))
        return Action.APPLIED


Signer = Callable[[bytes], bytes]
"""Signs bytes with the client's Ed25519 private key, returning the 64-byte signature."""


class ApiError(Exception):
    """An error answer: HTTP status, stable ``code``, message, and ``seq`` if sequenced."""

    def __init__(self, status: int, body: Mapping[str, Any]) -> None:
        error = body.get("error", {})
        self.status = status
        self.code = str(error.get("code", "unknown"))
        self.seq: int | None = error.get("seq")
        super().__init__(f"{status} {self.code}: {error.get('message', '')}")


class RestClient:
    """The REST API over ``urllib``: a session token, or signed requests with an API key."""

    def __init__(self, base_url: str, timeout: float = 15.0) -> None:
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.token: str | None = None
        self.key: tuple[int, Signer, Callable[[], int]] | None = None

    def use_key(self, key_id: int, sign: Signer, clock_ms: Callable[[], int]) -> None:
        """Sign every later request with this key (``clock_ms`` gives Unix time in ms)."""
        self.key = (key_id, sign, clock_ms)

    def request(self, method: str, path: str, body: Mapping[str, Any] | None = None) -> Any:  # noqa: ANN401 - JSON
        """Send one request; return the decoded JSON answer (None for 204)."""
        data = b"" if body is None else json.dumps(body, separators=(",", ":")).encode()
        headers = {"content-type": "application/json"}
        if self.key is not None:
            key_id, sign, clock_ms = self.key
            ts = clock_ms()
            signature = sign(signing_payload(ts, method, path, data))
            headers |= {
                "brier-key": str(key_id),
                "brier-timestamp": str(ts),
                "brier-signature": base64.b64encode(signature).decode(),
            }
        elif self.token is not None:
            headers["authorization"] = f"Bearer {self.token}"
        if not self.base_url.startswith(("http://", "https://")):
            raise ValueError("base_url must be http or https")
        req = urllib.request.Request(  # noqa: S310 - scheme checked above
            self.base_url + path, data=data or None, method=method, headers=headers
        )
        try:
            with urllib.request.urlopen(req, timeout=self.timeout) as res:  # noqa: S310
                raw = res.read()
        except urllib.error.HTTPError as e:
            raise ApiError(e.code, json.loads(e.read() or b"{}")) from None
        return json.loads(raw) if raw else None

    def login(self, username: str, password: str) -> None:
        """Log in and use the session for later requests."""
        self.token = self.request(
            "POST", "/v1/login", {"username": username, "password": password}
        )["token"]
