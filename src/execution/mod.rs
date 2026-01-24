//! Execution routing and direct task spawning

pub mod engine;
pub mod executor;
pub mod router;

pub use engine::DryTestingEngine;
pub use executor::{LegExecutionResult, VenueExecutor};
pub use router::ExecutionLaneRouter;
