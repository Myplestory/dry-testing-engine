//! Arbitrage execution coordinator implementation

use crate::coordinator::timing::ExecutionTiming;
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
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::{debug, error, info};
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

    /// Execution timing trackers
    execution_timings: Arc<DashMap<Uuid, ExecutionTiming>>,
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
            execution_timings: Arc::new(DashMap::new()),
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
        // Initialize timing tracker
        let timing = ExecutionTiming::new(intent.intent_id);
        self.execution_timings.insert(intent.intent_id, timing);

        info!(
            intent_id = %intent.intent_id,
            sequence = intent.sequence_number,
            pair_id = %intent.pair_id,
            "Coordinator received intent, spawning leg execution tasks"
        );

        // Validate intent
        intent.validate().map_err(|e| {
            DryTestingError::Coordinator(CoordinatorError::CompensationFailed(format!(
                "Intent validation failed: {}",
                e
            )))
        })?;

        // Generate client_order_ids for both legs
        let (client_order_id_a, client_order_id_b) = intent.generate_client_order_ids();

        // Record order creation time
        let order_creation_start = Instant::now();

        debug!(
            intent_id = %intent.intent_id,
            leg_a_venue = ?intent.leg_a.venue,
            leg_b_venue = ?intent.leg_b.venue,
            client_order_id_a = %client_order_id_a,
            client_order_id_b = %client_order_id_b,
            "Spawning parallel leg execution"
        );

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

        // Record order creation time
        if let Some(mut timing) = self.execution_timings.get_mut(&intent.intent_id) {
            timing.order_creation_time = Some(order_creation_start.elapsed());
        }

        // Fire-and-forget order writes (Phase 3: 0-latency critical path)
        // Spawn background tasks to write orders to database
        // Execution continues immediately without blocking on DB writes
        let db_writer_a = self.db_writer.clone();
        let state_machine_a = self.state_machine.clone();
        let order_a_clone = order_a.clone();
        let client_order_id_a_clone = client_order_id_a.clone();
        let intent_id_clone = intent.intent_id;
        let execution_timings_a = self.execution_timings.clone();
        
        tokio::spawn(async move {
            let write_start = Instant::now();
            match db_writer_a.write_order(&order_a_clone).await {
                Ok(db_order_id) => {
                    let write_time = write_start.elapsed();
                    
                    // Record DB write time
                    if let Some(mut timing) = execution_timings_a.get_mut(&intent_id_clone) {
                        timing.db_write_time_a = Some(write_time);
                    }
                    
                    debug!(
                        intent_id = %intent_id_clone,
                        client_order_id = %client_order_id_a_clone,
                        local_order_id = %order_a_clone.id,
                        db_order_id = %db_order_id,
                        leg = "A",
                        db_write_time_ms = write_time.as_millis(),
                        "Order A written to database (background)"
                    );
                    
                    // Update OrderState.order_id in cache (Phase 4)
                    if let Err(e) = state_machine_a
                        .update_order_id(&client_order_id_a_clone, db_order_id)
                        .await
                    {
                        error!(
                            intent_id = %intent_id_clone,
                            client_order_id = %client_order_id_a_clone,
                            error = %e,
                            "Failed to update OrderState.order_id for order A"
                        );
                    }
                }
                Err(e) => {
                    error!(
                        intent_id = %intent_id_clone,
                        client_order_id = %client_order_id_a_clone,
                        order_id = %order_a_clone.id,
                        leg = "A",
                        error = %e,
                        "Failed to write order A to database (will retry in background)"
                    );
                    // Note: Execution continues regardless of write failure (fire-and-forget)
                }
            }
        });
        
        let db_writer_b = self.db_writer.clone();
        let state_machine_b = self.state_machine.clone();
        let order_b_clone = order_b.clone();
        let client_order_id_b_clone = client_order_id_b.clone();
        let intent_id_clone_b = intent.intent_id;
        let execution_timings_b = self.execution_timings.clone();
        
        tokio::spawn(async move {
            let write_start = Instant::now();
            match db_writer_b.write_order(&order_b_clone).await {
                Ok(db_order_id) => {
                    let write_time = write_start.elapsed();
                    
                    // Record DB write time
                    if let Some(mut timing) = execution_timings_b.get_mut(&intent_id_clone_b) {
                        timing.db_write_time_b = Some(write_time);
                    }
                    
                    debug!(
                        intent_id = %intent_id_clone_b,
                        client_order_id = %client_order_id_b_clone,
                        local_order_id = %order_b_clone.id,
                        db_order_id = %db_order_id,
                        leg = "B",
                        db_write_time_ms = write_time.as_millis(),
                        "Order B written to database (background)"
                    );
                    
                    // Update OrderState.order_id in cache (Phase 4)
                    if let Err(e) = state_machine_b
                        .update_order_id(&client_order_id_b_clone, db_order_id)
                        .await
                    {
                        error!(
                            intent_id = %intent_id_clone_b,
                            client_order_id = %client_order_id_b_clone,
                            error = %e,
                            "Failed to update OrderState.order_id for order B"
                        );
                    }
                }
                Err(e) => {
                    error!(
                        intent_id = %intent_id_clone_b,
                        client_order_id = %client_order_id_b_clone,
                        order_id = %order_b_clone.id,
                        leg = "B",
                        error = %e,
                        "Failed to write order B to database (will retry in background)"
                    );
                    // Note: Execution continues regardless of write failure (fire-and-forget)
                }
            }
        });

        // Use local UUIDs for OrderState initially (will be updated when DB writes complete)
        // Note: order_a.id and order_b.id are local UUIDs, not DB IDs yet

        // Register orders with router (using local UUIDs initially)
        self.order_router.register_order(&order_a);
        self.order_router.register_order(&order_b);

        // Create OrderState for state machine cache (using local UUIDs, will be updated when DB writes complete)
        let order_state_a = crate::types::order::OrderState {
            order_id: order_a.id, // Local UUID, will be updated to DB ID when write completes
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
            order_id: order_b.id, // Local UUID, will be updated to DB ID when write completes
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

        // Create execution tracking (using local UUIDs, will be updated when DB writes complete)
        self.create_execution(&intent, order_a.id, order_b.id)
            .await?;

        // Get venue executors
        let executor_a = match self.get_venue_executor(&intent.leg_a.venue) {
            Ok(exec) => exec,
            Err(e) => {
                error!(
                    intent_id = %intent.intent_id,
                    venue = ?intent.leg_a.venue,
                    error = %e,
                    "Failed to get venue executor for leg A"
                );
                return Err(e);
            }
        };
        
        let executor_b = match self.get_venue_executor(&intent.leg_b.venue) {
            Ok(exec) => exec,
            Err(e) => {
                error!(
                    intent_id = %intent.intent_id,
                    venue = ?intent.leg_b.venue,
                    error = %e,
                    "Failed to get venue executor for leg B"
                );
                return Err(e);
            }
        };

        // Clone necessary data for spawned tasks
        let order_a_clone = order_a.clone();
        let order_b_clone = order_b.clone();
        let intent_id = intent.intent_id;

        // Spawn both legs as independent tasks
        info!(
            intent_id = %intent_id,
            order_a_id = %order_a.id,
            order_b_id = %order_b.id,
            "Spawning parallel leg execution tasks"
        );

        let intent_id_a = intent_id;
        let intent_id_b = intent_id;
        let mut handle_a = tokio::spawn(async move {
            debug!(
                intent_id = %intent_id_a,
                order_id = %order_a_clone.id,
                leg = "A",
                venue = ?order_a_clone.venue,
                "Starting leg A execution task"
            );
            executor_a
                .execute_leg(order_a_clone, OrderLeg::A, intent_id_a)
                .await
        });

        let mut handle_b = tokio::spawn(async move {
            debug!(
                intent_id = %intent_id_b,
                order_id = %order_b_clone.id,
                leg = "B",
                venue = ?order_b_clone.venue,
                "Starting leg B execution task"
            );
            executor_b
                .execute_leg(order_b_clone, OrderLeg::B, intent_id_b)
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
            intent_id = %intent_id,
            final_status = ?final_status,
            "Execution completed"
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

        debug!(
            intent_id = %intent_id,
            leg = ?leg,
            new_status = ?new_status,
            leg_a_state = ?execution.leg_a_state,
            leg_b_state = ?execution.leg_b_state,
            "Updating execution leg state"
        );

        // Update leg state
        match leg {
            OrderLeg::A => execution.leg_a_state = new_status,
            OrderLeg::B => execution.leg_b_state = new_status,
        }

        // Evaluate overall status
        let new_status = self.evaluate_execution_status(&execution);
        execution.status = new_status;

        info!(
            intent_id = %intent_id,
            leg = ?leg,
            leg_status = ?new_status,
            overall_status = ?execution.status,
            "Execution status updated"
        );

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
        info!(
            intent_id = %intent_id,
            completed_leg = ?completed_leg,
            completed_order_id = %completed_order_id,
            "Leg completed, waiting for other leg"
        );

        match result {
            Ok(Ok(LegExecutionResult::Filled)) | Ok(Ok(LegExecutionResult::Acknowledged)) => {
                debug!(
                    intent_id = %intent_id,
                    completed_leg = ?completed_leg,
                    result = ?result,
                    "First leg succeeded, waiting for other leg"
                );

                // This leg succeeded - wait for other leg
                match other_handle.await {
                    Ok(Ok(LegExecutionResult::Filled))
                    | Ok(Ok(LegExecutionResult::Acknowledged)) => {
                        // Both succeeded - COMPLETED
                        let completion_start = Instant::now();
                        info!(
                            intent_id = %intent_id,
                            "Both legs succeeded - execution COMPLETED"
                        );
                        let other_leg = match completed_leg {
                            OrderLeg::A => OrderLeg::B,
                            OrderLeg::B => OrderLeg::A,
                        };
                        self.update_execution(intent_id, other_leg, OrderStatus::Filled)
                            .await?;
                        
                        // Record completion time and print summary
                        let completion_time = completion_start.elapsed();
                        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
                            timing.coordinator_completion_time = Some(completion_time);
                            timing.end_time = Some(Instant::now());
                            
                            // Print comprehensive timing summary
                            println!("\n{}", timing.format_summary());
                        }
                        
                        Ok(ArbExecutionStatus::Completed)
                    }
                    Ok(Ok(LegExecutionResult::Rejected)) | Ok(Err(_)) | Err(_) => {
                        // Other leg failed - rollback this leg (handle already consumed by await)
                        info!(
                            intent_id = %intent_id,
                            completed_leg = ?completed_leg,
                            other_order_id = %other_order_id,
                            "Other leg failed - rolling back completed leg"
                        );
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

    /// Record submission time for a leg (called by executor)
    pub(crate) fn record_submission_time(
        &self,
        intent_id: Uuid,
        leg: OrderLeg,
        duration: Duration,
    ) {
        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
            match leg {
                OrderLeg::A => timing.submission_time_a = Some(duration),
                OrderLeg::B => timing.submission_time_b = Some(duration),
            }
        }
    }

    /// Record process time for a leg (called by executor)
    pub(crate) fn record_process_time(
        &self,
        intent_id: Uuid,
        leg: OrderLeg,
        duration: Duration,
    ) {
        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
            match leg {
                OrderLeg::A => timing.process_time_a = Some(duration),
                OrderLeg::B => timing.process_time_b = Some(duration),
            }
        }
    }

    /// Record state transition time for a leg (called by state machine)
    pub(crate) fn record_state_transition_time(
        &self,
        intent_id: Uuid,
        leg: OrderLeg,
        duration: Duration,
    ) {
        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
            match leg {
                OrderLeg::A => timing.state_transition_time_a = Some(duration),
                OrderLeg::B => timing.state_transition_time_b = Some(duration),
            }
        }
    }

    /// Record fill latency for a leg (called when fill is received)
    pub(crate) fn record_fill_latency(
        &self,
        intent_id: Uuid,
        leg: OrderLeg,
        duration: Duration,
    ) {
        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
            match leg {
                OrderLeg::A => timing.fill_latency_a = Some(duration),
                OrderLeg::B => timing.fill_latency_b = Some(duration),
            }
        }
    }

    /// Record enqueue time (called by router)
    pub(crate) fn record_enqueue_time(&self, intent_id: Uuid, duration: Duration) {
        if let Some(mut timing) = self.execution_timings.get_mut(&intent_id) {
            timing.enqueue_time = Some(duration);
        }
    }
}
