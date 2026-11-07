"""The reference client's gap handling, against injected faults (SPEC §6).

A model server publishes a book channel exactly as the gateway does: a snapshot, then deltas
carrying the new totals of the levels each change touched, with consecutive sequence numbers.
The delivered stream then has messages dropped, duplicated, and swapped. The client must
never show a book that differs from the server's book at the client's sequence number, and
must end equal to the server's final book.
"""

from __future__ import annotations

import base64
import hashlib
import http.server
import json
import threading
from typing import Any, ClassVar

import pytest
from hypothesis import given, settings
from hypothesis import strategies as st

from brier_services.client import (
    Action,
    ApiError,
    BookTracker,
    RestClient,
    SequenceTracker,
    signing_payload,
)

Level = tuple[bool, int, int]  # (bid?, price, new total; 0 removes the level)


class ModelFeed:
    """A book channel: every published state, by sequence number."""

    def __init__(self) -> None:
        self.seq = 0
        self.bids: dict[int, int] = {}
        self.asks: dict[int, int] = {}
        self.history: dict[int, tuple[dict[int, int], dict[int, int]]] = {0: ({}, {})}

    def snapshot(self, seq: int) -> dict[str, Any]:
        bids, asks = self.history[seq]
        return {
            "type": "snapshot",
            "seq": seq,
            "journal_seq": seq,
            "state": "open",
            "bids": sorted(bids.items(), reverse=True),
            "asks": sorted(asks.items()),
        }

    def change(self, levels: list[Level]) -> dict[str, Any]:
        for bid, price, qty in levels:
            side = self.bids if bid else self.asks
            if qty == 0:
                side.pop(price, None)
            else:
                side[price] = qty
        self.seq += 1
        self.history[self.seq] = (dict(self.bids), dict(self.asks))
        touched = {(bid, price) for bid, price, _ in levels}
        return {
            "type": "delta",
            "seq": self.seq,
            "journal_seq": self.seq,
            "state": "open",
            "bids": [[p, self.bids.get(p, 0)] for bid, p in sorted(touched) if bid],
            "asks": [[p, self.asks.get(p, 0)] for bid, p in sorted(touched) if not bid],
        }


levels = st.lists(
    st.tuples(st.booleans(), st.integers(1, 99), st.integers(0, 50)), min_size=1, max_size=3
)
faults = st.sampled_from(["ok", "ok", "ok", "ok", "drop", "duplicate", "swap"])


def deliver(messages: list[dict[str, Any]], plan: list[str]) -> list[tuple[int, dict[str, Any]]]:
    """Apply the fault plan; each delivered message keeps the index it was published at."""
    out = [(i, m) for i, m in enumerate(messages)]
    result: list[tuple[int, dict[str, Any]]] = []
    i = 0
    while i < len(out):
        fault = plan[i % len(plan)]
        if fault == "drop":
            i += 1
            continue
        if fault == "duplicate":
            result += [out[i], out[i]]
        elif fault == "swap" and i + 1 < len(out):
            result += [out[i + 1], out[i]]
            i += 1
        else:
            result.append(out[i])
        i += 1
    return result


def check(tracker: BookTracker, feed: ModelFeed) -> None:
    assert tracker.seq is not None
    assert (tracker.bids, tracker.asks) == feed.history[tracker.seq]


@settings(max_examples=400, deadline=None)
@given(changes=st.lists(levels, min_size=1, max_size=120), plan=st.lists(faults, min_size=1))
def test_the_book_is_never_wrong_and_ends_right(
    changes: list[list[Level]], plan: list[str]
) -> None:
    feed = ModelFeed()
    messages = [feed.change(c) for c in changes]
    tracker = BookTracker()
    assert tracker.on_message(feed.snapshot(0)) is Action.APPLIED
    published_up_to = 0
    for index, m in deliver(messages, plan):
        published_up_to = max(published_up_to, index + 1)  # the server has sent this far
        action = tracker.on_message(m)
        if action is Action.RESUBSCRIBE:
            # The resubscription's snapshot is the server's state at that moment.
            assert not tracker.synced
            assert tracker.on_message(feed.snapshot(published_up_to)) is Action.APPLIED
        if tracker.synced:
            check(tracker, feed)
    if tracker.seq != feed.seq:  # the last messages were dropped: nothing showed a gap yet
        tracker.on_message(feed.snapshot(feed.seq))
    check(tracker, feed)
    assert tracker.seq == feed.seq


@given(changes=st.lists(levels, min_size=1, max_size=60))
def test_a_clean_stream_needs_no_resubscription(changes: list[list[Level]]) -> None:
    feed = ModelFeed()
    tracker = BookTracker()
    tracker.on_message(feed.snapshot(0))
    for c in changes:
        assert tracker.on_message(feed.change(c)) is Action.APPLIED
        check(tracker, feed)
    assert tracker.gaps == 0


