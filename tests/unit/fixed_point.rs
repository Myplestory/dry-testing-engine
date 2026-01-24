//! Unit tests for fixed-point arithmetic

use dry_testing_engine::types::fixed_point::*;

#[test]
fn test_add_same_scale() {
    let a = (100, 2); // 1.00
    let b = (50, 2);  // 0.50
    let result = add_scaled(a, b).unwrap();
    assert_eq!(result, (150, 2)); // 1.50
}

#[test]
fn test_add_different_scales() {
    let a = (100, 2);  // 1.00
    let b = (500, 3);  // 0.500
    let result = add_scaled(a, b).unwrap();
    assert_eq!(result, (1500, 3)); // 1.500
}

#[test]
fn test_subtract_scaled() {
    let a = (100, 2); // 1.00
    let b = (25, 2);   // 0.25
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
    let b = (50, 2);  // 0.50
    let result = divide_scaled(a, b).unwrap();
    assert_eq!(result.0, 200); // 2.00
}

#[test]
fn test_divide_by_zero() {
    let a = (100, 2);
    let b = (0, 2);
    assert!(matches!(divide_scaled(a, b), Err(FixedPointError::DivisionByZero)));
}

#[test]
fn test_compare_scaled() {
    let a = (100, 2); // 1.00
    let b = (50, 2);  // 0.50
    assert_eq!(compare_scaled(a, b), std::cmp::Ordering::Greater);
    
    let c = (100, 2); // 1.00
    let d = (1000, 3); // 1.000
    assert_eq!(compare_scaled(c, d), std::cmp::Ordering::Equal);
}

#[test]
fn test_overflow_handling() {
    let a = (i64::MAX, 2);
    let b = (1, 2);
    assert!(matches!(add_scaled(a, b), Err(FixedPointError::Overflow)));
}

#[test]
fn test_underflow_handling() {
    let a = (i64::MIN, 2);
    let b = (1, 2);
    assert!(matches!(subtract_scaled(a, b), Err(FixedPointError::Underflow)));
}

#[test]
fn test_zero_checks() {
    assert!(is_zero((0, 2)));
    assert!(!is_zero((1, 2)));
    assert!(is_positive((1, 2)));
    assert!(is_negative((-1, 2)));
}

