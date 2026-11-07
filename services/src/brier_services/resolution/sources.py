"""What every source module provides, and the reading it returns."""

from __future__ import annotations

import json
from dataclasses import dataclass
from fractions import Fraction
from typing import Any


class MalformedResponseError(ValueError):
    """The response is not what the source documents; no outcome can be read from it."""


@dataclass(frozen=True)
class Missing:
    """The source has no value for the period (yet): not published, or published as missing."""

    why: str


Reading = Fraction | Missing


def _no_constants(name: str) -> Any:  # noqa: ANN401 - json's parse_constant hook
    msg = f"non-standard JSON constant {name}"
    raise MalformedResponseError(msg)


def strict_json(body: bytes) -> Any:  # noqa: ANN401 - JSON
    """Parse ``body`` as UTF-8 JSON, refusing NaN and Infinity."""
    try:
        return json.loads(body.decode("utf-8"), parse_constant=_no_constants)
    except (UnicodeDecodeError, json.JSONDecodeError) as e:
        msg = f"not JSON: {e}"
        raise MalformedResponseError(msg) from e


def field(obj: object, name: str, kind: type) -> Any:  # noqa: ANN401 - JSON
    """``obj[name]``, which must exist and be a ``kind``."""
    if not isinstance(obj, dict) or not isinstance(obj.get(name), kind):
        msg = f"expected a {kind.__name__} field {name!r}"
        raise MalformedResponseError(msg)
    return obj[name]
