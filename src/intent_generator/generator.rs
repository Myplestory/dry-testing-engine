//! Intent generator for creating ArbExecutionIntent from database data
//!
//! This module provides functionality to query verified pairs and markets
//! from Supabase and construct execution intents for testing the dry
//! testing engine.
//!
//! # Example
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

use crate::intent_generator::config::TestConfig;
use crate::intent_generator::database::DatabaseReader;
use crate::intent_generator::errors::IntentGeneratorError;
use crate::types::execution::{ArbExecutionIntent, ExecutionPriority, LegSpec};
use crate::types::order::OrderSide;
use crate::types::venue::VenueType;
use std::collections::HashMap;
use std::str::FromStr;
use tracing::{debug, error, info};
use uuid::Uuid;

/// Maximum batch size for parallel processing
const MAX_BATCH_SIZE: usize = 100;

/// Intent generator for creating execution intents from database
pub struct IntentGenerator {
    db_reader: DatabaseReader,
    config: TestConfig,
}

impl IntentGenerator {
    /// Create a new intent generator
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    /// * `config` - Test configuration
    ///
    /// # Returns
    /// New intent generator instance
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::InvalidConfig` if config validation fails
    pub fn new(pool: sqlx::PgPool, config: TestConfig) -> Result<Self, IntentGeneratorError> {
        config.validate()?;

        Ok(Self {
            db_reader: DatabaseReader::new(pool),
            config,
        })
    }

    /// Create with custom query timeout
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    /// * `config` - Test configuration
    /// * `timeout` - Custom query timeout
    ///
    /// # Returns
    /// New intent generator instance with custom timeout
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::InvalidConfig` if config validation fails
    pub fn with_timeout(
        pool: sqlx::PgPool,
        config: TestConfig,
        timeout: std::time::Duration,
    ) -> Result<Self, IntentGeneratorError> {
        config.validate()?;

        Ok(Self {
            db_reader: DatabaseReader::with_timeout(pool, timeout),
            config,
        })
    }

    /// Generate intent from verified pair ID
    ///
    /// # Arguments
    /// * `pair_id` - Verified pair UUID
    /// * `customer_id` - Customer UUID (from strategy evaluator)
    /// * `strategy_id` - Strategy UUID (from strategy evaluator)
    ///
    /// # Returns
    /// Generated execution intent
    ///
    /// # Errors
    /// Returns various `IntentGeneratorError` variants for different failure modes
    pub async fn generate_from_pair_id(
        &self,
        pair_id: Uuid,
        customer_id: Uuid,
        strategy_id: Uuid,
    ) -> Result<ArbExecutionIntent, IntentGeneratorError> {
        info!(
            "Generating intent for pair: {}, customer: {}, strategy: {}",
            pair_id, customer_id, strategy_id
        );

        // Get verified pair
        let pair = self.db_reader.get_verified_pair(pair_id).await?;

        // Get markets in parallel (non-blocking)
        let market_ids = vec![pair.market_a_id, pair.market_b_id];
        let markets = self.db_reader.get_markets(&market_ids).await?;

        if markets.len() != 2 {
            return Err(IntentGeneratorError::InvalidConfig(format!(
                "Expected 2 markets, got {}",
                markets.len()
            )));
        }

        // Find market A and B
        let market_a = markets
            .iter()
            .find(|m| m.id == pair.market_a_id)
            .ok_or_else(|| IntentGeneratorError::MarketANotFound(pair.id))?;

        let market_b = markets
            .iter()
            .find(|m| m.id == pair.market_b_id)
            .ok_or_else(|| IntentGeneratorError::MarketBNotFound(pair.id))?;

        // Parse outcome mapping with explicit error handling
        let outcome_mapping = self.parse_outcome_mapping(&pair.outcome_mapping)?;

        // Extract outcome IDs from contract specs
        let (leg_a_outcome, leg_b_outcome) =
            self.extract_outcome_ids(&pair, &outcome_mapping).await?;

        // Build intent
        let intent = ArbExecutionIntent {
            intent_id: Uuid::new_v4(),
            sequence_number: 0, // Will be set by router
            customer_id,
            strategy_id,
            pair_id: pair.id,
            leg_a: LegSpec {
                venue: VenueType::from_str(&market_a.venue).map_err(|e| {
                    IntentGeneratorError::InvalidVenue(format!(
                        "Market A venue '{}': {}",
                        market_a.venue, e
                    ))
                })?,
                venue_market_id: market_a.venue_market_id.clone(),
                venue_outcome_id: leg_a_outcome,
                side: OrderSide::Buy,
                price_int: self.config.leg_a_price_int,
                price_scale: self.config.leg_a_price_scale,
                size_int: self.config.size_int,
                size_scale: self.config.size_scale,
            },
            leg_b: LegSpec {
                venue: VenueType::from_str(&market_b.venue).map_err(|e| {
                    IntentGeneratorError::InvalidVenue(format!(
                        "Market B venue '{}': {}",
                        market_b.venue, e
                    ))
                })?,
                venue_market_id: market_b.venue_market_id.clone(),
                venue_outcome_id: leg_b_outcome,
                side: OrderSide::Buy,
                price_int: self.config.leg_b_price_int,
                price_scale: self.config.leg_b_price_scale,
                size_int: self.config.size_int,
                size_scale: self.config.size_scale,
            },
            expected_profit_int: self.config.expected_profit_int,
            expected_profit_scale: self.config.expected_profit_scale,
            expected_roi_bps: self.config.expected_roi_bps,
            outcome_mapping,
            priority: ExecutionPriority::Opportunity,
            detected_at: chrono::Utc::now(),
            enqueued_at: None,
        };

        info!(
            "Generated intent: {} for pair: {}",
            intent.intent_id, pair_id
        );
        Ok(intent)
    }