def test_a_gap_stops_the_book_until_a_newer_snapshot() -> None:
    feed = ModelFeed()
    tracker = BookTracker()
    tracker.on_message(feed.snapshot(0))
    d1, d2, d3 = (feed.change([(True, 40, n)]) for n in (1, 2, 3))
    assert tracker.on_message(d1) is Action.APPLIED
    assert tracker.on_message(d3) is Action.RESUBSCRIBE
    assert tracker.on_message(d2) is Action.IGNORED, "nothing applies until a snapshot"
    assert tracker.on_message(feed.snapshot(3)) is Action.APPLIED
    assert tracker.on_message(feed.snapshot(2)) is Action.IGNORED, "a stale snapshot"
    assert tracker.bids == {40: 3}
    assert tracker.best() == (40, None)


def test_channels_without_snapshots_ask_for_a_refetch_after_lost_messages() -> None:
    t = SequenceTracker()
    assert t.on_message({"type": "subscribed", "seq": 5}) is Action.APPLIED
    assert t.on_message({"type": "trade", "seq": 6}) is Action.APPLIED
    assert t.on_message({"type": "trade", "seq": 6}) is Action.IGNORED
    assert t.on_message({"type": "trade", "seq": 8}) is Action.RESUBSCRIBE
    assert t.on_message({"type": "subscribed", "seq": 20}) is Action.REFETCH
    assert t.on_message({"type": "trade", "seq": 21}) is Action.APPLIED
    assert t.on_message({"type": "resync", "seq": 22}) is Action.REFETCH
    assert [m["seq"] for m in t.messages] == [6, 21]


def test_the_signed_payload_matches_the_gateway() -> None:
    # The same vector as the gateway's own test (crates/gateway/src/auth.rs).
    assert signing_payload(1_700_000_000_000, "post", "/v1/orders", b"{}") == (
        b"1700000000000\nPOST\n/v1/orders\n"
        b"44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    )


STUB_SESSION = "session-from-stub"  # what the stub server hands out; not a secret


class _Stub(http.server.BaseHTTPRequestHandler):
    """Records each request; answers 422 with a sequenced rejection for /v1/orders."""

    seen: ClassVar[list[tuple[str, str, dict[str, str], bytes]]] = []

    def _answer(self) -> None:
        body = self.rfile.read(int(self.headers.get("content-length") or 0))
        headers = {k.lower(): v for k, v in self.headers.items()}
        _Stub.seen.append((self.command, self.path, headers, body))
        payload: dict[str, object]
        if self.path == "/v1/orders":
            payload = {"error": {"code": "insufficient_collateral", "message": "no", "seq": 9}}
            status = 422
        else:
            payload, status = {"token": STUB_SESSION, "ok": True}, 200
        raw = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    do_GET = do_POST = _answer

    def log_message(self, format: str, *args: object) -> None:
        return


def test_the_rest_client_signs_and_maps_errors() -> None:
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), _Stub)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        client = RestClient(f"http://127.0.0.1:{server.server_address[1]}")
        client.login("alice", "a long enough password")
        assert client.token == STUB_SESSION
        client.request("GET", "/v1/portfolio")
        assert _Stub.seen[-1][2]["authorization"] == f"Bearer {STUB_SESSION}"

        signed: list[bytes] = []

        def sign(payload: bytes) -> bytes:
            signed.append(payload)
            return hashlib.sha512(payload).digest()  # stands in for Ed25519: 64 bytes

        client.use_key(7, sign, lambda: 1_700_000_000_000)
        with pytest.raises(ApiError) as rejected:
            client.request("POST", "/v1/orders", {"market": 1, "side": "buy", "price": 5, "qty": 1})
        assert (rejected.value.status, rejected.value.code, rejected.value.seq) == (
            422,
            "insufficient_collateral",
            9,
        )
        method, path, headers, body = _Stub.seen[-1]
        assert signed == [signing_payload(1_700_000_000_000, method, path, body)]
        assert headers["brier-key"] == "7"
        assert headers["brier-timestamp"] == "1700000000000"
        assert base64.b64decode(headers["brier-signature"]) == hashlib.sha512(signed[0]).digest()
        assert "authorization" not in headers, "a signed request carries no session"
    finally:
        server.shutdown()


def test_totp_codes_match_rfc_6238() -> None:
    import base64

    from brier_services.client import totp_code

    secret = base64.b32encode(b"12345678901234567890").decode().rstrip("=")
    for t, eight in [(59, "94287082"), (1_111_111_109, "07081804"), (2_000_000_000, "69279037")]:
        assert totp_code(secret, t // 30) == eight[2:]
