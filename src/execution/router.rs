//! Execution lane router

use crate::coordinator::ArbExecutionCoordinator;
use crate::core::SequenceGenerator;
use crate::types::execution::ArbExecutionIntent;
use crate::types::errors::LaneError;
use crate::types::venue::VenueType;
use crate::execution::lane::ExecutionLane;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Routes execution intents to appropriate venue lanes
pub struct ExecutionLaneRouter {
    /// Fast path: array for known venues
    known_lanes: [Option<Arc<ExecutionLane>>; 5],
    
    /// Fallback: HashMap for dynamic venues
    dynamic_lanes: HashMap<String, Arc<ExecutionLane>>,
    
    /// Sequence number generator
    sequence_generator: Arc<SequenceGenerator>,
    
    /// Coordinator for multi-leg execution
    coordinator: Arc<ArbExecutionCoordinator>,
}

impl ExecutionLaneRouter {
    /// Create a new execution lane router
    pub async fn new(coordinator: Arc<ArbExecutionCoordinator>) -> crate::types::errors::Result<Self> {
        // Note: We need to create lanes and store receivers separately for processing
        // For now, create lanes - receivers will be extracted for processing tasks
        let mut known_lanes: [Option<Arc<ExecutionLane>>; 5] = [
            None, None, None, None, None,
        ];
        
        // Initialize known venue lanes
        known_lanes[0] = Some(Arc::new(ExecutionLane::new(VenueType::Polymarket, 1024)));
        known_lanes[1] = Some(Arc::new(ExecutionLane::new(VenueType::Kalshi, 1024)));
        known_lanes[2] = Some(Arc::new(ExecutionLane::new(VenueType::Manifold, 1024)));
        known_lanes[3] = Some(Arc::new(ExecutionLane::new(VenueType::Opinion, 1024)));
        known_lanes[4] = Some(Arc::new(ExecutionLane::new(VenueType::Gemini, 1024)));
        
        Ok(Self {
            known_lanes,
            dynamic_lanes: HashMap::new(),
            sequence_generator: Arc::new(SequenceGenerator::new()),
            coordinator,
        })
    }
    
    /// Route a leg to its appropriate lane (returns sender for enqueueing)
    pub fn route_leg(&self, venue: &VenueType) -> std::result::Result<mpsc::Sender<ArbExecutionIntent>, LaneError> {
        // Fast path: known venues use array indexing
        if let Some(index) = venue.array_index() {
            if let Some(lane) = self.known_lanes[index].as_ref() {
                return Ok(lane.sender());
            }
            return Err(LaneError::LaneNotInitialized);
        }
        
        // Fallback: dynamic venues use HashMap
        self.dynamic_lanes
            .get(venue.as_str())
            .map(|lane| lane.sender())
            .ok_or_else(|| LaneError::UnknownVenue(venue.as_str().to_string()))
    }
    
    /// Enqueue an execution intent
    pub async fn enqueue_intent(&self, mut intent: ArbExecutionIntent) -> crate::types::errors::Result<()> {
        // Assign sequence number
        let seq = self.sequence_generator.next();
        intent.sequence_number = seq;
        intent.enqueued_at = Some(chrono::Utc::now());
        
        // Route to coordinator (which will create orders and route to lanes)
        self.coordinator.enqueue_intent(intent).await?;
        
        Ok(())
    }
    
    /// Register a dynamic venue lane
    pub fn register_venue(&mut self, venue: VenueType, lane: Arc<ExecutionLane>) {
        if venue.array_index().is_none() {
            self.dynamic_lanes.insert(venue.as_str().to_string(), lane);
        }
    }
}

