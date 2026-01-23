//! Error types for the dry testing engine

use thiserror::Error;

/// Result type alias for the dry testing engine
pub type Result<T> = std::result::Result<T, DryTestingError>;

/// Main error type for the dry testing engine
#[derive(Error, Debug)]
pub enum DryTestingError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Lane error: {0}")]
    Lane(#[from] LaneError),

    #[error("State machine error: {0}")]
    StateMachine(#[from] StateMachineError),

    #[error("Coordinator error: {0}")]
    Coordinator(#[from] CoordinatorError),

    #[error("Venue error: {0}")]
    Venue(#[from] VenueError),

    #[error("Routing error: {0}")]
    Routing(#[from] RoutingError),

    #[error("Recovery error: {0}")]
    Recovery(String),

    #[error("Invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("Order not found: {0}")]
    OrderNotFound(uuid::Uuid),

    #[error("Execution not found: {0}")]
    ExecutionNotFound(uuid::Uuid),

    #[error("Queue full")]
    QueueFull,

    #[error("Unknown venue: {0}")]
    UnknownVenue(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Lane-specific errors
#[derive(Error, Debug)]
pub enum LaneError {
    #[error("Queue full")]
    QueueFull,

    #[error("Lane not initialized")]
    LaneNotInitialized,

    #[error("Unknown venue: {0}")]
    UnknownVenue(String),
}

/// State machine errors
#[derive(Error, Debug)]
pub enum StateMachineError {
    #[error("Invalid state transition: {0}")]
    InvalidTransition(String),

    #[error("Order not found: {0}")]
    OrderNotFound(uuid::Uuid),

    #[error("State already terminal")]
    StateAlreadyTerminal,
}

/// Coordinator errors
#[derive(Error, Debug)]
pub enum CoordinatorError {
    #[error("Execution not found: {0}")]
    ExecutionNotFound(uuid::Uuid),

    #[error("Compensation failed: {0}")]
    CompensationFailed(String),

    #[error("Timeout: {0}")]
    Timeout(String),
}

/// Venue adapter errors
#[derive(Error, Debug)]
pub enum VenueError {
    #[error("Order submission failed: {0}")]
    SubmissionFailed(String),

    #[error("Order cancellation failed: {0}")]
    CancellationFailed(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),
}

/// Routing errors
#[derive(Error, Debug)]
pub enum RoutingError {
    #[error("Order not found: {0}")]
    OrderNotFound(String),

    #[error("Venue order ID not found: {0}")]
    VenueOrderIdNotFound(String),

    #[error("Client order ID not found: {0}")]
    ClientOrderIdNotFound(String),
}

