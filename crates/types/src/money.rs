//! Prices, quantities, and credits (SPEC §2.1).

use crate::UnitError;

/// Lowest price a contract can trade at, in ticks (credit-cents).
pub const MIN_PRICE_TICKS: u8 = 1;
/// Highest price a contract can trade at, in ticks. A price of 0 or 100 would mean certainty,
/// which a market that is still open cannot express.
pub const MAX_PRICE_TICKS: u8 = 99;
/// What one winning contract pays at settlement, in ticks.
pub const PAYOUT_TICKS: u8 = 100;
/// Largest quantity a single order may carry. A fat-finger limit, not a position limit.
pub const MAX_ORDER_QTY: u32 = 25_000;

/// The price of a YES contract, in ticks in `[1, 99]`.
///
/// One book per market is quoted in YES terms (SPEC §2.2); a NO price is the complement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(u8);

impl Price {
    /// The lowest valid price.
    pub const MIN: Self = Self(MIN_PRICE_TICKS);
    /// The highest valid price.
    pub const MAX: Self = Self(MAX_PRICE_TICKS);

    /// A price of `ticks`, or an error if it is outside `[1, 99]`.
    ///
    /// # Errors
    /// [`UnitError::PriceOutOfRange`] if `ticks` is 0 or above 99.
    pub const fn new(ticks: u8) -> Result<Self, UnitError> {
        if ticks >= MIN_PRICE_TICKS && ticks <= MAX_PRICE_TICKS {
            Ok(Self(ticks))
        } else {
            Err(UnitError::PriceOutOfRange { ticks })
        }
    }

    /// The price in ticks.
    #[must_use]
    pub const fn ticks(self) -> u8 {
        self.0
    }

    /// The price of the opposite contract: a YES at `p` is a NO at `100 - p` (SPEC §2.2).
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)] // self.0 is in [1, 99], so 100 - self.0 is in [1, 99].
    pub const fn complement(self) -> Self {
        Self(PAYOUT_TICKS - self.0)
    }

    /// Cost of opening `qty` YES contracts at this price: `p × q` (SPEC §2.3).
    #[must_use]
    pub const fn yes_cost(self, qty: Quantity) -> Credits {
        Credits(small_product(self.0, qty.0))
    }

    /// Cost of opening `qty` NO contracts against a YES price of `p`: `(100 − p) × q`.
    #[must_use]
    pub const fn no_cost(self, qty: Quantity) -> Credits {
        Credits(small_product(self.complement().0, qty.0))
    }
}

/// Escrow backing `qty` YES/NO pairs: `100 × q`. Always equals `yes_cost + no_cost` at any
/// price, which is the identity behind the escrow invariant I2 (SPEC §4.3).
#[must_use]
pub const fn pair_value(qty: Quantity) -> Credits {
    Credits(small_product(PAYOUT_TICKS, qty.0))
}

/// `a × b` for a tick count and a contract count.
///
/// The largest possible value is `100 × 25_000 = 2_500_000`, far below `i64::MAX`, so the
/// product cannot overflow for any `Price`/`Quantity`. Callers only pass validated values.
#[allow(clippy::arithmetic_side_effects)] // bounded as shown above; checked by a test at the limits.
const fn small_product(ticks: u8, contracts: u32) -> i64 {
    (ticks as i64) * (contracts as i64)
}

/// A number of whole contracts in `[1, MAX_ORDER_QTY]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quantity(u32);

impl Quantity {
    /// The largest quantity a single order may carry.
    pub const MAX: Self = Self(MAX_ORDER_QTY);

    /// A quantity of `contracts`, or an error if it is 0 or above [`MAX_ORDER_QTY`].
    ///
    /// # Errors
    /// [`UnitError::QuantityOutOfRange`] if `contracts` is 0 or above the limit.
    pub const fn new(contracts: u32) -> Result<Self, UnitError> {
        if contracts >= 1 && contracts <= MAX_ORDER_QTY {
            Ok(Self(contracts))
        } else {
            Err(UnitError::QuantityOutOfRange { contracts })
        }
    }

    /// The quantity in contracts.
    #[must_use]
    pub const fn contracts(self) -> u32 {
        self.0
    }
}

/// An amount of credits, in integer credit-cents. Signed: the system issuance account holds
/// the negative of every credit ever granted (SPEC §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Credits(i64);

impl Credits {
    /// Zero credits.
    pub const ZERO: Self = Self(0);

    /// An amount of `cents` credit-cents.
    #[must_use]
    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    /// The amount in credit-cents.
    #[must_use]
    pub const fn cents(self) -> i64 {
        self.0
    }

    /// `self + other`, or an error if the exact sum does not fit in an `i64`.
    ///
    /// # Errors
    /// [`UnitError::CreditsOverflow`] on overflow.
    pub const fn checked_add(self, other: Self) -> Result<Self, UnitError> {
        match self.0.checked_add(other.0) {
            Some(sum) => Ok(Self(sum)),
            None => Err(UnitError::CreditsOverflow),
        }
    }

