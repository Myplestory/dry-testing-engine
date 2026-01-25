//! Intent generator for creating ArbExecutionIntent from database data
//!
//! This module provides functionality to query verified pairs and markets
//! from Supabase and construct execution intents for testing the dry
//! testing engine.
//!
//! # Example
//!
//! ```rust,no_run
//! use dry_testing_engine::intent_generator::{IntentGenerator, TestConfig};
//!
//! let generator = IntentGenerator::new(pool, TestConfig::default())?;
//! let intent = generator.generate_from_pair_id(pair_id, customer_id, None).await?;
//! engine.router().enqueue_intent(intent).await?;
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
    /// * `customer_id` - Customer UUID
    /// * `scope_id` - Optional scope ID (if None, will try to get first scope)
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
        scope_id: Option<Uuid>,
    ) -> Result<ArbExecutionIntent, IntentGeneratorError> {
        info!(
            "Generating intent for pair: {}, customer: {}",
            pair_id, customer_id
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

        // Get or create test strategy
        let scope_id = match scope_id {
            Some(id) => id,
            None => {
                debug!("No scope_id provided, fetching first scope for customer");
                let scope = self.db_reader.get_first_scope(customer_id).await?;
                scope.id
            }
        };

        let strategy = self
            .db_reader
            .get_or_create_test_strategy(customer_id, scope_id)
            .await?;

        // Parse outcome mapping with explicit error handling
        let outcome_mapping = self.parse_outcome_mapping(&pair.outcome_mapping)?;

        // Extract outcome IDs from mapping (explicit logic)
        let (leg_a_outcome, leg_b_outcome) =
            self.extract_outcome_ids(&outcome_mapping, &pair.id)?;

        // Build intent
        let intent = ArbExecutionIntent {
            intent_id: Uuid::new_v4(),
            sequence_number: 0, // Will be set by router
            customer_id,
            strategy_id: strategy.id,
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
        scope_id: Option<Uuid>,
    ) -> Result<(Vec<ArbExecutionIntent>, Vec<(Uuid, IntentGeneratorError)>), IntentGeneratorError>
    {
        if pair_ids.len() > MAX_BATCH_SIZE {
            return Err(IntentGeneratorError::BatchSizeExceeded(
                pair_ids.len(),
                MAX_BATCH_SIZE,
            ));
        }

        info!(
            "Generating batch of {} intents for customer: {}",
            pair_ids.len(),
            customer_id
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
                let scope_id_clone = scope_id;

                tokio::spawn(async move {
                    // Create temporary generator for this task
                    let temp_generator = IntentGenerator {
                        db_reader: DatabaseReader::with_timeout(pool, query_timeout),
                        config,
                    };

                    temp_generator
                        .generate_from_pair_id(pair_id, customer_id_clone, scope_id_clone)
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

    /// Extract outcome IDs from mapping
    ///
    /// # Arguments
    /// * `mapping` - Outcome mapping HashMap
    /// * `pair_id` - Pair ID for error context
    ///
    /// # Returns
    /// Tuple of (leg_a_outcome_id, leg_b_outcome_id)
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::MissingOutcomeKey` if required keys are missing
    fn extract_outcome_ids(
        &self,
        mapping: &HashMap<String, String>,
        pair_id: &Uuid,
    ) -> Result<(String, String), IntentGeneratorError> {
        // Try explicit keys first (case-insensitive)
        let leg_a_outcome = mapping
            .get("YES_A")
            .or_else(|| mapping.get("yes_a"))
            .or_else(|| mapping.get("Yes_A"))
            .cloned();

        let leg_b_outcome = mapping
            .get("YES_B")
            .or_else(|| mapping.get("yes_b"))
            .or_else(|| mapping.get("Yes_B"))
            .cloned();

        match (leg_a_outcome, leg_b_outcome) {
            (Some(a), Some(b)) => Ok((a, b)),
            (None, _) => Err(IntentGeneratorError::MissingOutcomeKey(format!(
                "Missing outcome mapping key for leg A in pair {}",
                pair_id
            ))),
            (_, None) => Err(IntentGeneratorError::MissingOutcomeKey(format!(
                "Missing outcome mapping key for leg B in pair {}",
                pair_id
            ))),
        }
    }
}
