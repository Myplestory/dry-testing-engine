//! Dry Testing Engine
//!
//! A high-performance dry testing engine for execution lanes and order state machine.
//! Designed for testing and benchmarking arbitrage execution with low-latency guarantees.
//!
//! # Architecture
//!
//! - **Venue Executors**: Direct task spawning for parallel leg execution
//! - **Order State Machine**: Lifecycle management with state transitions
//! - **Arbitrage Coordinator**: Multi-leg execution with coordinated cancellation
//! - **Coordinated Cancellation**: tokio::select! + abort() for instant rollback
//! - **Venue Simulator**: Configurable latency and response simulation

pub mod coordinator;
pub mod core;
pub mod db;
pub mod execution;
pub mod recovery;
pub mod routing;
pub mod state_machine;
pub mod types;
pub mod venue;

pub use execution::engine::DryTestingEngine;
pub use types::errors::{DryTestingError, Result};
