//! Arbitrage execution coordinator implementation

use crate::db::DatabaseWriter;
use crate::execution::executor::{LegExecutionResult, VenueExecutor};
use crate::routing::OrderRouter;
use crate::state_machine::{EventSource, OrderStateMachine};
use crate::types::errors::{CoordinatorError, DryTestingError, Result};
use crate::types::execution::ArbExecutionIntent;
use crate::types::order::ArbExecutionStatus;
use crate::types::order::{ArbExecution, Order, OrderLeg, OrderStatus};
use crate::types::venue::VenueType;
use dashmap::DashMap;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tracing::{error, info};
use uuid::Uuid;

/// Coordinates multi-leg arbitrage execution
pub struct ArbExecutionCoordinator {
    /// Active executions
    active_executions: Arc<DashMap<Uuid, ArbExecution>>,

    /// Order state machine
    state_machine: Arc<OrderStateMachine>,

    /// Order router
    order_router: Arc<OrderRouter>,

    /// Pre-allocated venue executors (no queues, direct execution)
    venue_executors: Arc<DashMap<VenueType, Arc<VenueExecutor>>>,

    /// Database writer
    db_writer: Arc<DatabaseWriter>,

    /// Database connection pool
    db: Arc<PgPool>,
}

impl ArbExecutionCoordinator {
    /// Create a new coordinator
    pub async fn new(
        state_machine: Arc<OrderStateMachine>,
        order_router: Arc<OrderRouter>,
        db_writer: Arc<DatabaseWriter>,
        db: Arc<PgPool>,
    ) -> Result<Self> {
        Ok(Self {
            active_executions: Arc::new(DashMap::new()),
            state_machine,
            order_router,
            venue_executors: Arc::new(DashMap::new()),
            db_writer,
            db,
        })
    }

    /// Register a venue executor (called during engine initialization)
    ///
    /// # Arguments
    /// * `venue` - Venue type to register
    /// * `executor` - Venue executor instance
    pub fn register_executor(&self, venue: VenueType, executor: Arc<VenueExecutor>) {
        self.venue_executors.insert(venue, executor);
    }

