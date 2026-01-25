//! Unit tests for intent generator

use dry_testing_engine::intent_generator::{IntentGeneratorError, TestConfig};

#[test]
fn test_config_default() {
    let config = TestConfig::default();
    assert_eq!(config.leg_a_price_int, 5000);
    assert_eq!(config.leg_a_price_scale, 2);
    assert_eq!(config.leg_b_price_int, 4900);
    assert_eq!(config.leg_b_price_scale, 2);
    assert_eq!(config.size_int, 1000000);
    assert_eq!(config.size_scale, 6);
    assert_eq!(config.expected_profit_int, 10000);
    assert_eq!(config.expected_profit_scale, 6);
    assert_eq!(config.expected_roi_bps, 200);
}

#[test]
fn test_config_validation_success() {
    let config = TestConfig::default();
    assert!(config.validate().is_ok());
}

#[test]
fn test_config_validation_invalid_price_scale() {
    let mut config = TestConfig::default();
    config.leg_a_price_scale = 19; // Invalid: > 18
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_negative_price() {
    let mut config = TestConfig::default();
    config.leg_a_price_int = -1;
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_zero_size() {
    let mut config = TestConfig::default();
    config.size_int = 0;
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_negative_size() {
    let mut config = TestConfig::default();
    config.size_int = -1;
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_invalid_size_scale() {
    let mut config = TestConfig::default();
    config.size_scale = 19; // Invalid: > 18
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_negative_profit() {
    let mut config = TestConfig::default();
    config.expected_profit_int = -1;
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_invalid_profit_scale() {
    let mut config = TestConfig::default();
    config.expected_profit_scale = 19; // Invalid: > 18
    assert!(matches!(
        config.validate(),
        Err(IntentGeneratorError::InvalidConfig(_))
    ));
}

#[test]
fn test_config_validation_all_scales_zero() {
    let config = TestConfig {
        leg_a_price_int: 100,
        leg_a_price_scale: 0,
        leg_b_price_int: 100,
        leg_b_price_scale: 0,
        size_int: 1000,
        size_scale: 0,
        expected_profit_int: 10,
        expected_profit_scale: 0,
        expected_roi_bps: 100,
    };
    assert!(config.validate().is_ok(), "Scale of 0 should be valid");
}

#[test]
fn test_config_validation_max_valid_scale() {
    let config = TestConfig {
        leg_a_price_int: 100,
        leg_a_price_scale: 18,
        leg_b_price_int: 100,
        leg_b_price_scale: 18,
        size_int: 1000,
        size_scale: 18,
        expected_profit_int: 10,
        expected_profit_scale: 18,
        expected_roi_bps: 100,
    };
    assert!(config.validate().is_ok(), "Scale of 18 should be valid");
}

