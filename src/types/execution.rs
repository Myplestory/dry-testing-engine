//! Execution intent and related types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::types::order::{OrderLeg, OrderSide};
use crate::types::venue::VenueType;

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
pub enum ExecutionPriority {
    /// Standard opportunity (can be dropped under backpressure)
    Opportunity,
    
    /// Risk-critical execution (never drop)
    RiskCritical,
}

