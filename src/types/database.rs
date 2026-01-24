//! Database type conversions and utilities
//!
//! Provides conversions between Rust types and database types,
//! ensuring type safety and preventing data corruption.

use crate::types::order::{OrderLeg, OrderSide, OrderStatus};
use crate::types::venue::VenueType;
use sqlx::FromRow;
use std::str::FromStr;

/// Convert OrderStatus to database string (for SQL queries)
impl From<OrderStatus> for &str {
    fn from(status: OrderStatus) -> Self {
        match status {
            OrderStatus::Pending => "pending",
            OrderStatus::Submitted => "submitted",
            OrderStatus::Partial => "partial",
            OrderStatus::Filled => "filled",
            OrderStatus::Rejected => "rejected",
            OrderStatus::Canceled => "canceled",
        }
    }
}

/// Convert OrderSide to database string
impl From<OrderSide> for &str {
    fn from(side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
        }
    }
}

/// Convert OrderLeg to database string
impl From<OrderLeg> for &str {
    fn from(leg: OrderLeg) -> Self {
        match leg {
            OrderLeg::A => "A",
            OrderLeg::B => "B",
        }
    }
}

/// Database row for orders table
#[derive(Debug, Clone, FromRow)]
pub struct OrderRow {
    pub id: uuid::Uuid,
    pub customer_id: uuid::Uuid,
    pub strategy_id: uuid::Uuid,
    pub pair_id: uuid::Uuid,
    pub leg: String,   // "A" or "B"
    pub venue: String, // venue_type enum
    pub venue_market_id: String,
    pub venue_outcome_id: String,
    pub client_order_id: String,
    pub venue_order_id: Option<String>,
    pub side: String, // "buy" or "sell"
    pub limit_price_int: i64,
    pub price_scale: i16,
    pub size_int: i64,
    pub size_scale: i16,
    pub status: String, // order_status enum
    pub filled_size_int: i64,
    pub filled_size_scale: i16,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl TryFrom<OrderRow> for crate::types::order::Order {
    type Error = String;

    fn try_from(row: OrderRow) -> Result<Self, Self::Error> {
        Ok(crate::types::order::Order {
            id: row.id,
            customer_id: row.customer_id,
            strategy_id: row.strategy_id,
            pair_id: row.pair_id,
            leg: OrderLeg::from_str(&row.leg)?,
            venue: VenueType::from_str(&row.venue).map_err(|e| format!("Invalid venue: {}", e))?,
            venue_market_id: row.venue_market_id,
            venue_outcome_id: row.venue_outcome_id,
            client_order_id: row.client_order_id,
            venue_order_id: row.venue_order_id,
            side: OrderSide::from_str(&row.side)?,
            limit_price_int: row.limit_price_int,
            price_scale: row.price_scale,
            size_int: row.size_int,
            size_scale: row.size_scale,
            status: OrderStatus::from_str(&row.status)?,
            filled_size_int: row.filled_size_int,
            filled_size_scale: row.filled_size_scale,
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_status_database_conversion() {
        let status = OrderStatus::Filled;
        let db_str: &str = status.into();
        assert_eq!(db_str, "filled");

        let parsed = OrderStatus::from_str(db_str).unwrap();
        assert_eq!(parsed, OrderStatus::Filled);
    }

    #[test]
    fn test_order_side_database_conversion() {
        let side = OrderSide::Buy;
        let db_str: &str = side.into();
        assert_eq!(db_str, "buy");

        let parsed = OrderSide::from_str(db_str).unwrap();
        assert_eq!(parsed, OrderSide::Buy);
    }

    #[test]
    fn test_order_leg_database_conversion() {
        let leg = OrderLeg::A;
        let db_str: &str = leg.into();
        assert_eq!(db_str, "A");

        let parsed = OrderLeg::from_str(db_str).unwrap();
        assert_eq!(parsed, OrderLeg::A);
    }
}
