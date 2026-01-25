//! Test configuration for intent generation
//!
//! Provides configurable test parameters for generating execution intents
//! with validation to ensure values are reasonable.

use crate::intent_generator::errors::IntentGeneratorError;

/// Test configuration for generating intents
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Default price for leg A (fixed-point: 0.50 = 5000 with scale 2)
    pub leg_a_price_int: i64,
    pub leg_a_price_scale: i16,

    /// Default price for leg B (fixed-point: 0.49 = 4900 with scale 2)
    pub leg_b_price_int: i64,
    pub leg_b_price_scale: i16,

    /// Default order size (fixed-point: 1.0 = 1000000 with scale 6)
    pub size_int: i64,
    pub size_scale: i16,

    /// Expected profit (fixed-point: 0.01 = 10000 with scale 6)
    pub expected_profit_int: i64,
    pub expected_profit_scale: i16,

    /// Expected ROI in basis points (200 = 2%)
    pub expected_roi_bps: i32,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            leg_a_price_int: 5000, // 0.50
            leg_a_price_scale: 2,
            leg_b_price_int: 4900, // 0.49
            leg_b_price_scale: 2,
            size_int: 1000000, // 1.0 USD
            size_scale: 6,
            expected_profit_int: 10000, // 0.01 USD
            expected_profit_scale: 6,
            expected_roi_bps: 200, // 2%
        }
    }
}

impl TestConfig {
    /// Validate configuration values
    ///
    /// # Returns
    /// `Ok(())` if configuration is valid
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::InvalidConfig` if values are invalid
    pub fn validate(&self) -> Result<(), IntentGeneratorError> {
        // Validate price scales (0-18)
        if self.leg_a_price_scale < 0 || self.leg_a_price_scale > 18 {
            return Err(IntentGeneratorError::InvalidConfig(format!(
                "leg_a_price_scale must be between 0 and 18, got {}",
                self.leg_a_price_scale
            )));
        }

        if self.leg_b_price_scale < 0 || self.leg_b_price_scale > 18 {
            return Err(IntentGeneratorError::InvalidConfig(format!(
                "leg_b_price_scale must be between 0 and 18, got {}",
                self.leg_b_price_scale
            )));
        }

        // Validate size scale (0-18)
        if self.size_scale < 0 || self.size_scale > 18 {
            return Err(IntentGeneratorError::InvalidConfig(format!(
                "size_scale must be between 0 and 18, got {}",
                self.size_scale
            )));
        }

        // Validate prices are non-negative
        if self.leg_a_price_int < 0 {
            return Err(IntentGeneratorError::InvalidConfig(
                "leg_a_price_int must be non-negative".to_string(),
            ));
        }

        if self.leg_b_price_int < 0 {
            return Err(IntentGeneratorError::InvalidConfig(
                "leg_b_price_int must be non-negative".to_string(),
            ));
        }

        // Validate size is positive
        if self.size_int <= 0 {
            return Err(IntentGeneratorError::InvalidConfig(
                "size_int must be positive".to_string(),
            ));
        }

        // Validate profit scale (0-18)
        if self.expected_profit_scale < 0 || self.expected_profit_scale > 18 {
            return Err(IntentGeneratorError::InvalidConfig(format!(
                "expected_profit_scale must be between 0 and 18, got {}",
                self.expected_profit_scale
            )));
        }

        // Validate profit is non-negative
        if self.expected_profit_int < 0 {
            return Err(IntentGeneratorError::InvalidConfig(
                "expected_profit_int must be non-negative".to_string(),
            ));
        }

        Ok(())
    }
}
