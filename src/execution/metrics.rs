//! Execution lane metrics

use serde::{Deserialize, Serialize};

/// Metrics for an execution lane
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneMetrics {
    /// Number of intents enqueued
    pub enqueued: u64,
    
    /// Number of intents dequeued
    pub dequeued: u64,
    
    /// Number of OPPORTUNITY priority intents dropped
    pub dropped_opportunities: u64,
    
    /// Number of RISK_CRITICAL priority intents dropped (should be 0)
    pub dropped_risk_critical: u64,
    
    /// Current queue size
    pub queue_size: usize,
}

