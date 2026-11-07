"""Exact values and decisions. Every number is a ``Fraction``: source values are parsed from
their decimal strings, arithmetic is exact, and rounding is explicit (half away from zero,
which is how BLS and NWS round)."""

from __future__ import annotations

import re
from fractions import Fraction

from brier_services.rules import Op

_DECIMAL = re.compile(r"-?[0-9]+(?:\.[0-9]+)?")


def parse_decimal(text: str) -> Fraction | None:
    """The exact value of a plain decimal string such as ``"4.26"`` or ``"-3.2"``; ``None`` for
    anything else (exponents, spaces inside, ``nan``)."""
    s = text.strip()
    if not _DECIMAL.fullmatch(s):
        return None
    return Fraction(s)


def round_half_up(x: Fraction, places: int) -> Fraction:
    """``x`` rounded to ``places`` decimal places, halves away from zero."""
    scale = 10**places
    scaled = abs(x) * scale
    whole = scaled.numerator // scaled.denominator
    if scaled - whole >= Fraction(1, 2):
        whole += 1
    return Fraction(whole if x >= 0 else -whole, scale)


def format_decimal(x: Fraction, places: int) -> str:
    """``x``, which must already be a multiple of 10^-places, with exactly ``places`` decimals."""
    scale = 10**places
    scaled = x * scale
    if scaled.denominator != 1:
        msg = f"{x} has more than {places} decimal places"
        raise ValueError(msg)
    n = scaled.numerator
    sign, digits = ("-" if n < 0 else ""), str(abs(n)).rjust(places + 1, "0")
    if places == 0:
        return sign + digits
    return f"{sign}{digits[:-places]}.{digits[-places:]}"


def holds(value: Fraction, op: Op, threshold: str) -> bool:
    """Whether ``value op threshold`` is true."""
    t = parse_decimal(threshold)
    if t is None:
        msg = f"threshold {threshold!r} is not a decimal"
        raise ValueError(msg)
    if op is Op.ABOVE:
        return value > t
    if op is Op.AT_LEAST:
        return value >= t
    if op is Op.BELOW:
        return value < t
    return value <= t
