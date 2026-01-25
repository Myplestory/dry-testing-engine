//! Order state machine implementation

use crate::db::DatabaseWriter;
use crate::routing::OrderRouter;
use crate::state_machine::events::EventSource;
use crate::types::errors::{Result, StateMachineError};
use crate::types::order::{OrderState, OrderStatus, StateTransition};
use dashmap::DashMap;
use std::sync::Arc;
use tracing::debug;
use uuid::Uuid;

/// Order state machine for managing order lifecycle
pub struct OrderStateMachine {
    /// Active orders cache
    active_orders: Arc<DashMap<Uuid, OrderState>>,

    /// Order router for event routing
    order_router: Arc<OrderRouter>,

    /// Database writer for persistence
    db_writer: Arc<DatabaseWriter>,
}

impl OrderStateMachine {
    /// Create a new order state machine
    pub async fn new(
        order_router: Arc<OrderRouter>,
        db_writer: Arc<DatabaseWriter>,
    ) -> Result<Self> {
        Ok(Self {
            active_orders: Arc::new(DashMap::new()),
            order_router,
            db_writer,
        })
    }

    /// Process an event and transition state
    pub async fn process_event(
        &self,
        order_id: Uuid,
        event: EventSource,
    ) -> Result<StateTransition> {
        let mut order = self.get_order_mut(order_id)?;

        debug!(
            order_id = %order_id,
            current_status = ?order.status,
            event = ?event,
            "Processing state machine event"
        );

        let transition = match event {
            EventSource::VenueAck {
                venue_order_id,
                timestamp,
            } => {
                // pending → submitted
                if order.status == OrderStatus::Pending {
                    order.status = OrderStatus::Submitted;
                    order.venue_order_id = Some(venue_order_id);
                    StateTransition {
                        from: OrderStatus::Pending,
                        to: OrderStatus::Submitted,
                        timestamp,
                        source: "venue_ack".to_string(),
                    }
                } else {
                    return Err(StateMachineError::InvalidTransition(format!(
                        "Cannot ACK order in {:?} state",
                        order.status
                    ))
                    .into());
                }
            }
            EventSource::VenueFill {
                fill_size,
                fill_price: _,
                trade_id,
                timestamp,
                ..
            } => {
                // submitted/partial → partial/filled
                // Validate state before processing (industry standard for correctness)
                if order.status != OrderStatus::Submitted && order.status != OrderStatus::Partial {
                    return Err(StateMachineError::InvalidTransition(format!(
                        "Cannot FILL order in {:?} state (must be Submitted or Partial)",
                        order.status
                    ))
                    .into());
                }
                
                // Use total_size_int and total_size_scale from OrderState for proper comparison
                let new_filled = order.filled_size_int + fill_size.0;

                // Compare filled size with total size (both use same scale)
                let new_status = if new_filled >= order.total_size_int {
                    OrderStatus::Filled
                } else {
                    OrderStatus::Partial
                };

                let from_status = order.status;
                order.status = new_status;
                order.filled_size_int = new_filled;

                StateTransition {
                    from: from_status,
                    to: new_status,
                    timestamp,
                    source: format!("venue_fill_{}", trade_id),
                }
            }
            EventSource::VenueReject {
                reason,
                timestamp,
                venue_order_id: _,
            } => {
                // any → rejected
                let from_status = order.status;
                order.status = OrderStatus::Rejected;
                StateTransition {
                    from: from_status,
                    to: OrderStatus::Rejected,
                    timestamp,
                    source: format!("venue_reject: {}", reason),
                }
            }
            EventSource::CoordinatorTimeout {
                timestamp,
                order_id: _,
            } => {
                // any → canceled
                let from_status = order.status;
                order.status = OrderStatus::Canceled;
                StateTransition {
                    from: from_status,
                    to: OrderStatus::Canceled,
                    timestamp,
                    source: "coordinator_timeout".to_string(),
                }
            }
            EventSource::CoordinatorCancel {
                reason,
                timestamp,
                order_id: _,
            } => {
                // any → canceled
                let from_status = order.status;
                order.status = OrderStatus::Canceled;
                StateTransition {
                    from: from_status,
                    to: OrderStatus::Canceled,
                    timestamp,
                    source: format!("coordinator_cancel: {}", reason),
                }
            }
        };

        order.transitions.push(transition.clone());
        order.updated_at = chrono::Utc::now();

        debug!(
            order_id = %order_id,
            transition = ?transition,
            new_status = ?order.status,
            "State transition completed"
        );

        // Queue transition for DB write (batched)
        self.db_writer
            .queue_transition(order_id, transition.clone())
            .await?;

        Ok(transition)
    }

    /// Get order state (immutable)
    pub fn get_order(&self, order_id: Uuid) -> Result<OrderState> {
        self.active_orders
            .get(&order_id)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| StateMachineError::OrderNotFound(order_id).into())
    }

    /// Get order state (mutable)
    fn get_order_mut(
        &self,
        order_id: Uuid,
    ) -> Result<dashmap::mapref::one::RefMut<'_, Uuid, OrderState>> {
        self.active_orders
            .get_mut(&order_id)
            .ok_or_else(|| StateMachineError::OrderNotFound(order_id).into())
    }

    /// Restore order state (for recovery)
    pub async fn restore_order(&self, order: OrderState) -> Result<()> {
        self.active_orders.insert(order.order_id, order);
        Ok(())
    }
}
