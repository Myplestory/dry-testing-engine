//! Venue adapter trait

use crate::types::errors::VenueError;
use crate::types::order::Order;
use crate::types::order::OrderStatus;
use crate::types::venue::VenueType;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Venue response from order submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VenueResponse {
    /// Order acknowledged
    Ack { venue_order_id: String },

    /// Order filled (partial or full)
    Fill {
        venue_order_id: String,
        fill_size: (i64, i16),  // (int, scale)
        fill_price: (i64, i16), // (int, scale)
        trade_id: String,
    },

    /// Order rejected
    Reject {
        venue_order_id: Option<String>,
        reason: String,
    },
}

/// Venue adapter trait (implemented by simulator and real venues)
#[async_trait]
pub trait VenueAdapter: Send + Sync {
    /// Submit order to venue with timeout
    ///
    /// # Arguments
    /// * `order` - Order to submit
    /// * `timeout` - Maximum time to wait for response
    ///
    /// # Returns
    /// * `Ok(VenueResponse)` - Order was submitted (ACK, FILL, or REJECT)
    /// * `Err(VenueError)` - Submission failed or timed out
    async fn submit_order(
        &self,
        order: Order,
        timeout: Duration,
    ) -> Result<VenueResponse, VenueError>;

    /// Get order status with timeout
    ///
    /// # Arguments
    /// * `venue_order_id` - Venue's order identifier
    /// * `timeout` - Maximum time to wait for response
    ///
    /// # Returns
    /// * `Ok(OrderStatus)` - Current order status
    /// * `Err(VenueError)` - Status lookup failed or timed out
    async fn get_order_status(
        &self,
        venue_order_id: &str,
        timeout: Duration,
    ) -> Result<OrderStatus, VenueError>;

    /// Cancel order with timeout
    ///
    /// # Arguments
    /// * `venue_order_id` - Venue's order identifier
    /// * `timeout` - Maximum time to wait for response
    ///
    /// # Returns
    /// * `Ok(())` - Order was canceled successfully
    /// * `Err(VenueError)` - Cancellation failed or timed out
    async fn cancel_order(&self, venue_order_id: &str, timeout: Duration)
        -> Result<(), VenueError>;

    /// Get venue type
    fn venue_type(&self) -> VenueType;
}
