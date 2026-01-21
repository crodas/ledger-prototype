use std::ops::Deref;

use serde::{Deserialize, Serialize};

use crate::Error;

/// Amount
///
/// The ledger supports negative and positive numbers. By definition the ledger is append only, and
/// all transactions are final. That's why chargebacks are negative deposits.
///
/// Amounts are always stored in the lowest denomination (cents or sats), and the ledger users give
/// them meaning (dollar, bitcoin, etc).
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount(i128);

impl From<i128> for Amount {
    fn from(value: i128) -> Self {
        Amount(value)
    }
}

impl Deref for Amount {
    type Target = i128;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Amount {
    /// Serializes the amount to bytes for hashing and storage.
    ///
    /// Uses little-endian encoding for consistency across platforms.
    pub fn to_bytes(&self) -> [u8; 16] {
        self.0.to_le_bytes()
    }

    /// Converts a floating-point number to an Amount with the given decimal precision.
    ///
    /// The precision indicates how many decimal places to preserve. For example,
    /// with precision=2 (cents), 12.34 becomes 1234. Values are truncated toward
    /// zero (not rounded) to ensure predictable behavior.
    ///
    /// # Errors
    /// Returns `Error::Math` for NaN, infinity, or values outside i128 range.
    pub fn from_f64(number: f64, precision: u8) -> Result<Self, Error> {
        if !number.is_finite() {
            return Err(Error::Math);
        }

        let scale = 10f64.powi(precision as i32);
        let scaled = number * scale;

        // Chop directly (truncate toward zero), no rounding
        let chopped = scaled.trunc();

        // Reject values outside i128 range
        if chopped < i128::MIN as f64 || chopped > i128::MAX as f64 {
            return Err(Error::Math);
        }

        Ok(Amount(chopped as i128))
    }

    /// Converts the amount back to a floating-point number.
    ///
    /// The precision indicates how many decimal places the stored value represents.
    /// For example, with precision=2, an amount of 1234 becomes 12.34.
    ///
    /// # Errors
    /// Returns `Error::Math` if the result would be infinite or if precision is too large.
    ///
    /// # Note
    /// Large i128 values may lose precision when converted to f64.
    pub fn to_f64(&self, precision: u8) -> Result<f64, Error> {
        // 10^precision as f64
        let scale = 10f64.powi(precision as i32);

        // Convert i128 -> f64 (note: large values may lose integer precision in f64)
        let value = self.0 as f64;

        if !value.is_finite() || !scale.is_finite() || scale == 0.0 {
            return Err(Error::Math);
        }

        let out = value / scale;

        if !out.is_finite() {
            return Err(Error::Math);
        }

        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_f64_rejects_nan_and_infinity() {
        assert!(matches!(Amount::from_f64(f64::NAN, 2), Err(Error::Math)));
        assert!(matches!(
            Amount::from_f64(f64::INFINITY, 2),
            Err(Error::Math)
        ));
        assert!(matches!(
            Amount::from_f64(f64::NEG_INFINITY, 2),
            Err(Error::Math)
        ));
    }

    #[test]
    fn from_f64_truncates_toward_zero_positive() {
        // 12.349 @ 2dp -> 1234.9 -> 1234
        let a = Amount::from_f64(12.349, 2).expect("12.349 @ 2dp should succeed");
        assert_eq!(*a, 1234);

        // 12.340 @ 2dp -> 1234.0 -> 1234
        let b = Amount::from_f64(12.34, 2).expect("12.34 @ 2dp should succeed");
        assert_eq!(*b, 1234);

        // small positive gets chopped to 0 at 0dp
        let c = Amount::from_f64(0.999, 0).expect("0.999 @ 0dp should succeed");
        assert_eq!(*c, 0);
    }

    #[test]
    fn from_f64_truncates_toward_zero_negative() {
        // -1.239 @ 2dp -> -123.9 -> -123 (toward zero)
        let a = Amount::from_f64(-1.239, 2).expect("-1.239 @ 2dp should succeed");
        assert_eq!(*a, -123);

        // -1.230 @ 2dp -> -123.0 -> -123
        let b = Amount::from_f64(-1.23, 2).expect("-1.23 @ 2dp should succeed");
        assert_eq!(*b, -123);

        // small negative gets chopped to 0 at 0dp (toward zero)
        let c = Amount::from_f64(-0.999, 0).expect("-0.999 @ 0dp should succeed");
        assert_eq!(*c, 0);
    }

    #[test]
    fn from_f64_precision_zero() {
        let a = Amount::from_f64(12.9, 0).expect("12.9 @ 0dp should succeed");
        assert_eq!(*a, 12);

        let b = Amount::from_f64(-12.9, 0).expect("-12.9 @ 0dp should succeed");
        assert_eq!(*b, -12);
    }

    #[test]
    fn from_f64_handles_negative_zero() {
        let a = Amount::from_f64(-0.0, 6).expect("-0.0 should convert to zero");
        assert_eq!(*a, 0);
    }

    #[test]
    fn from_f64_overflow_returns_error() {
        // Out of i128 range before chopping
        let too_big = (i128::MAX as f64) * 2.0;
        assert!(matches!(Amount::from_f64(too_big, 0), Err(Error::Math)));

        let too_small = (i128::MIN as f64) * 2.0;
        assert!(matches!(Amount::from_f64(too_small, 0), Err(Error::Math)));

        // In range at precision=0, but scaling by 10 pushes it out of range
        let near_max = i128::MAX as f64;
        assert!(matches!(Amount::from_f64(near_max, 1), Err(Error::Math)));
    }

    #[test]
    fn from_f64_chops_not_rounds_at_half() {
        // With truncation, 1.999 @ 0dp -> 1 (not 2)
        let a = Amount::from_f64(1.999, 0).expect("1.999 @ 0dp should succeed");
        assert_eq!(*a, 1);

        // And negatives truncate toward zero: -1.999 @ 0dp -> -1 (not -2)
        let b = Amount::from_f64(-1.999, 0).expect("-1.999 @ 0dp should succeed");
        assert_eq!(*b, -1);
    }
}
