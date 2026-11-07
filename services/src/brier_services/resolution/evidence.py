"""The evidence store and offline recomputation (R2 in docs/notes/resolution.md).

Raw responses are kept under ``var/evidence/<sha256>`` (git-ignored), written once and checked
on every read. ``recompute`` decides again from a record and its stored bytes alone, with no
network, and must reach the record's outcome.
"""

from __future__ import annotations

import hashlib
import os
import tempfile
from pathlib import Path
from typing import Any

from brier_services.resolution.resolver import SOURCES, decide
from brier_services.rules import Rulebook, instant

DEFAULT_DIR = Path("var/evidence")


class EvidenceError(Exception):
    """A stored body is missing or does not match its hash, or a record does not match."""


class EvidenceStore:
    """Content-addressed raw responses."""

    def __init__(self, root: Path = DEFAULT_DIR) -> None:
        self.root = root

    def put(self, body: bytes) -> str:
        """Store ``body`` (atomically, once) and return its SHA-256."""
        digest = hashlib.sha256(body).hexdigest()
        path = self.root / digest
        if path.exists():
            self.get(digest)  # an existing file must hold exactly these bytes
            return digest
        self.root.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=self.root, prefix=".tmp-")
        try:
            with os.fdopen(fd, "wb") as f:
                f.write(body)
                f.flush()
                os.fsync(f.fileno())
            Path(tmp).replace(path)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise
        return digest

    def get(self, digest: str) -> bytes:
        """The body with this SHA-256; refuses a missing or altered file."""
        path = self.root / digest
        if not path.is_file():
            msg = f"no stored body {digest}"
            raise EvidenceError(msg)
        body = path.read_bytes()
        if hashlib.sha256(body).hexdigest() != digest:
            msg = f"stored body {digest} does not match its hash"
            raise EvidenceError(msg)
        return body


def recompute(
    record: dict[str, Any], rule: Rulebook, store: EvidenceStore
) -> tuple[str, str | None]:
    """The outcome and void condition the stored body gives, decided again offline."""
    if record.get("rules_hash") != rule.hash or record.get("market") != rule.market:
        msg = "the record is for another rulebook"
        raise EvidenceError(msg)
    retrieved = instant(str(record.get("retrieved_at")))
    if retrieved is None:
        msg = "unreadable retrieved_at"
        raise EvidenceError(msg)
    body = store.get(str(record.get("body_sha256")))
    decided = decide(rule, SOURCES[rule.provider].parse(body, rule), retrieved)
    if decided is None:
        msg = "the stored body gives no outcome yet"
        raise EvidenceError(msg)
    return decided[0], decided[1]
