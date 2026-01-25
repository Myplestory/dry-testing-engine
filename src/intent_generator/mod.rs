//! Intent generator for creating ArbExecutionIntent from database
//!
//! This module provides functionality to query verified pairs and markets
//! from Supabase (PostgreSQL) and construct execution intents for testing
//! the dry testing engine.
//!
//! # Architecture
//!
//! - **DatabaseReader**: Non-blocking database queries with timeout handling
//! - **TestConfig**: Configurable test parameters with validation
//! - **IntentGenerator**: Main generator that orchestrates intent creation
//!
//! # Usage
//!
//! ```rust,no_run
//! use dry_testing_engine::intent_generator::{IntentGenerator, TestConfig};
//!
//! let config = TestConfig::default();
//! let generator = IntentGenerator::new(pool, config)?;
//!
//! let intent = generator.generate_from_pair_id(pair_id, customer_id, None).await?;
//! engine.router().enqueue_intent(intent).await?;
//! ```

pub mod config;
pub mod database;
pub mod errors;
pub mod generator;

pub use config::TestConfig;
pub use database::DatabaseReader;
pub use errors::IntentGeneratorError;
pub use generator::IntentGenerator;
