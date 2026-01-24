//! Venue simulator for dry testing

use crate::types::errors::VenueError;
use crate::types::order::{Order, OrderStatus};
use crate::types::venue::VenueType;
use crate::venue::adapter::{VenueAdapter, VenueResponse};
use async_trait::async_trait;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing::debug;

/// Fill event callback for simulator
pub type FillCallback = Box<dyn Fn(String, (i64, i16), (i64, i16), String) + Send + Sync>;

/// Venue simulator for dry testing
pub struct VenueSimulator {
    venue: VenueType,
    ack_latency_ms: u64,
    fill_latency_ms: u64,
    reject_probability: f64,
    fill_probability: f64,
    partial_fill_probability: f64,
    fill_callback: Option<Arc<FillCallback>>,
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
            reject_probability: reject_probability.clamp(0.0, 1.0),
            fill_probability: fill_probability.clamp(0.0, 1.0),
            partial_fill_probability: 0.0,
            fill_callback: None,
        }
    }

    /// Set partial fill probability (0.0 - 1.0)
    pub fn with_partial_fill_probability(mut self, prob: f64) -> Self {
        self.partial_fill_probability = prob.clamp(0.0, 1.0);
        self
    }

    /// Set fill event callback
    pub fn with_fill_callback(mut self, callback: Arc<FillCallback>) -> Self {
        self.fill_callback = Some(callback);
        self
    }

    /// Generate a fill event for an order
    async fn generate_fill(&self, order: &Order, venue_order_id: &str) {
        sleep(Duration::from_millis(self.fill_latency_ms)).await;

        let mut rng = rand::thread_rng();
        let is_partial = rng.gen::<f64>() < self.partial_fill_probability;

        // Calculate fill size
        let fill_size = if is_partial {
            // Partial fill: 50-90% of order size
            let fill_percentage = rng.gen_range(0.5..0.9);
            let fill_amount = (order.size_int as f64 * fill_percentage) as i64;
            (fill_amount, order.size_scale)
        } else {
            // Full fill
            (order.size_int, order.size_scale)
        };

        // Use order price as fill price (simplified - real venues have slippage)
        let fill_price = (order.limit_price_int, order.price_scale);

        // Generate unique trade_id
        let trade_id = format!("sim_trade_{}", uuid::Uuid::new_v4());

        // Emit fill event via callback if set
        if let Some(ref callback) = self.fill_callback {
            callback(venue_order_id.to_string(), fill_size, fill_price, trade_id);
        } else {
            debug!(
                "Simulated fill for order {}: size={:?}, price={:?}, trade_id={}",
                order.id, fill_size, fill_price, trade_id
            );
        }
    }
}

#[async_trait]
impl VenueAdapter for VenueSimulator {
    async fn submit_order(
        &self,
        order: Order,
        timeout_duration: Duration,
    ) -> Result<VenueResponse, VenueError> {
        // Simulate ACK latency with timeout
        let ack_delay = Duration::from_millis(self.ack_latency_ms);

        match timeout(timeout_duration, sleep(ack_delay)).await {
            Ok(_) => {
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

                // Simulate fill (if configured) - spawn async task
                if rng.gen::<f64>() < self.fill_probability {
                    let simulator = VenueSimulator {
                        venue: self.venue.clone(),
                        ack_latency_ms: self.ack_latency_ms,
                        fill_latency_ms: self.fill_latency_ms,
                        reject_probability: self.reject_probability,
                        fill_probability: self.fill_probability,
                        partial_fill_probability: self.partial_fill_probability,
                        fill_callback: self.fill_callback.clone(),
                    };
                    let order_clone = order.clone();
                    let venue_order_id_clone = venue_order_id.clone();

                    tokio::spawn(async move {
                        simulator
                            .generate_fill(&order_clone, &venue_order_id_clone)
                            .await;
                    });
                }

                Ok(VenueResponse::Ack { venue_order_id })
            }
            Err(_) => Err(VenueError::SubmissionFailed(format!(
                "Order submission timed out after {:?}",
                timeout_duration
            ))),
        }
    }

    async fn get_order_status(
        &self,
        _venue_order_id: &str,
        timeout_duration: Duration,
    ) -> Result<OrderStatus, VenueError> {
        // Simulate status lookup latency
        let lookup_delay = Duration::from_millis(10); // Fast lookup

        match timeout(timeout_duration, sleep(lookup_delay)).await {
            Ok(_) => Ok(OrderStatus::Submitted),
            Err(_) => Err(VenueError::InvalidResponse(
                "Status lookup timed out".to_string(),
            )),
        }
    }

    async fn cancel_order(
        &self,
        _venue_order_id: &str,
        timeout_duration: Duration,
    ) -> Result<(), VenueError> {
        // Simulate cancellation latency
        let cancel_delay = Duration::from_millis(50);

        match timeout(timeout_duration, sleep(cancel_delay)).await {
            Ok(_) => Ok(()),
            Err(_) => Err(VenueError::CancellationFailed(format!(
                "Order cancellation timed out after {:?}",
                timeout_duration
            ))),
        }
    }

    fn venue_type(&self) -> VenueType {
        self.venue.clone()
    }
}
