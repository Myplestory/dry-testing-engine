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
//! use dry_testing_engine::intent_generator::{IntentGenerator, TestConfig, DatabaseReader};
//! use dry_testing_engine::DryTestingEngine;
//! use sqlx::PgPool;
//! use uuid::Uuid;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create database pool
//!     let pool = PgPool::connect("postgresql://user:password@localhost:5432/test").await?;
//!     
//!     // Query database for active verified pairs
//!     let db_reader = DatabaseReader::new(pool.clone());
//!     let active_pairs = db_reader.list_active_pairs(1).await?;
//!     let pair_id = active_pairs.first()
//!         .ok_or("No active verified pairs found")?
//!         .id;
//!     
//!     // Customer ID can be simulated for testing
//!     let customer_id = Uuid::new_v4();
//!     
//!     // Create engine
//!     let engine = DryTestingEngine::new().await?;
//!     
//!     // Create generator and generate intent
//!     let config = TestConfig::default();
//!     let generator = IntentGenerator::new(pool, config)?;
//!     let intent = generator.generate_from_pair_id(pair_id, customer_id, None).await?;
//!     
//!     // Enqueue intent
//!     engine.router().enqueue_intent(intent).await?;
//!     
//!     Ok(())
//! }
//! ```

pub mod config;
pub mod database;
pub mod errors;
pub mod generator;

pub use config::TestConfig;
pub use database::DatabaseReader;
pub use errors::IntentGeneratorError;
pub use generator::IntentGenerator;
