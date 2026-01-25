//! Venue executor for direct order execution

use crate::coordinator::ArbExecutionCoordinator;
use crate::routing::OrderRouter;
use crate::state_machine::events::EventSource;
use crate::state_machine::OrderStateMachine;
use crate::types::errors::{DryTestingError, Result};
use crate::types::order::{Order, OrderLeg, OrderStatus};
use crate::types::venue::VenueType;
use crate::venue::adapter::{VenueAdapter, VenueResponse};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};

/// Result of leg execution
#[derive(Debug)]
pub enum LegExecutionResult {
    /// Order was acknowledged
    Acknowledged,
    /// Order was filled (partial or full)
    Filled,
    /// Order was rejected
    Rejected,
}

/// Venue executor for direct order execution
pub struct VenueExecutor {
    /// Venue type
    venue: VenueType,

    /// Venue adapter (simulator or real API)
    venue_adapter: Arc<dyn VenueAdapter>,

    /// Order state machine
    state_machine: Arc<OrderStateMachine>,

    /// Order router for event routing
    order_router: Arc<OrderRouter>,

    /// Coordinator for multi-leg execution
    coordinator: Arc<ArbExecutionCoordinator>,

    /// Submission timeout
    submission_timeout: Duration,
}

impl VenueExecutor {
    /// Create a new venue executor
    pub fn new(
        venue: VenueType,
        venue_adapter: Arc<dyn VenueAdapter>,
        state_machine: Arc<OrderStateMachine>,
        order_router: Arc<OrderRouter>,
        coordinator: Arc<ArbExecutionCoordinator>,
        submission_timeout: Duration,
    ) -> Self {
        Self {
            venue,
            venue_adapter,
            state_machine,
            order_router,
            coordinator,
            submission_timeout,
        }
    }

    /// Execute a leg and return result
    pub async fn execute_leg(
        &self,
        order: Order,
        leg: OrderLeg,
        intent_id: uuid::Uuid,
    ) -> Result<LegExecutionResult> {
        let order_id = order.id;

        info!(
            intent_id = %intent_id,
            order_id = %order_id,
            leg = ?leg,
            venue = ?self.venue,
            client_order_id = %order.client_order_id,
            "Submitting order to venue"
        );

        debug!(
            intent_id = %intent_id,
            order_id = %order_id,
            leg = ?leg,
            venue = ?self.venue,
            venue_market_id = %order.venue_market_id,
            venue_outcome_id = %order.venue_outcome_id,
            side = ?order.side,
            limit_price_int = order.limit_price_int,
            price_scale = order.price_scale,
            size_int = order.size_int,
            size_scale = order.size_scale,
            "Order submission details"
        );

        // Submit order to venue
        let response = self
            .venue_adapter
            .submit_order(order.clone(), self.submission_timeout)
            .await
            .map_err(DryTestingError::Venue)?;

        info!(
            intent_id = %intent_id,
            order_id = %order_id,
            leg = ?leg,
            venue = ?self.venue,
            response = ?response,
            "Received venue response"
        );

        // Process response
        self.process_venue_response(response, order, leg, intent_id)
            .await?;

        // Determine result based on final order status
        let order_state = self.state_machine.get_order(order_id)?;
        match order_state.status {
            OrderStatus::Filled => Ok(LegExecutionResult::Filled),
            OrderStatus::Rejected => Ok(LegExecutionResult::Rejected),
            OrderStatus::Submitted | OrderStatus::Partial => Ok(LegExecutionResult::Acknowledged),
            _ => Ok(LegExecutionResult::Acknowledged),
        }
    }

    /// Process venue response and update state
    async fn process_venue_response(
        &self,
        response: VenueResponse,
        order: Order,
        leg: OrderLeg,
        intent_id: uuid::Uuid,
    ) -> Result<()> {
        let timestamp = chrono::Utc::now();

        match response {
            VenueResponse::Ack { venue_order_id } => {
                debug!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id,
                    "Processing ACK response"
                );

                // Update router mapping
                self.order_router.update_venue_order_id(
                    self.venue.clone(),
                    &order.client_order_id,
                    &venue_order_id,
                );

                // Create ACK event
                let ack_event = EventSource::VenueAck {
                    venue_order_id: venue_order_id.clone(),
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, ack_event)
                    .await?;

                let venue_order_id_clone = venue_order_id.clone();
                info!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id_clone,
                    "Order acknowledged by venue"
                );

                // Notify coordinator
                self.coordinator
                    .update_execution(intent_id, leg, OrderStatus::Submitted)
                    .await?;
            }

            VenueResponse::Fill {
                venue_order_id,
                fill_size,
                fill_price: _,
                trade_id,
            } => {
                debug!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id,
                    fill_size_int = fill_size.0,
                    fill_size_scale = fill_size.1,
                    trade_id = %trade_id,
                    "Processing FILL response"
                );

                // Create FILL event
                let fill_event = EventSource::VenueFill {
                    venue_order_id: venue_order_id.clone(),
                    fill_size,
                    fill_price: (order.limit_price_int, order.price_scale),
                    trade_id: trade_id.clone(),
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, fill_event)
                    .await?;

                // Get updated order state
                let updated_state = self.state_machine.get_order(order.id)?;

                let venue_order_id_clone = venue_order_id.clone();
                let trade_id_clone = trade_id.clone();
                info!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id_clone,
                    trade_id = %trade_id_clone,
                    status = ?updated_state.status,
                    filled_size_int = updated_state.filled_size_int,
                    "Order fill processed"
                );

                // Notify coordinator
                self.coordinator
                    .update_execution(intent_id, leg, updated_state.status)
                    .await?;
            }

            VenueResponse::Reject {
                venue_order_id,
                reason,
            } => {
                let venue_order_id_str = venue_order_id
                    .as_ref()
                    .map(|id| id.as_str())
                    .unwrap_or("none");
                let venue_order_id_for_event = venue_order_id.clone().unwrap_or_default();
                let reason_clone = reason.clone();

                debug!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id_str,
                    reason = %reason_clone,
                    "Processing REJECT response"
                );

                // Create REJECT event
                let reject_event = EventSource::VenueReject {
                    venue_order_id: venue_order_id_for_event,
                    reason: reason_clone.clone(),
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, reject_event)
                    .await?;

                info!(
                    intent_id = %intent_id,
                    order_id = %order.id,
                    leg = ?leg,
                    venue_order_id = %venue_order_id_str,
                    reason = %reason_clone,
                    "Order rejected by venue"
                );

                // Notify coordinator
                self.coordinator
                    .update_execution(intent_id, leg, OrderStatus::Rejected)
                    .await?;
            }
        }

        Ok(())
    }

    /// Get venue type
    pub fn venue(&self) -> &VenueType {
        &self.venue
    }

    /// Get venue adapter reference (for cancel_order)
    pub fn venue_adapter(&self) -> &Arc<dyn VenueAdapter> {
        &self.venue_adapter
    }
}