    /// `self - other`, or an error if the exact difference does not fit in an `i64`.
    ///
    /// # Errors
    /// [`UnitError::CreditsOverflow`] on overflow.
    pub const fn checked_sub(self, other: Self) -> Result<Self, UnitError> {
        match self.0.checked_sub(other.0) {
            Some(difference) => Ok(Self(difference)),
            None => Err(UnitError::CreditsOverflow),
        }
    }

    /// `-self`, or an error for `i64::MIN`, whose negation does not fit.
    ///
    /// # Errors
    /// [`UnitError::CreditsOverflow`] if `self` is `i64::MIN` credit-cents.
    pub const fn checked_neg(self) -> Result<Self, UnitError> {
        match self.0.checked_neg() {
            Some(negated) => Ok(Self(negated)),
            None => Err(UnitError::CreditsOverflow),
        }
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects)] // tests compute expected values with plain arithmetic.
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn price_accepts_exactly_one_to_ninety_nine() {
        // SPEC §2.1: prices are integer ticks 1 to 99.
        for ticks in 0..=u8::MAX {
            let valid = (1..=99).contains(&ticks);
            assert_eq!(Price::new(ticks).is_ok(), valid, "ticks = {ticks}");
        }
        assert_eq!(Price::new(0), Err(UnitError::PriceOutOfRange { ticks: 0 }));
        assert_eq!(
            Price::new(100),
            Err(UnitError::PriceOutOfRange { ticks: 100 })
        );
    }

    #[test]
    fn quantity_accepts_exactly_one_to_the_order_limit() {
        // SPEC §2.1: 1 ≤ q ≤ MAX_ORDER_QTY (25,000).
        assert_eq!(
            Quantity::new(0),
            Err(UnitError::QuantityOutOfRange { contracts: 0 })
        );
        assert!(Quantity::new(1).is_ok());
        assert!(Quantity::new(25_000).is_ok());
        assert_eq!(
            Quantity::new(25_001),
            Err(UnitError::QuantityOutOfRange { contracts: 25_001 })
        );
        assert_eq!(
            Quantity::new(u32::MAX),
            Err(UnitError::QuantityOutOfRange {
                contracts: u32::MAX
            })
        );
    }

    #[test]
    fn costs_at_the_limits_are_exact() {
        // The largest products small_product can see: 99 × 25,000 and 100 × 25,000 (its
        // overflow argument rests on these).
        let q = Quantity::MAX;
        assert_eq!(Price::MAX.yes_cost(q), Credits::from_cents(2_475_000));
        assert_eq!(Price::MIN.no_cost(q), Credits::from_cents(2_475_000));
        assert_eq!(pair_value(q), Credits::from_cents(2_500_000));
    }

    #[test]
    fn credits_overflow_is_an_error_not_a_wrap() {
        let max = Credits::from_cents(i64::MAX);
        let min = Credits::from_cents(i64::MIN);
        let one = Credits::from_cents(1);
        assert_eq!(max.checked_add(one), Err(UnitError::CreditsOverflow));
        assert_eq!(min.checked_sub(one), Err(UnitError::CreditsOverflow));
        assert_eq!(min.checked_neg(), Err(UnitError::CreditsOverflow));
        assert_eq!(max.checked_neg(), Ok(Credits::from_cents(-i64::MAX)));
    }

    proptest! {
        #[test]
        fn complement_is_an_involution_summing_to_the_payout(ticks in 1u8..=99) {
            let p = Price::new(ticks).unwrap();
            prop_assert_eq!(p.complement().complement(), p);
            prop_assert_eq!(p.ticks() + p.complement().ticks(), PAYOUT_TICKS);
        }

        #[test]
        fn yes_and_no_cost_always_sum_to_the_pair_value(ticks in 1u8..=99, contracts in 1u32..=25_000) {
            // The identity behind invariant I2 (SPEC §4.3): two opening sides together fund
            // exactly 100 per pair, whatever the price.
            let p = Price::new(ticks).unwrap();
            let q = Quantity::new(contracts).unwrap();
            let total = p.yes_cost(q).checked_add(p.no_cost(q)).unwrap();
            prop_assert_eq!(total, pair_value(q));
            prop_assert_eq!(p.yes_cost(q).cents(), i64::from(ticks) * i64::from(contracts));
        }

        #[test]
        fn checked_add_and_sub_match_exact_integer_arithmetic(a in any::<i64>(), b in any::<i64>()) {
            // Reference: the exact result in i128, which cannot overflow for two i64 values.
            let exact_sum = i128::from(a) + i128::from(b);
            let exact_diff = i128::from(a) - i128::from(b);
            let (x, y) = (Credits::from_cents(a), Credits::from_cents(b));
            prop_assert_eq!(x.checked_add(y).ok().map(|c| i128::from(c.cents())),
                            i64::try_from(exact_sum).ok().map(i128::from));
            prop_assert_eq!(x.checked_sub(y).ok().map(|c| i128::from(c.cents())),
                            i64::try_from(exact_diff).ok().map(i128::from));
        }
    }
}