    /// Generate intents from multiple pairs (parallel batch processing)
    ///
    /// # Arguments
    /// * `pair_ids` - Vector of verified pair UUIDs
    /// * `customer_id` - Customer UUID
    /// * `scope_id` - Optional scope ID
    ///
    /// # Returns
    /// Tuple of (successful intents, errors) - processes all pairs even if some fail
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::BatchSizeExceeded` if batch is too large
    pub async fn generate_batch(
        &self,
        pair_ids: &[Uuid],
        customer_id: Uuid,
        strategy_id: Uuid,
    ) -> Result<(Vec<ArbExecutionIntent>, Vec<(Uuid, IntentGeneratorError)>), IntentGeneratorError>
    {
        if pair_ids.len() > MAX_BATCH_SIZE {
            return Err(IntentGeneratorError::BatchSizeExceeded(
                pair_ids.len(),
                MAX_BATCH_SIZE,
            ));
        }

        info!(
            "Generating batch of {} intents for customer: {}, strategy: {}",
            pair_ids.len(),
            customer_id,
            strategy_id
        );

        // Process all pairs in parallel (non-blocking)
        // Clone pool for parallel execution (PgPool is Arc internally, so cheap to clone)
        let pool = self.db_reader.pool().clone();
        let config = self.config.clone();
        let query_timeout = self.db_reader.query_timeout();

        // Spawn tasks for true parallelism
        let handles: Vec<_> = pair_ids
            .iter()
            .map(|&pair_id| {
                let pool = pool.clone();
                let config = config.clone();
                let customer_id_clone = customer_id;
                let strategy_id_clone = strategy_id;

                tokio::spawn(async move {
                    // Create temporary generator for this task
                    let temp_generator = IntentGenerator {
                        db_reader: DatabaseReader::with_timeout(pool, query_timeout),
                        config,
                    };

                    temp_generator
                        .generate_from_pair_id(pair_id, customer_id_clone, strategy_id_clone)
                        .await
                        .map_err(|e| (pair_id, e))
                })
            })
            .collect();

        // Wait for all tasks to complete
        let results = futures::future::join_all(handles).await;

        // Separate successes and errors
        let mut intents = Vec::new();
        let mut errors = Vec::new();

        for result in results {
            match result {
                Ok(Ok(intent)) => intents.push(intent),
                Ok(Err((pair_id, err))) => {
                    error!("Failed to generate intent for pair {}: {:?}", pair_id, err);
                    errors.push((pair_id, err));
                }
                Err(join_err) => {
                    error!("Task panicked: {:?}", join_err);
                    // Can't recover pair_id from panicked task
                }
            }
        }

        info!(
            "Successfully generated {} intents, {} errors",
            intents.len(),
            errors.len()
        );
        Ok((intents, errors))
    }

    /// Parse outcome_mapping JSONB to HashMap
    ///
    /// # Arguments
    /// * `value` - JSONB value from database
    ///
    /// # Returns
    /// HashMap of outcome mappings
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::InvalidOutcomeMapping` if parsing fails
    fn parse_outcome_mapping(
        &self,
        value: &serde_json::Value,
    ) -> Result<HashMap<String, String>, IntentGeneratorError> {
        let mut mapping = HashMap::new();

        if !value.is_object() {
            return Err(IntentGeneratorError::InvalidOutcomeMapping(
                "outcome_mapping must be a JSON object".to_string(),
            ));
        }

        if let Some(obj) = value.as_object() {
            for (k, v) in obj {
                let value_str = v.as_str().ok_or_else(|| {
                    IntentGeneratorError::InvalidOutcomeMapping(format!(
                        "Outcome mapping value for key '{}' must be a string",
                        k
                    ))
                })?;
                mapping.insert(k.clone(), value_str.to_string());
            }
        }

        if mapping.is_empty() {
            return Err(IntentGeneratorError::InvalidOutcomeMapping(
                "outcome_mapping cannot be empty".to_string(),
            ));
        }

        Ok(mapping)
    }

