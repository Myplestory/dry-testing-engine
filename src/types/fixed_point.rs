//! Fixed-point arithmetic helpers for financial calculations
//!
//! All financial calculations use fixed-point integers (i64, i16) to avoid
//! floating-point precision errors. This matches the database schema pattern.

use std::cmp::Ordering;

/// Fixed-point number representation: (integer part, scale)
/// Example: (100, 2) = 1.00, (1000, 3) = 1.000
pub type ScaledInt = (i64, i16);

/// Error type for fixed-point arithmetic operations
#[derive(Debug, thiserror::Error)]
pub enum FixedPointError {
    #[error("Overflow in fixed-point arithmetic")]
    Overflow,

    #[error("Underflow in fixed-point arithmetic")]
    Underflow,

    #[error("Division by zero")]
    DivisionByZero,

    #[error("Scale mismatch: {0} != {1}")]
    ScaleMismatch(i16, i16),
}

/// Normalize two scaled integers to the same scale (use maximum scale)
fn normalize_scales(a: ScaledInt, b: ScaledInt) -> (ScaledInt, ScaledInt) {
    let max_scale = a.1.max(b.1);
    let scale_diff_a = max_scale - a.1;
    let scale_diff_b = max_scale - b.1;

    let normalized_a = if scale_diff_a > 0 {
        (a.0 * 10_i64.pow(scale_diff_a as u32), max_scale)
    } else {
        a
    };

    let normalized_b = if scale_diff_b > 0 {
        (b.0 * 10_i64.pow(scale_diff_b as u32), max_scale)
    } else {
        b
    };

    (normalized_a, normalized_b)
}

/// Add two scaled integers with automatic scale normalization
pub fn add_scaled(a: ScaledInt, b: ScaledInt) -> std::result::Result<ScaledInt, FixedPointError> {
    let (norm_a, norm_b) = normalize_scales(a, b);
    let scale = norm_a.1;

    norm_a
        .0
        .checked_add(norm_b.0)
        .map(|sum| (sum, scale))
        .ok_or(FixedPointError::Overflow)
}

/// Subtract two scaled integers with automatic scale normalization
pub fn subtract_scaled(
    a: ScaledInt,
    b: ScaledInt,
) -> std::result::Result<ScaledInt, FixedPointError> {
    let (norm_a, norm_b) = normalize_scales(a, b);
    let scale = norm_a.1;

    norm_a
        .0
        .checked_sub(norm_b.0)
        .map(|diff| (diff, scale))
        .ok_or(FixedPointError::Underflow)
}

/// Multiply two scaled integers
/// Result scale is sum of input scales (e.g., (100, 2) * (100, 2) = (10000, 4))
pub fn multiply_scaled(
    a: ScaledInt,
    b: ScaledInt,
) -> std::result::Result<ScaledInt, FixedPointError> {
    a.0.checked_mul(b.0)
        .map(|product| (product, a.1 + b.1))
        .ok_or(FixedPointError::Overflow)
}

/// Divide two scaled integers
/// Result scale matches the numerator scale (e.g., (100, 2) / (50, 2) = (200, 2) = 2.00)
/// To maintain precision, we multiply numerator by 10^result_scale before dividing
pub fn divide_scaled(
    a: ScaledInt,
    b: ScaledInt,
) -> std::result::Result<ScaledInt, FixedPointError> {
    if b.0 == 0 {
        return Err(FixedPointError::DivisionByZero);
    }

    // Use the maximum scale to maintain precision
    let result_scale = a.1.max(b.1);

    // Multiply numerator by 10^result_scale to maintain precision
    // Example: (100, 2) / (50, 2) -> (100 * 100) / 50 = 200 (with scale 2)
    let multiplier = 10_i64.pow(result_scale as u32);

    a.0.checked_mul(multiplier)
        .and_then(|num| num.checked_div(b.0))
        .map(|quotient| (quotient, result_scale))
        .ok_or(FixedPointError::Overflow)
}

/// Compare two scaled integers
pub fn compare_scaled(a: ScaledInt, b: ScaledInt) -> Ordering {
    let (norm_a, norm_b) = normalize_scales(a, b);
    norm_a.0.cmp(&norm_b.0)
}

/// Check if a scaled integer is zero
pub fn is_zero(value: ScaledInt) -> bool {
    value.0 == 0
}

/// Check if a scaled integer is positive
pub fn is_positive(value: ScaledInt) -> bool {
    value.0 > 0
}

/// Check if a scaled integer is negative
pub fn is_negative(value: ScaledInt) -> bool {
    value.0 < 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_same_scale() {
        let a = (100, 2); // 1.00
        let b = (50, 2); // 0.50
        let result = add_scaled(a, b).unwrap();
        assert_eq!(result, (150, 2)); // 1.50
    }

    #[test]
    fn test_add_different_scales() {
        let a = (100, 2); // 1.00
        let b = (500, 3); // 0.500
        let result = add_scaled(a, b).unwrap();
        assert_eq!(result, (1500, 3)); // 1.500
    }

    #[test]
    fn test_subtract_scaled() {
        let a = (100, 2); // 1.00
        let b = (25, 2); // 0.25
        let result = subtract_scaled(a, b).unwrap();
        assert_eq!(result, (75, 2)); // 0.75
    }

    #[test]
    fn test_multiply_scaled() {
        let a = (100, 2); // 1.00
        let b = (200, 2); // 2.00
        let result = multiply_scaled(a, b).unwrap();
        assert_eq!(result, (20000, 4)); // 2.0000
    }

    #[test]
    fn test_divide_scaled() {
        let a = (100, 2); // 1.00
        let b = (50, 2); // 0.50
        let result = divide_scaled(a, b).unwrap();
        // 1.00 / 0.50 = 2.00 = (200, 2)
        assert_eq!(result, (200, 2));
    }

    #[test]
    fn test_divide_by_zero() {
        let a = (100, 2);
        let b = (0, 2);
        assert!(matches!(
            divide_scaled(a, b),
            Err(FixedPointError::DivisionByZero)
        ));
    }

    #[test]
    fn test_compare_scaled() {
        let a = (100, 2); // 1.00
        let b = (50, 2); // 0.50
        assert_eq!(compare_scaled(a, b), Ordering::Greater);

        let c = (100, 2); // 1.00
        let d = (1000, 3); // 1.000
        assert_eq!(compare_scaled(c, d), Ordering::Equal);
    }

    #[test]
    fn test_overflow_handling() {
        let a = (i64::MAX, 2);
        let b = (1, 2);
        assert!(matches!(add_scaled(a, b), Err(FixedPointError::Overflow)));
    }
}
