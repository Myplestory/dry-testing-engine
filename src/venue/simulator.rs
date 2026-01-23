//! Venue simulator for dry testing

use crate::types::errors::VenueError;
use crate::types::order::{Order, OrderStatus};
use crate::types::venue::VenueType;
use crate::venue::adapter::{VenueAdapter, VenueResponse};
use async_trait::async_trait;
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;

/// Venue simulator for dry testing
pub struct VenueSimulator {
    venue: VenueType,
    ack_latency_ms: u64,
    fill_latency_ms: u64,
    reject_probability: f64,
    fill_probability: f64,
}

impl VenueSimulator {
    /// Create a new venue simulator
    pub fn new(
        venue: VenueType,
        ack_latency_ms: u64,
        fill_latency_ms: u64,
        reject_probability: f64,
        fill_probability: f64,
    ) -> Self {
        Self {
            venue,
            ack_latency_ms,
            fill_latency_ms,
            reject_probability,
            fill_probability,
        }
    }
}

#[async_trait]
impl VenueAdapter for VenueSimulator {
    async fn submit_order(&self, order: Order) -> Result<VenueResponse, VenueError> {
        // Simulate ACK latency
        sleep(Duration::from_millis(self.ack_latency_ms)).await;
        
        // Simulate rejection
        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < self.reject_probability {
            return Ok(VenueResponse::Reject {
                venue_order_id: None,
                reason: "Simulated rejection".to_string(),
            });
        }
        
        // Generate venue_order_id
        let venue_order_id = format!("sim_{}", uuid::Uuid::new_v4());
        
        // Simulate fill (if configured)
        if rng.gen::<f64>() < self.fill_probability {
            // Spawn async task to simulate fill after delay
            let order_id = order.id;
            let venue_order_id_clone = venue_order_id.clone();
            let fill_latency = self.fill_latency_ms;
            
            tokio::spawn(async move {
                sleep(Duration::from_millis(fill_latency)).await;
                // TODO: Emit fill event via callback or channel
                tracing::debug!("Simulated fill for order {}", order_id);
            });
        }
        
        Ok(VenueResponse::Ack { venue_order_id })
    }
    
    async fn get_order_status(&self, _venue_order_id: &str) -> Result<OrderStatus, VenueError> {
        // TODO: Implement status lookup
        Ok(OrderStatus::Submitted)
    }
    
    async fn cancel_order(&self, _venue_order_id: &str) -> Result<(), VenueError> {
        // TODO: Implement cancellation
        Ok(())
    }
    
    fn venue_type(&self) -> VenueType {
        self.venue.clone()
    }
}

