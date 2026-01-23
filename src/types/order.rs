//! Order and state machine types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::types::venue::VenueType;

/// Order leg identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderLeg {
    A,
    B,
}

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order status (matches database enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Submitted,
    Partial,
    Filled,
    Rejected,
    Canceled,
}

impl OrderStatus {
    /// Check if status is terminal (no further transitions possible)
    pub fn is_terminal(&self) -> bool {
        matches!(self, OrderStatus::Filled | OrderStatus::Rejected | OrderStatus::Canceled)
    }
}

/// Order state in the state machine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderState {
    /// Order ID (database primary key)
    pub order_id: Uuid,
    
    /// Intent ID this order belongs to
    pub intent_id: Uuid,
    
    /// Order leg
    pub leg: OrderLeg,
    
    /// Current status
    pub status: OrderStatus,
    
    /// Sequence number for ordering
    pub sequence_number: u64,
    
    /// Filled size (fixed-point: int, scale)
    pub filled_size_int: i64,
    pub filled_size_scale: i16,
    
    /// Venue order ID (set on ACK)
    pub venue_order_id: Option<String>,
    
    /// State transition history
    pub transitions: Vec<StateTransition>,
    
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    
    /// Last updated timestamp
    pub updated_at: DateTime<Utc>,
}

/// State transition record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Previous status
    pub from: OrderStatus,
    
    /// New status
    pub to: OrderStatus,
    
    /// Transition timestamp
    pub timestamp: DateTime<Utc>,
    
    /// Source of transition (venue_ack, venue_fill, coordinator_timeout, etc.)
    pub source: String,
}

/// Order record (matches database schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    /// Order ID (database primary key)
    pub id: Uuid,
    
    /// Customer ID
    pub customer_id: Uuid,
    
    /// Strategy ID
    pub strategy_id: Uuid,
    
    /// Pair ID
    pub pair_id: Uuid,
    
    /// Order leg
    pub leg: OrderLeg,
    
    /// Venue
    pub venue: VenueType,
    
    /// Venue market ID
    pub venue_market_id: String,
    
    /// Venue outcome ID
    pub venue_outcome_id: String,
    
    /// Client order ID (our idempotency key)
    pub client_order_id: String,
    
    /// Venue order ID (set on ACK)
    pub venue_order_id: Option<String>,
    
    /// Order side
    pub side: OrderSide,
    
    /// Limit price (fixed-point: int, scale)
    pub limit_price_int: i64,
    pub price_scale: i16,
    
    /// Order size (fixed-point: int, scale)
    pub size_int: i64,
    pub size_scale: i16,
    
    /// Current status
    pub status: OrderStatus,
    
    /// Filled size (fixed-point: int, scale)
    pub filled_size_int: i64,
    pub filled_size_scale: i16,
    
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    
    /// Updated timestamp
    pub updated_at: DateTime<Utc>,
}

/// Arbitrage execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArbExecutionStatus {
    Pending,
    Completed,
    Failed,
    Canceled,
    Partial,
}

impl ArbExecutionStatus {
    /// Check if status is terminal
    pub fn is_terminal(&self) -> bool {
        matches!(self, ArbExecutionStatus::Completed | ArbExecutionStatus::Failed | ArbExecutionStatus::Canceled)
    }
}

/// Arbitrage execution tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbExecution {
    /// Intent ID
    pub intent_id: Uuid,
    
    /// Leg A order ID
    pub leg_a_order_id: Uuid,
    
    /// Leg B order ID
    pub leg_b_order_id: Uuid,
    
    /// Overall execution status
    pub status: ArbExecutionStatus,
    
    /// Leg A state
    pub leg_a_state: OrderStatus,
    
    /// Leg B state
    pub leg_b_state: OrderStatus,
    
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    
    /// Completed timestamp (if terminal)
    pub completed_at: Option<DateTime<Utc>>,
}

