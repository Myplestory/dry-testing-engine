//! Execution intent and related types

use crate::types::order::{OrderLeg, OrderSide};
use crate::types::venue::VenueType;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Arbitrage execution intent - input to execution lanes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbExecutionIntent {
    /// Unique identifier for this execution intent
    pub intent_id: Uuid,

    /// Sequence number for ordering guarantees
    pub sequence_number: u64,

    /// Customer ID
    pub customer_id: Uuid,

    /// Strategy ID
    pub strategy_id: Uuid,

    /// Verified pair ID
    pub pair_id: Uuid,

    /// Leg A specification
    pub leg_a: LegSpec,

    /// Leg B specification
    pub leg_b: LegSpec,

    /// Expected profit (fixed-point: int, scale)
    pub expected_profit_int: i64,
    pub expected_profit_scale: i16,

    /// Expected ROI in basis points
    pub expected_roi_bps: i32,

    /// Outcome mapping from verified pair
    pub outcome_mapping: std::collections::HashMap<String, String>,

    /// Execution priority
    pub priority: ExecutionPriority,

    /// When opportunity was detected
    pub detected_at: DateTime<Utc>,

    /// When intent was enqueued (set by router)
    pub enqueued_at: Option<DateTime<Utc>>,
}

impl ArbExecutionIntent {
    /// Generate unique client_order_id for a leg
    /// Format: {intent_id}_{leg}_{venue}
    ///
    /// This ensures uniqueness per (customer_id, venue, client_order_id) as required
    /// by the database unique constraint for idempotency.
    pub fn generate_client_order_id(&self, leg: OrderLeg) -> String {
        let leg_str = match leg {
            OrderLeg::A => "a",
            OrderLeg::B => "b",
        };
        let venue_str = match leg {
            OrderLeg::A => self.leg_a.venue.as_str(),
            OrderLeg::B => self.leg_b.venue.as_str(),
        };
        format!("{}_{}_{}", self.intent_id, leg_str, venue_str)
    }

    /// Generate client_order_ids for both legs
    pub fn generate_client_order_ids(&self) -> (String, String) {
        (
            self.generate_client_order_id(OrderLeg::A),
            self.generate_client_order_id(OrderLeg::B),
        )
    }

    /// Validate intent before execution
    pub fn validate(&self) -> Result<(), String> {
        // Validate fixed-point values are non-negative
        if self.expected_profit_int < 0 {
            return Err("Expected profit cannot be negative".to_string());
        }

        if self.leg_a.price_int < 0 || self.leg_b.price_int < 0 {
            return Err("Price cannot be negative".to_string());
        }

        if self.leg_a.size_int <= 0 || self.leg_b.size_int <= 0 {
            return Err("Order size must be positive".to_string());
        }

        // Validate scales are reasonable (0-18, typical for financial systems)
        if self.leg_a.price_scale < 0 || self.leg_a.price_scale > 18 {
            return Err("Price scale must be between 0 and 18".to_string());
        }

        if self.leg_a.size_scale < 0 || self.leg_a.size_scale > 18 {
            return Err("Size scale must be between 0 and 18".to_string());
        }

        Ok(())
    }
}

/// Leg specification for arbitrage execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegSpec {
    /// Venue for this leg
    pub venue: VenueType,

    /// Venue market ID
    pub venue_market_id: String,

    /// Venue outcome ID (token ID, outcome name, etc.)
    pub venue_outcome_id: String,

    /// Order side
    pub side: OrderSide,

    /// Limit price (fixed-point: int, scale)
    pub price_int: i64,
    pub price_scale: i16,

    /// Order size (fixed-point: int, scale)
    pub size_int: i64,
    pub size_scale: i16,
}

impl Default for LegSpec {
    fn default() -> Self {
        Self {
            venue: VenueType::Polymarket,
            venue_market_id: String::new(),
            venue_outcome_id: String::new(),
            side: OrderSide::Buy,
            price_int: 0,
            price_scale: 2,
            size_int: 0,
            size_scale: 2,
        }
    }
}

/// Execution priority
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionPriority {
    /// Standard opportunity (can be dropped under backpressure)
    Opportunity,

    /// Risk-critical execution (never drop)
    RiskCritical,
}

impl std::fmt::Display for ExecutionPriority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ExecutionPriority::Opportunity => "opportunity",
            ExecutionPriority::RiskCritical => "risk_critical",
        };
        write!(f, "{}", s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_order_id_generation() {
        let intent = ArbExecutionIntent {
            intent_id: Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap(),
            sequence_number: 0,
            customer_id: Uuid::new_v4(),
            strategy_id: Uuid::new_v4(),
            pair_id: Uuid::new_v4(),
            leg_a: LegSpec {
                venue: VenueType::Polymarket,
                ..Default::default()
            },
            leg_b: LegSpec {
                venue: VenueType::Kalshi,
                ..Default::default()
            },
            expected_profit_int: 1000,
            expected_profit_scale: 6,
            expected_roi_bps: 50,
            outcome_mapping: Default::default(),
            priority: ExecutionPriority::Opportunity,
            detected_at: Utc::now(),
            enqueued_at: None,
        };

        let id_a = intent.generate_client_order_id(OrderLeg::A);
        let id_b = intent.generate_client_order_id(OrderLeg::B);

        assert!(id_a.contains("a"));
        assert!(id_a.contains("polymarket"));
        assert!(id_b.contains("b"));
        assert!(id_b.contains("kalshi"));
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn test_intent_validation() {
        let mut intent = ArbExecutionIntent {
            intent_id: Uuid::new_v4(),
            sequence_number: 0,
            customer_id: Uuid::new_v4(),
            strategy_id: Uuid::new_v4(),
            pair_id: Uuid::new_v4(),
            leg_a: LegSpec {
                price_int: 100,
                price_scale: 2,
                size_int: 1000,
                size_scale: 2,
                ..Default::default()
            },
            leg_b: LegSpec {
                price_int: 100,
                price_scale: 2,
                size_int: 1000,
                size_scale: 2,
                ..Default::default()
            },
            expected_profit_int: 1000,
            expected_profit_scale: 6,
            expected_roi_bps: 50,
            outcome_mapping: Default::default(),
            priority: ExecutionPriority::Opportunity,
            detected_at: Utc::now(),
            enqueued_at: None,
        };

        assert!(intent.validate().is_ok());

        // Test negative price
        intent.leg_a.price_int = -1;
        assert!(intent.validate().is_err());

        // Reset
        intent.leg_a.price_int = 100;

        // Test zero size
        intent.leg_a.size_int = 0;
        assert!(intent.validate().is_err());
    }
}
