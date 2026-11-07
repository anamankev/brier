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
- ``Ed25519Key``: a bot's key pair (``cryptography``); only the public half is sent.
- ``WsClient``: the WebSocket API (``websockets``): auth, order entry with replies paired by
  ``id``, and channels kept by the trackers, resubscribing by itself after a gap.

The trackers are pure: they take decoded messages and return what to do, so any transport
can drive them, and the tests drive them with dropped, duplicated, and reordered messages.
"""

from __future__ import annotations

import asyncio
import base64
import contextlib
import hashlib
import itertools
import json
import time
import urllib.error
import urllib.request
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from enum import Enum
from types import TracebackType
from typing import Any, Self

from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat
from websockets.asyncio.client import ClientConnection, connect


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


def clock_ms() -> int:
    """Unix time in milliseconds, for ``Brier-Timestamp``."""
    return time.time_ns() // 1_000_000


class Ed25519Key:
    """A bot's signing key. Register ``public_key_b64()`` with ``POST /v1/keys``."""

    def __init__(self, private: Ed25519PrivateKey | None = None) -> None:
        self._private = private or Ed25519PrivateKey.generate()

    def public_key_b64(self) -> str:
        """The 32-byte public key, standard base64, as ``POST /v1/keys`` takes it."""
        raw = self._private.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
        return base64.b64encode(raw).decode()

    def sign(self, payload: bytes) -> bytes:
        """The 64-byte Ed25519 signature of ``payload``."""
        return self._private.sign(payload)

    def ws_auth(self, key_id: int) -> dict[str, Any]:
        """The fields of a WebSocket ``auth`` message signed with this key."""
        ts = clock_ms()
        signature = self.sign(signing_payload(ts, "GET", "/v1/ws", b""))
        return {"key": key_id, "timestamp": ts, "signature": base64.b64encode(signature).decode()}


class WsClient:
    """The WebSocket API: requests with replies, and channels kept current by trackers.

    Use as ``async with WsClient(url) as ws:``. Book channels resubscribe by themselves after
    a gap; ``refetches`` counts the times a snapshot-less channel lost messages, when a real
    client would re-read the state over REST.
    """

    def __init__(self, url: str) -> None:
        self.url = url
        self.books: dict[int, BookTracker] = {}
        self.sequences: dict[str, SequenceTracker] = {}
        self.refetches = 0
        self._ids = itertools.count(1)
        self._waiting: dict[int, asyncio.Future[dict[str, Any]]] = {}
        self._socket: ClientConnection | None = None
        self._reader: asyncio.Task[None] | None = None
        self._background: set[asyncio.Task[Any]] = set()

    async def __aenter__(self) -> Self:
        self._socket = await connect(self.url)
        self._reader = asyncio.create_task(self._read())
        return self

    async def __aexit__(
        self,
        kind: type[BaseException] | None,
        error: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        if self._socket is not None:
            await self._socket.close()
        if self._reader is not None:
            self._reader.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._reader  # a reader error after our own close is surfaced here

    async def request(self, op: str, **fields: Any) -> dict[str, Any]:  # noqa: ANN401 - JSON
        """Send one message and wait for its reply; raise ``ApiError`` if it failed."""
        if self._socket is None:
            raise RuntimeError("use WsClient as an async context manager")
        request_id = next(self._ids)
        reply = asyncio.get_running_loop().create_future()
        self._waiting[request_id] = reply
        await self._socket.send(json.dumps({"op": op, "id": request_id, **fields}))
        answer = await reply
        if not answer["ok"]:
            raise ApiError(0, {"error": answer["error"]})
        result: dict[str, Any] = answer["result"]
        return result

    async def subscribe_book(self, market: int) -> BookTracker:
        """Subscribe to ``book:<market>``; the tracker fills as messages arrive."""
        tracker = self.books.setdefault(market, BookTracker())
        await self.request("subscribe", channel=f"book:{market}")
        return tracker

    async def subscribe(self, channel: str) -> SequenceTracker:
        """Subscribe to ``trades:<market>`` or ``account``."""
        tracker = self.sequences.setdefault(channel, SequenceTracker())
        await self.request("subscribe", channel=channel)
        return tracker

    async def _resubscribe(self, channel: str) -> None:
        await self.request("unsubscribe", channel=channel)
        await self.request("subscribe", channel=channel)

    def _later(self, work: Any) -> None:  # noqa: ANN401 - a coroutine
        task = asyncio.create_task(work)
        self._background.add(task)
        task.add_done_callback(self._background.discard)

    async def _read(self) -> None:
        assert self._socket is not None  # noqa: S101 - set before the reader starts
        async for raw in self._socket:
            m: dict[str, Any] = json.loads(raw)
            if m["type"] == "reply":
                waiting = self._waiting.pop(m["id"], None) if isinstance(m["id"], int) else None
                if waiting is not None and not waiting.done():
                    waiting.set_result(m)
                continue
            channel = str(m["channel"])
            if channel.startswith("book:"):
                action = self.books[int(channel[5:])].on_message(m)
            else:
                action = self.sequences[channel].on_message(m)
            if action is Action.RESUBSCRIBE:
                self._later(self._resubscribe(channel))
            if action in (Action.RESUBSCRIBE, Action.REFETCH) and not channel.startswith("book:"):
                self.refetches += 1