    /// Get venue executor for a venue
    fn get_venue_executor(&self, venue: &VenueType) -> Result<Arc<VenueExecutor>> {
        self.venue_executors
            .get(venue)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| {
                DryTestingError::Coordinator(CoordinatorError::UnknownVenue(
                    venue.as_str().to_string(),
                ))
            })
    }

    /// Enqueue an execution intent
    pub async fn enqueue_intent(&self, intent: ArbExecutionIntent) -> Result<()> {
        // Validate intent
        intent.validate().map_err(|e| {
            DryTestingError::Coordinator(CoordinatorError::CompensationFailed(format!(
                "Intent validation failed: {}",
                e
            )))
        })?;

        // Generate client_order_ids for both legs
        let (client_order_id_a, client_order_id_b) = intent.generate_client_order_ids();

        // Create Order for Leg A
        let order_a = Order {
            id: uuid::Uuid::new_v4(),
            customer_id: intent.customer_id,
            strategy_id: intent.strategy_id,
            pair_id: intent.pair_id,
            leg: OrderLeg::A,
            venue: intent.leg_a.venue.clone(),
            venue_market_id: intent.leg_a.venue_market_id.clone(),
            venue_outcome_id: intent.leg_a.venue_outcome_id.clone(),
            client_order_id: client_order_id_a.clone(),
            venue_order_id: None,
            side: intent.leg_a.side,
            limit_price_int: intent.leg_a.price_int,
            price_scale: intent.leg_a.price_scale,
            size_int: intent.leg_a.size_int,
            size_scale: intent.leg_a.size_scale,
            status: OrderStatus::Pending,
            filled_size_int: 0,
            filled_size_scale: intent.leg_a.size_scale,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Create Order for Leg B
        let order_b = Order {
            id: uuid::Uuid::new_v4(),
            customer_id: intent.customer_id,
            strategy_id: intent.strategy_id,
            pair_id: intent.pair_id,
            leg: OrderLeg::B,
            venue: intent.leg_b.venue.clone(),
            venue_market_id: intent.leg_b.venue_market_id.clone(),
            venue_outcome_id: intent.leg_b.venue_outcome_id.clone(),
            client_order_id: client_order_id_b.clone(),
            venue_order_id: None,
            side: intent.leg_b.side,
            limit_price_int: intent.leg_b.price_int,
            price_scale: intent.leg_b.price_scale,
            size_int: intent.leg_b.size_int,
            size_scale: intent.leg_b.size_scale,
            status: OrderStatus::Pending,
            filled_size_int: 0,
            filled_size_scale: intent.leg_b.size_scale,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        // Write orders to database immediately (need order_id for tracking)
        let order_a_id = self.db_writer.write_order(&order_a).await?;
        let order_b_id = self.db_writer.write_order(&order_b).await?;

        // Update orders with database IDs
        let mut order_a = order_a;
        let mut order_b = order_b;
        order_a.id = order_a_id;
        order_b.id = order_b_id;

        // Register orders with router
        self.order_router.register_order(&order_a);
        self.order_router.register_order(&order_b);

        // Create OrderState for state machine cache
        let order_state_a = crate::types::order::OrderState {
            order_id: order_a.id,
            intent_id: intent.intent_id,
            leg: OrderLeg::A,
            client_order_id: client_order_id_a.clone(),
            status: OrderStatus::Pending,
            sequence_number: intent.sequence_number,
            filled_size_int: 0,
            filled_size_scale: intent.leg_a.size_scale,
            total_size_int: intent.leg_a.size_int,
            total_size_scale: intent.leg_a.size_scale,
            venue: intent.leg_a.venue.clone(),
            venue_order_id: None,
            transitions: Vec::new(),
            created_at: order_a.created_at,
            updated_at: order_a.updated_at,
        };

        let order_state_b = crate::types::order::OrderState {
            order_id: order_b.id,
            intent_id: intent.intent_id,
            leg: OrderLeg::B,
            client_order_id: client_order_id_b.clone(),
            status: OrderStatus::Pending,
            sequence_number: intent.sequence_number,
            filled_size_int: 0,
            filled_size_scale: intent.leg_b.size_scale,
            total_size_int: intent.leg_b.size_int,
            total_size_scale: intent.leg_b.size_scale,
            venue: intent.leg_b.venue.clone(),
            venue_order_id: None,
            transitions: Vec::new(),
            created_at: order_b.created_at,
            updated_at: order_b.updated_at,
        };

        // Restore orders in state machine cache
        self.state_machine.restore_order(order_state_a).await?;
        self.state_machine.restore_order(order_state_b).await?;

        // Create execution tracking
        self.create_execution(&intent, order_a.id, order_b.id)
            .await?;

        // Get venue executors
        let executor_a = self.get_venue_executor(&intent.leg_a.venue)?;
        let executor_b = self.get_venue_executor(&intent.leg_b.venue)?;

        // Clone necessary data for spawned tasks
        let order_a_clone = order_a.clone();
        let order_b_clone = order_b.clone();
        let intent_id = intent.intent_id;

        // Spawn both legs as independent tasks
        let mut handle_a = tokio::spawn(async move {
            executor_a
                .execute_leg(order_a_clone, OrderLeg::A, intent_id)
                .await
        });

        let mut handle_b = tokio::spawn(async move {
            executor_b
                .execute_leg(order_b_clone, OrderLeg::B, intent_id)
                .await
        });

        // Race both legs - first to complete determines outcome
        let final_status = tokio::select! {
            result_a = &mut handle_a => {
                self.handle_leg_completion(
                    result_a,
                    handle_b,
                    OrderLeg::A,
                    order_a.id,
                    order_b.id,
                    intent_id,
                ).await?
            }
            result_b = &mut handle_b => {
                self.handle_leg_completion(
                    result_b,
                    handle_a,
                    OrderLeg::B,
                    order_b.id,
                    order_a.id,
                    intent_id,
                ).await?
            }
        };

        info!(
            "Execution {} completed with status: {:?}",
            intent_id, final_status
        );
        Ok(())
    }

    /// Create execution when orders are created
    pub async fn create_execution(
        &self,
        intent: &ArbExecutionIntent,
        leg_a_order_id: Uuid,
        leg_b_order_id: Uuid,
    ) -> Result<Uuid> {
        let execution = ArbExecution {
            intent_id: intent.intent_id,
            leg_a_order_id,
            leg_b_order_id,
            status: ArbExecutionStatus::Pending,
            leg_a_state: OrderStatus::Pending,
            leg_b_state: OrderStatus::Pending,
            created_at: chrono::Utc::now(),
            completed_at: None,
        };

        self.active_executions.insert(intent.intent_id, execution);

        // TODO: Persist to DB

        Ok(intent.intent_id)
    }

    /// Update execution when order state changes
    pub async fn update_execution(
        &self,
        intent_id: Uuid,
        leg: OrderLeg,
        new_status: OrderStatus,
    ) -> Result<ArbExecutionStatus> {
        let mut execution = self
            .active_executions
            .get_mut(&intent_id)
            .ok_or_else(|| CoordinatorError::ExecutionNotFound(intent_id))?;

        // Update leg state
        match leg {
            OrderLeg::A => execution.leg_a_state = new_status,
            OrderLeg::B => execution.leg_b_state = new_status,
        }

        // Evaluate overall status
        let new_status = self.evaluate_execution_status(&execution);
        execution.status = new_status;

        // TODO: Persist update

        // Cleanup if terminal
        if new_status.is_terminal() {
            execution.completed_at = Some(chrono::Utc::now());
            // TODO: Remove from active_executions after persistence
        }

        Ok(new_status)
    }

    /// Evaluate overall execution status
    fn evaluate_execution_status(&self, execution: &ArbExecution) -> ArbExecutionStatus {
        match (&execution.leg_a_state, &execution.leg_b_state) {
            (OrderStatus::Filled, OrderStatus::Filled) => ArbExecutionStatus::Completed,
            (OrderStatus::Rejected, _) | (_, OrderStatus::Rejected) => ArbExecutionStatus::Failed,
            (OrderStatus::Canceled, _) | (_, OrderStatus::Canceled) => ArbExecutionStatus::Canceled,
            (OrderStatus::Filled, _) | (_, OrderStatus::Filled) => ArbExecutionStatus::Partial,
            _ => ArbExecutionStatus::Pending,
        }
    }

    /// Handle leg completion and coordinate with other leg
    async fn handle_leg_completion(
        &self,
        result: std::result::Result<Result<LegExecutionResult>, tokio::task::JoinError>,
        mut other_handle: JoinHandle<Result<LegExecutionResult>>,
        completed_leg: OrderLeg,
        completed_order_id: Uuid,
        other_order_id: Uuid,
        intent_id: Uuid,
    ) -> Result<ArbExecutionStatus> {
        match result {
            Ok(Ok(LegExecutionResult::Filled)) | Ok(Ok(LegExecutionResult::Acknowledged)) => {
                // This leg succeeded - wait for other leg
                match other_handle.await {
                    Ok(Ok(LegExecutionResult::Filled))
                    | Ok(Ok(LegExecutionResult::Acknowledged)) => {
                        // Both succeeded - COMPLETED
                        let other_leg = match completed_leg {
                            OrderLeg::A => OrderLeg::B,
                            OrderLeg::B => OrderLeg::A,
                        };
                        self.update_execution(intent_id, other_leg, OrderStatus::Filled)
                            .await?;
                        Ok(ArbExecutionStatus::Completed)
                    }
                    Ok(Ok(LegExecutionResult::Rejected)) | Ok(Err(_)) | Err(_) => {
                        // Other leg failed - rollback this leg (handle already consumed by await)
                        self.cancel_order(
                            completed_order_id,
                            "Rollback: other leg failed".to_string(),
                        )
                        .await?;
                        let other_leg = match completed_leg {
                            OrderLeg::A => OrderLeg::B,
                            OrderLeg::B => OrderLeg::A,
                        };
                        self.update_execution(intent_id, other_leg, OrderStatus::Rejected)
                            .await?;
                        Ok(ArbExecutionStatus::Failed)
                    }
                }
            }
            Ok(Ok(LegExecutionResult::Rejected)) => {
                // This leg failed - abort other leg immediately
                other_handle.abort();
                self.cancel_order(
                    other_order_id,
                    format!("Rollback: {} leg failed", completed_leg),
                )
                .await?;
                self.update_execution(intent_id, completed_leg, OrderStatus::Rejected)
                    .await?;
                Ok(ArbExecutionStatus::Failed)
            }
            Ok(Err(e)) => {
                // Execution error - abort other leg immediately
                other_handle.abort();
                self.cancel_order(
                    other_order_id,
                    format!("Rollback: {} leg execution error", completed_leg),
                )
                .await?;
                self.update_execution(intent_id, completed_leg, OrderStatus::Rejected)
                    .await?;
                error!("Leg {} execution error: {:?}", completed_leg, e);
                Ok(ArbExecutionStatus::Failed)
            }
            Err(e) => {
                // Task panicked or was aborted
                other_handle.abort();
                self.cancel_order(
                    other_order_id,
                    format!("Rollback: {} leg task failed", completed_leg),
                )
                .await?;
                self.update_execution(intent_id, completed_leg, OrderStatus::Rejected)
                    .await?;
                error!("Leg {} task error: {:?}", completed_leg, e);
                Ok(ArbExecutionStatus::Failed)
            }
        }
    }

    /// Cancel an order (for rollback)
    pub async fn cancel_order(&self, order_id: Uuid, reason: String) -> Result<()> {
        // Get order state
        let order_state = self.state_machine.get_order(order_id)?;

        // Create cancel event
        let cancel_event = EventSource::CoordinatorCancel {
            order_id,
            reason: reason.clone(),
            timestamp: chrono::Utc::now(),
        };

        // Process cancel in state machine
        self.state_machine
            .process_event(order_id, cancel_event)
            .await?;

        // If order was submitted to venue, cancel via venue adapter
        if let Some(venue_order_id) = order_state.venue_order_id {
            if let Ok(executor) = self.get_venue_executor(&order_state.venue) {
                // Attempt to cancel at venue (best effort, don't fail if it errors)
                if let Err(e) = executor
                    .venue_adapter()
                    .cancel_order(&venue_order_id, Duration::from_secs(5))
                    .await
                {
                    tracing::warn!(
                        "Failed to cancel order {} at venue: {:?}",
                        venue_order_id,
                        e
                    );
                }
            }
        }

        Ok(())
    }

    /// Recover executions from database
    pub async fn recover_executions(&self) -> Result<()> {
        // TODO: Load active executions from DB
        // TODO: Rebuild active_executions map

        Ok(())
    }
}
