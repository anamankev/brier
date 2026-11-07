"""Domain units (SPEC §2.1), written from the specification for the Python reference engine.

The reference engine (SPEC §3.4) must be independent of the Rust engine, so these units are
stated here from the spec text, not translated from the Rust crate. Both sides are tested
against the same spec rules; differential tests in G1 then compare the two engines.

Python integers never overflow, but the Rust ledger stores credit-cents in an ``i64``. Credits
here therefore reject any value outside the ``i64`` range, so the two implementations fail on
the same inputs instead of silently diverging.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Final

MIN_PRICE_TICKS: Final = 1
MAX_PRICE_TICKS: Final = 99
PAYOUT_TICKS: Final = 100
MAX_ORDER_QTY: Final = 25_000  # fat-finger limit per order (SPEC §2.1)

I64_MIN: Final = -(2**63)
I64_MAX: Final = 2**63 - 1


class UnitError(ValueError):
    """A value outside the range its unit allows. Values are rejected, never clipped."""


class PriceOutOfRangeError(UnitError):
    """A price outside [MIN_PRICE_TICKS, MAX_PRICE_TICKS]."""


class QuantityOutOfRangeError(UnitError):
    """A quantity outside [1, MAX_ORDER_QTY]."""


class CreditsOverflowError(UnitError):
    """A credit amount, or the result of credit arithmetic, outside the i64 range."""


def _require_int(value: object, name: str) -> int:
    # bool is a subclass of int in Python; Price(True) must not quietly mean 1 tick.
    if isinstance(value, bool) or not isinstance(value, int):
        msg = f"{name} must be an int, got {type(value).__name__}"
        raise TypeError(msg)
    return value


@dataclass(frozen=True, slots=True, order=True)
class Quantity:
    """A number of whole contracts in [1, MAX_ORDER_QTY]."""

    contracts: int

    def __post_init__(self) -> None:
        n = _require_int(self.contracts, "contracts")
        if not 1 <= n <= MAX_ORDER_QTY:
            msg = f"quantity {n} is outside [1, {MAX_ORDER_QTY}] contracts"
            raise QuantityOutOfRangeError(msg)


@dataclass(frozen=True, slots=True, order=True)
class Credits:
    """An amount of credits in integer credit-cents, within the i64 range."""

    cents: int

    def __post_init__(self) -> None:
        n = _require_int(self.cents, "cents")
        if not I64_MIN <= n <= I64_MAX:
            msg = f"{n} credit-cents is outside the i64 range"
            raise CreditsOverflowError(msg)

    def __add__(self, other: Credits) -> Credits:
        return Credits(self.cents + other.cents)

    def __sub__(self, other: Credits) -> Credits:
        return Credits(self.cents - other.cents)

    def __neg__(self) -> Credits:
        return Credits(-self.cents)


@dataclass(frozen=True, slots=True, order=True)
class Price:
    """The price of a YES contract in ticks, in [1, 99]. The NO price is the complement."""

    ticks: int

    def __post_init__(self) -> None:
        n = _require_int(self.ticks, "ticks")
        if not MIN_PRICE_TICKS <= n <= MAX_PRICE_TICKS:
            msg = f"price {n} is outside [{MIN_PRICE_TICKS}, {MAX_PRICE_TICKS}] ticks"
            raise PriceOutOfRangeError(msg)

    def complement(self) -> Price:
        """The opposite contract's price: a YES at p is a NO at 100 - p (SPEC §2.2)."""
        return Price(PAYOUT_TICKS - self.ticks)

    def yes_cost(self, qty: Quantity) -> Credits:
        """Cost of opening ``qty`` YES contracts: p x q (SPEC §2.3)."""
        return Credits(self.ticks * qty.contracts)

    def no_cost(self, qty: Quantity) -> Credits:
        """Cost of opening ``qty`` NO contracts against YES price p: (100 - p) x q."""
        return Credits((PAYOUT_TICKS - self.ticks) * qty.contracts)


def pair_value(qty: Quantity) -> Credits:
    """Escrow backing ``qty`` YES/NO pairs: 100 x q (the identity behind invariant I2)."""
    return Credits(PAYOUT_TICKS * qty.contracts)
