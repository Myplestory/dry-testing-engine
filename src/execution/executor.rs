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

        // Submit order to venue
        let response = self
            .venue_adapter
            .submit_order(order.clone(), self.submission_timeout)
            .await
            .map_err(DryTestingError::Venue)?;

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
                // Update router mapping
                self.order_router.update_venue_order_id(
                    self.venue.clone(),
                    &order.client_order_id,
                    &venue_order_id,
                );

                // Create ACK event
                let ack_event = EventSource::VenueAck {
                    venue_order_id,
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, ack_event)
                    .await?;

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
                // Create FILL event
                let fill_event = EventSource::VenueFill {
                    venue_order_id,
                    fill_size,
                    fill_price: (order.limit_price_int, order.price_scale),
                    trade_id,
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, fill_event)
                    .await?;

                // Get updated order state
                let updated_state = self.state_machine.get_order(order.id)?;

                // Notify coordinator
                self.coordinator
                    .update_execution(intent_id, leg, updated_state.status)
                    .await?;
            }

            VenueResponse::Reject {
                venue_order_id,
                reason,
            } => {
                // Create REJECT event
                let reject_event = EventSource::VenueReject {
                    venue_order_id: venue_order_id.unwrap_or_default(),
                    reason,
                    timestamp,
                };

                self.state_machine
                    .process_event(order.id, reject_event)
                    .await?;

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
