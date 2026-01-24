//! Execution lane router (simplified - sequence number assignment only)

use crate::coordinator::ArbExecutionCoordinator;
use crate::core::SequenceGenerator;
use crate::types::errors::Result;
use crate::types::execution::ArbExecutionIntent;
use std::sync::Arc;

/// Routes execution intents to coordinator (sequence number assignment)
pub struct ExecutionLaneRouter {
    /// Sequence number generator
    sequence_generator: Arc<SequenceGenerator>,

    /// Coordinator for multi-leg execution
    coordinator: Arc<ArbExecutionCoordinator>,
}

impl ExecutionLaneRouter {
    /// Create a new execution lane router
    pub async fn new(coordinator: Arc<ArbExecutionCoordinator>) -> Result<Self> {
        Ok(Self {
            sequence_generator: Arc::new(SequenceGenerator::new()),
            coordinator,
        })
    }

    /// Enqueue an execution intent
    ///
    /// Assigns a sequence number and delegates to the coordinator for execution.
    ///
    /// # Arguments
    /// * `intent` - Arbitrage execution intent to enqueue
    ///
    /// # Returns
    /// * `Ok(())` - Intent was successfully enqueued
    /// * `Err(DryTestingError)` - Enqueue failed
    pub async fn enqueue_intent(&self, mut intent: ArbExecutionIntent) -> Result<()> {
        // Assign sequence number
        let seq = self.sequence_generator.next();
        intent.sequence_number = seq;
        intent.enqueued_at = Some(chrono::Utc::now());

        // Delegate to coordinator (which spawns tasks directly)
        self.coordinator.enqueue_intent(intent).await?;

        Ok(())
    }
}
