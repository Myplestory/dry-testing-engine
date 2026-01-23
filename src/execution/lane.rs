//! Execution lane implementation

use crate::execution::metrics::LaneMetrics;
use crate::types::execution::{ArbExecutionIntent, ExecutionPriority};
use crate::types::errors::LaneError;
use crate::types::order::OrderState;
use crate::types::venue::VenueType;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Execution lane for a specific venue
pub struct ExecutionLane {
    /// Lane ID
    lane_id: usize,
    
    /// Venue this lane handles
    venue: VenueType,
    
    /// Bounded queue for execution intents
    sender: mpsc::Sender<ArbExecutionIntent>,
    receiver: mpsc::Receiver<ArbExecutionIntent>,
    
    /// Maximum queue capacity
    max_capacity: usize,
    
    /// Active orders in this lane
    active_orders: Arc<DashMap<Uuid, OrderState>>,
    
    /// Lane metrics
    metrics: Arc<dashmap::DashMap<String, u64>>,
    
    /// Current queue size (approximate)
    queue_size: Arc<std::sync::atomic::AtomicUsize>,
}

impl ExecutionLane {
    /// Create a new execution lane
    pub fn new(venue: VenueType, capacity: usize) -> Self {
        let (sender, receiver) = mpsc::channel(capacity);
        let metrics = Arc::new(dashmap::DashMap::new());
        
        // Initialize metrics
        metrics.insert("enqueued".to_string(), 0);
        metrics.insert("dequeued".to_string(), 0);
        metrics.insert("dropped_opportunities".to_string(), 0);
        metrics.insert("dropped_risk_critical".to_string(), 0);
        
        Self {
            lane_id: 0, // Will be set by router
            venue,
            sender,
            receiver,
            max_capacity: capacity,
            active_orders: Arc::new(DashMap::new()),
            metrics,
            queue_size: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }
    
    /// Enqueue an execution intent
    pub fn enqueue(&self, intent: ArbExecutionIntent) -> std::result::Result<(), LaneError> {
        // Check backpressure (approximate queue size)
        let threshold = (self.max_capacity * 8) / 10; // 80% threshold
        
        // Try to send (non-blocking)
        match self.sender.try_send(intent.clone()) {
            Ok(_) => {
                self.queue_size.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if let Some(mut count) = self.metrics.get_mut("enqueued") {
                    *count += 1;
                }
                Ok(())
            }
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                // Queue is full - check priority
                match intent.priority {
                    ExecutionPriority::Opportunity => {
                        // Drop OPPORTUNITY priority intents
                        if let Some(mut count) = self.metrics.get_mut("dropped_opportunities") {
                            *count += 1;
                        }
                        Err(LaneError::QueueFull)
                    }
                    ExecutionPriority::RiskCritical => {
                        // RISK_CRITICAL: Shouldn't happen, but log if it does
                        if let Some(mut count) = self.metrics.get_mut("dropped_risk_critical") {
                            *count += 1;
                        }
                        tracing::error!("Queue full, dropping RISK_CRITICAL intent!");
                        Err(LaneError::QueueFull)
                    }
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                Err(LaneError::QueueFull)
            }
        }
    }
    
    /// Get lane metrics
    pub fn metrics(&self) -> LaneMetrics {
        LaneMetrics {
            enqueued: self.metrics.get("enqueued").map(|v| *v.value()).unwrap_or(0),
            dequeued: self.metrics.get("dequeued").map(|v| *v.value()).unwrap_or(0),
            dropped_opportunities: self.metrics.get("dropped_opportunities").map(|v| *v.value()).unwrap_or(0),
            dropped_risk_critical: self.metrics.get("dropped_risk_critical").map(|v| *v.value()).unwrap_or(0),
            queue_size: self.queue_size.load(std::sync::atomic::Ordering::Relaxed),
        }
    }
    
    /// Get receiver for processing intents (consumes self)
    pub fn into_receiver(self) -> tokio::sync::mpsc::Receiver<ArbExecutionIntent> {
        self.receiver
    }
    
    /// Get sender for enqueueing (cloned)
    pub fn sender(&self) -> mpsc::Sender<ArbExecutionIntent> {
        self.sender.clone()
    }
    
    /// Get venue type
    pub fn venue(&self) -> &VenueType {
        &self.venue
    }
}

