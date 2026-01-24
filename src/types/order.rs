//! Order and state machine types

use crate::types::venue::VenueType;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Order leg identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OrderLeg {
    A,
    B,
}

impl std::fmt::Display for OrderLeg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OrderLeg::A => "A",
            OrderLeg::B => "B",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for OrderLeg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "A" => Ok(OrderLeg::A),
            "B" => Ok(OrderLeg::B),
            _ => Err(format!("Invalid order leg: {}", s)),
        }
    }
}

impl From<OrderLeg> for String {
    fn from(leg: OrderLeg) -> Self {
        leg.to_string()
    }
}

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

impl std::fmt::Display for OrderSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for OrderSide {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "buy" => Ok(OrderSide::Buy),
            "sell" => Ok(OrderSide::Sell),
            _ => Err(format!("Invalid order side: {}", s)),
        }
    }
}

impl From<OrderSide> for String {
    fn from(side: OrderSide) -> Self {
        side.to_string()
    }
}

/// Order status (matches database enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
        matches!(
            self,
            OrderStatus::Filled | OrderStatus::Rejected | OrderStatus::Canceled
        )
    }
}

impl std::fmt::Display for OrderStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            OrderStatus::Pending => "pending",
            OrderStatus::Submitted => "submitted",
            OrderStatus::Partial => "partial",
            OrderStatus::Filled => "filled",
            OrderStatus::Rejected => "rejected",
            OrderStatus::Canceled => "canceled",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for OrderStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(OrderStatus::Pending),
            "submitted" => Ok(OrderStatus::Submitted),
            "partial" => Ok(OrderStatus::Partial),
            "filled" => Ok(OrderStatus::Filled),
            "rejected" => Ok(OrderStatus::Rejected),
            "canceled" | "cancelled" => Ok(OrderStatus::Canceled),
            _ => Err(format!("Invalid order status: {}", s)),
        }
    }
}

impl From<OrderStatus> for String {
    fn from(status: OrderStatus) -> Self {
        status.to_string()
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

    /// Client order ID (for router lookups)
    pub client_order_id: String,

    /// Current status
    pub status: OrderStatus,

    /// Sequence number for ordering
    pub sequence_number: u64,

    /// Filled size (fixed-point: int, scale)
    pub filled_size_int: i64,
    pub filled_size_scale: i16,

    /// Total order size (fixed-point: int, scale) - needed for fill completion check
    pub total_size_int: i64,
    pub total_size_scale: i16,

    /// Venue type (needed for cancel_order lookup)
    pub venue: crate::types::venue::VenueType,

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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
        matches!(
            self,
            ArbExecutionStatus::Completed
                | ArbExecutionStatus::Failed
                | ArbExecutionStatus::Canceled
        )
    }
}

impl std::fmt::Display for ArbExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ArbExecutionStatus::Pending => "pending",
            ArbExecutionStatus::Completed => "completed",
            ArbExecutionStatus::Failed => "failed",
            ArbExecutionStatus::Canceled => "canceled",
            ArbExecutionStatus::Partial => "partial",
        };
        write!(f, "{}", s)
    }
}

impl std::str::FromStr for ArbExecutionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pending" => Ok(ArbExecutionStatus::Pending),
            "completed" => Ok(ArbExecutionStatus::Completed),
            "failed" => Ok(ArbExecutionStatus::Failed),
            "canceled" | "cancelled" => Ok(ArbExecutionStatus::Canceled),
            "partial" => Ok(ArbExecutionStatus::Partial),
            _ => Err(format!("Invalid execution status: {}", s)),
        }
    }
}

impl From<ArbExecutionStatus> for String {
    fn from(status: ArbExecutionStatus) -> Self {
        status.to_string()
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
