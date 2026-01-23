//! Venue adapter trait

use crate::types::errors::VenueError;
use crate::types::order::Order;
use crate::types::order::OrderStatus;
use crate::types::venue::VenueType;
use async_trait::async_trait;

/// Venue adapter trait (implemented by simulator and real venues)
#[async_trait]
pub trait VenueAdapter: Send + Sync {
    /// Submit order to venue
    async fn submit_order(&self, order: Order) -> Result<VenueResponse, VenueError>;
    
    /// Get order status
    async fn get_order_status(&self, venue_order_id: &str) -> Result<OrderStatus, VenueError>;
    
    /// Cancel order
    async fn cancel_order(&self, venue_order_id: &str) -> Result<(), VenueError>;
    
    /// Get venue type
    fn venue_type(&self) -> VenueType;
}

/// Venue response from order submission
#[derive(Debug, Clone)]
pub enum VenueResponse {
    /// Order acknowledged
    Ack {
        venue_order_id: String,
    },
    
    /// Order filled (partial or full)
    Fill {
        venue_order_id: String,
        fill_size: (i64, i16),
        fill_price: (i64, i16),
        trade_id: String,
    },
    
    /// Order rejected
    Reject {
        venue_order_id: Option<String>,
        reason: String,
    },
}