    /// Extract outcome IDs from contract specs
    ///
    /// Fetches contract specs and extracts outcome names from spec_json.outcomes arrays.
    /// Uses outcome_mapping to determine which outcome to use (YES = index 0, NO = index 1).
    /// Defaults to "YES" outcome for testing.
    ///
    /// # Arguments
    /// * `pair` - Verified pair row (contains contract_spec IDs)
    /// * `outcome_mapping` - Outcome mapping for validation
    ///
    /// # Returns
    /// Tuple of (leg_a_outcome_id, leg_b_outcome_id)
    ///
    /// # Errors
    /// Returns `IntentGeneratorError` if contract specs can't be fetched or parsed
    async fn extract_outcome_ids(
        &self,
        pair: &crate::intent_generator::database::VerifiedPairRow,
        outcome_mapping: &HashMap<String, String>,
    ) -> Result<(String, String), IntentGeneratorError> {
        // Fetch contract specs in parallel
        let (spec_a, spec_b) = futures::try_join!(
            self.db_reader.get_contract_spec(pair.contract_spec_a_id),
            self.db_reader.get_contract_spec(pair.contract_spec_b_id)
        )?;

        // Parse spec_json to extract outcomes arrays
        let outcomes_a = self.parse_spec_outcomes(&spec_a.spec_json, "market A")?;
        let outcomes_b = self.parse_spec_outcomes(&spec_b.spec_json, "market B")?;

        // Determine which outcome to use (default to YES for testing)
        // outcome_mapping structure: {"YES_A": "YES_B", "NO_A": "NO_B"}
        // We'll use YES outcome (index 0) for both legs
        let outcome_index = 0; // YES is typically first in ["YES", "NO"]

        // Validate outcome_mapping indicates YES correspondence
        if !outcome_mapping.contains_key("YES_A") {
            return Err(IntentGeneratorError::MissingOutcomeKey(format!(
                "Missing YES_A key in outcome_mapping for pair {}",
                pair.id
            )));
        }

        // Extract outcome names from arrays
        let leg_a_outcome = outcomes_a
            .get(outcome_index)
            .ok_or_else(|| {
                IntentGeneratorError::InvalidOutcomeMapping(format!(
                    "Market A outcomes array doesn't have index {} (array: {:?})",
                    outcome_index, outcomes_a
                ))
            })?
            .clone();

        let leg_b_outcome = outcomes_b
            .get(outcome_index)
            .ok_or_else(|| {
                IntentGeneratorError::InvalidOutcomeMapping(format!(
                    "Market B outcomes array doesn't have index {} (array: {:?})",
                    outcome_index, outcomes_b
                ))
            })?
            .clone();

        Ok((leg_a_outcome, leg_b_outcome))
    }

    /// Parse outcomes array from contract spec JSON
    ///
    /// # Arguments
    /// * `spec_json` - Contract spec JSONB value
    /// * `market_label` - Label for error messages
    ///
    /// # Returns
    /// Vector of outcome names
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::InvalidOutcomeMapping` if parsing fails
    fn parse_spec_outcomes(
        &self,
        spec_json: &serde_json::Value,
        market_label: &str,
    ) -> Result<Vec<String>, IntentGeneratorError> {
        if !spec_json.is_object() {
            return Err(IntentGeneratorError::InvalidOutcomeMapping(format!(
                "Contract spec for {} must be a JSON object",
                market_label
            )));
        }

        let outcomes = spec_json
            .get("outcomes")
            .ok_or_else(|| {
                IntentGeneratorError::InvalidOutcomeMapping(format!(
                    "Missing 'outcomes' field in contract spec for {}",
                    market_label
                ))
            })?
            .as_array()
            .ok_or_else(|| {
                IntentGeneratorError::InvalidOutcomeMapping(format!(
                    "'outcomes' field in contract spec for {} must be an array",
                    market_label
                ))
            })?;

        let mut outcome_names = Vec::new();
        for (idx, outcome) in outcomes.iter().enumerate() {
            let name = outcome.as_str().ok_or_else(|| {
                IntentGeneratorError::InvalidOutcomeMapping(format!(
                    "Outcome at index {} in {} contract spec must be a string",
                    idx, market_label
                ))
            })?;
            outcome_names.push(name.to_string());
        }

        if outcome_names.is_empty() {
            return Err(IntentGeneratorError::InvalidOutcomeMapping(format!(
                "Outcomes array in {} contract spec cannot be empty",
                market_label
            )));
        }

        Ok(outcome_names)
    }
}
