//! Intent generator specific errors

use thiserror::Error;
use uuid::Uuid;

/// Intent generator errors
#[derive(Error, Debug)]
pub enum IntentGeneratorError {
    #[error("Pair not found: {0}")]
    PairNotFound(Uuid),

    #[error("Pair is not active: {0}")]
    PairNotActive(Uuid),

    #[error("Market not found: {0}")]
    MarketNotFound(Uuid),

    #[error("Market A not found for pair: {0}")]
    MarketANotFound(Uuid),

    #[error("Market B not found for pair: {0}")]
    MarketBNotFound(Uuid),

    #[error("Invalid outcome mapping: {0}")]
    InvalidOutcomeMapping(String),

    #[error("Missing outcome mapping key: {0}")]
    MissingOutcomeKey(String),

    #[error("Invalid venue type: {0}")]
    InvalidVenue(String),

    #[error("Strategy not found for customer: {0}")]
    StrategyNotFound(Uuid),

    #[error("Scope not found for customer: {0}")]
    ScopeNotFound(Uuid),

    #[error("Database query timeout")]
    QueryTimeout,

    #[error("Invalid test configuration: {0}")]
    InvalidConfig(String),

    #[error("Batch size exceeds maximum: {0} (max: {1})")]
    BatchSizeExceeded(usize, usize),
}
