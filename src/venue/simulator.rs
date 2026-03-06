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

pub type FillCallback = Box<dyn Fn(String, (i64, i16), (i64, i16), String) + Send + Sync>;

/// Probabilities stored as basis points (0-10_000 where 10_000 = 100%).
pub struct VenueSimulator {
    venue: VenueType,
    ack_latency_ms: u64,
    fill_latency_ms: u64,
    reject_probability_bps: u32,
    fill_probability_bps: u32,
    partial_fill_probability_bps: u32,
    fill_callback: Option<Arc<FillCallback>>,
}

impl VenueSimulator {
    pub fn new(
        venue: VenueType,
        ack_latency_ms: u64,
        fill_latency_ms: u64,
        reject_probability_bps: u32,
        fill_probability_bps: u32,
    ) -> Self {
        Self {
            venue,
            ack_latency_ms,
            fill_latency_ms,
            reject_probability_bps: reject_probability_bps.min(10_000),
            fill_probability_bps: fill_probability_bps.min(10_000),
            partial_fill_probability_bps: 0,
            fill_callback: None,
        }
    }

    pub fn with_partial_fill_probability_bps(mut self, bps: u32) -> Self {
        self.partial_fill_probability_bps = bps.min(10_000);
        self
    }

    pub fn with_fill_callback(mut self, callback: Arc<FillCallback>) -> Self {
        self.fill_callback = Some(callback);
        self
    }

    async fn generate_fill(&self, order: &Order, venue_order_id: &str) {
        sleep(Duration::from_millis(self.fill_latency_ms)).await;

        let mut rng = rand::thread_rng();
        let is_partial = rng.gen_range(0u32..10_000) < self.partial_fill_probability_bps;

        let fill_size = if is_partial {
            // Partial fill: 50%-90% using integer arithmetic
            let pct_bps = rng.gen_range(5000u32..9000);
            let fill_amount = order.size_int.saturating_mul(pct_bps as i64) / 10_000;
            (fill_amount, order.size_scale)
        } else {
            (order.size_int, order.size_scale)
        };

        let fill_price = (order.limit_price_int, order.price_scale);
        let trade_id = format!("sim_trade_{}", uuid::Uuid::new_v4());

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
        let ack_delay = Duration::from_millis(self.ack_latency_ms);

        match timeout(timeout_duration, sleep(ack_delay)).await {
            Ok(_) => {
                let mut rng = rand::thread_rng();
                if rng.gen_range(0u32..10_000) < self.reject_probability_bps {
                    return Ok(VenueResponse::Reject {
                        venue_order_id: None,
                        reason: "Simulated rejection".to_string(),
                    });
                }

                let venue_order_id = format!("sim_{}", uuid::Uuid::new_v4());

                if rng.gen_range(0u32..10_000) < self.fill_probability_bps {
                    let simulator = VenueSimulator {
                        venue: self.venue.clone(),
                        ack_latency_ms: self.ack_latency_ms,
                        fill_latency_ms: self.fill_latency_ms,
                        reject_probability_bps: self.reject_probability_bps,
                        fill_probability_bps: self.fill_probability_bps,
                        partial_fill_probability_bps: self.partial_fill_probability_bps,
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
        let lookup_delay = Duration::from_millis(10);

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
