//! Database queries for intent generation
//!
//! Provides non-blocking database access with timeout handling
//! for querying verified pairs and markets from Supabase.

use crate::intent_generator::errors::IntentGeneratorError;
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use std::time::Duration;
use tokio::time::timeout;
use tracing::debug;
use uuid::Uuid;

/// Default query timeout (5 seconds)
const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Verified pair row from database
#[derive(Debug, Clone, FromRow)]
pub struct VerifiedPairRow {
    pub id: Uuid,
    pub pair_key: String,
    pub market_a_id: Uuid,
    pub market_b_id: Uuid,
    pub outcome_mapping: Value, // JSONB
    pub verdict: String,        // pair_verdict enum
    pub is_current: bool,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Market row from database
#[derive(Debug, Clone, FromRow)]
pub struct MarketRow {
    pub id: Uuid,
    pub venue: String, // venue_type enum as text
    pub venue_market_id: String,
    pub title: String,
    pub canonical_text: String,
    pub category: String,
    pub league: String,
    pub is_active: bool,
}

/// Strategy row (for strategy_id reference)
#[derive(Debug, Clone, FromRow)]
pub struct StrategyRow {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub name: String,
    pub is_active: bool,
}

/// Scope row (for strategy creation)
#[derive(Debug, Clone, FromRow)]
pub struct ScopeRow {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub name: String,
}

/// Database reader with timeout handling
pub struct DatabaseReader {
    pool: PgPool,
    query_timeout: Duration,
}

impl DatabaseReader {
    /// Get the database pool (for cloning in parallel processing)
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Get the query timeout
    pub fn query_timeout(&self) -> Duration {
        self.query_timeout
    }

    /// Create a new database reader
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            query_timeout: DEFAULT_QUERY_TIMEOUT,
        }
    }

    /// Create with custom timeout
    ///
    /// # Arguments
    /// * `pool` - PostgreSQL connection pool
    /// * `timeout` - Maximum time to wait for queries
    pub fn with_timeout(pool: PgPool, timeout: Duration) -> Self {
        Self {
            pool,
            query_timeout: timeout,
        }
    }

    /// Get verified pair by ID
    ///
    /// # Arguments
    /// * `pair_id` - Verified pair UUID
    ///
    /// # Returns
    /// Verified pair row if found and active
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::PairNotFound` if pair doesn't exist
    /// Returns `IntentGeneratorError::PairNotActive` if pair is not active
    /// Returns `IntentGeneratorError::QueryTimeout` if query times out
    pub async fn get_verified_pair(
        &self,
        pair_id: Uuid,
    ) -> Result<VerifiedPairRow, IntentGeneratorError> {
        debug!("Fetching verified pair: {}", pair_id);

        let query = sqlx::query_as::<_, VerifiedPairRow>(
            r#"
            SELECT 
                id, pair_key, market_a_id, market_b_id,
                outcome_mapping, verdict, is_current, is_active,
                created_at, updated_at
            FROM verified_pairs
            WHERE id = $1
            "#,
        )
        .bind(pair_id);

        let pair = timeout(self.query_timeout, query.fetch_one(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| {
                if matches!(e, sqlx::Error::RowNotFound) {
                    IntentGeneratorError::PairNotFound(pair_id)
                } else {
                    IntentGeneratorError::InvalidConfig(format!("Database error: {}", e))
                }
            })?;

        if !pair.is_current {
            return Err(IntentGeneratorError::PairNotActive(pair_id));
        }

        if !pair.is_active {
            return Err(IntentGeneratorError::PairNotActive(pair_id));
        }

        debug!(
            "Found verified pair: {} (markets: {}, {})",
            pair_id, pair.market_a_id, pair.market_b_id
        );
        Ok(pair)
    }

    /// List active verified pairs (for batch testing)
    ///
    /// # Arguments
    /// * `limit` - Maximum number of pairs to return
    ///
    /// # Returns
    /// Vector of verified pair rows
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::QueryTimeout` if query times out
    pub async fn list_active_pairs(
        &self,
        limit: i64,
    ) -> Result<Vec<VerifiedPairRow>, IntentGeneratorError> {
        debug!("Fetching {} active verified pairs", limit);

        let query = sqlx::query_as::<_, VerifiedPairRow>(
            r#"
            SELECT 
                id, pair_key, market_a_id, market_b_id,
                outcome_mapping, verdict, is_current, is_active,
                created_at, updated_at
            FROM verified_pairs
            WHERE is_current = true AND is_active = true
            ORDER BY updated_at DESC
            LIMIT $1
            "#,
        )
        .bind(limit);

        let pairs = timeout(self.query_timeout, query.fetch_all(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| IntentGeneratorError::InvalidConfig(format!("Database error: {}", e)))?;

        debug!("Found {} active verified pairs", pairs.len());
        Ok(pairs)
    }

    /// Get markets by IDs (batch query)
    ///
    /// # Arguments
    /// * `market_ids` - Vector of market UUIDs
    ///
    /// # Returns
    /// Vector of market rows
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::QueryTimeout` if query times out
    pub async fn get_markets(
        &self,
        market_ids: &[Uuid],
    ) -> Result<Vec<MarketRow>, IntentGeneratorError> {
        if market_ids.is_empty() {
            return Ok(Vec::new());
        }

        debug!("Fetching {} markets", market_ids.len());

        let query = sqlx::query_as::<_, MarketRow>(
            r#"
            SELECT 
                id, venue::TEXT as venue, venue_market_id, title,
                canonical_text, category, league, is_active
            FROM markets
            WHERE id = ANY($1)
            "#,
        )
        .bind(market_ids);

        let markets = timeout(self.query_timeout, query.fetch_all(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| IntentGeneratorError::InvalidConfig(format!("Database error: {}", e)))?;

        debug!("Found {} markets", markets.len());
        Ok(markets)
    }

    /// Get or create a test strategy for customer
    ///
    /// # Arguments
    /// * `customer_id` - Customer UUID
    /// * `scope_id` - Scope UUID (required for strategy creation)
    ///
    /// # Returns
    /// Strategy row (existing or newly created)
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::ScopeNotFound` if scope doesn't exist
    /// Returns `IntentGeneratorError::QueryTimeout` if query times out
    pub async fn get_or_create_test_strategy(
        &self,
        customer_id: Uuid,
        scope_id: Uuid,
    ) -> Result<StrategyRow, IntentGeneratorError> {
        debug!(
            "Getting or creating test strategy for customer: {}",
            customer_id
        );

        // First, verify scope exists and belongs to customer
        let scope_exists = {
            let query = sqlx::query_scalar::<_, bool>(
                r#"
                SELECT EXISTS(
                    SELECT 1 FROM scopes 
                    WHERE id = $1 AND customer_id = $2
                )
                "#,
            )
            .bind(scope_id)
            .bind(customer_id);

            timeout(self.query_timeout, query.fetch_one(&self.pool))
                .await
                .map_err(|_| IntentGeneratorError::QueryTimeout)?
                .map_err(|e| {
                    IntentGeneratorError::InvalidConfig(format!("Database error: {}", e))
                })?
        };

        if !scope_exists {
            return Err(IntentGeneratorError::ScopeNotFound(customer_id));
        }

        // Try to get existing test strategy
        let query = sqlx::query_as::<_, StrategyRow>(
            r#"
            SELECT id, customer_id, name, is_active
            FROM strategies
            WHERE customer_id = $1 AND name = 'test_strategy'
            LIMIT 1
            "#,
        )
        .bind(customer_id);

        let strategy = timeout(self.query_timeout, query.fetch_optional(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| IntentGeneratorError::InvalidConfig(format!("Database error: {}", e)))?;

        if let Some(s) = strategy {
            debug!("Found existing test strategy: {}", s.id);
            return Ok(s);
        }

        // Create test strategy
        debug!("Creating new test strategy for customer: {}", customer_id);
        let query = sqlx::query_as::<_, StrategyRow>(
            r#"
            INSERT INTO strategies (
                customer_id, scope_id, name,
                min_edge_bps, min_notional_int, min_notional_scale,
                max_notional_int, max_notional_scale, staleness_ms, is_active
            )
            VALUES (
                $1, $2, 'test_strategy',
                0,  -- No minimum edge for testing
                0, 6,  -- No minimum notional
                1000000000, 6,  -- Max notional: 1000 USD (1,000,000,000 micros)
                60000,  -- 60 second staleness
                true
            )
            ON CONFLICT (customer_id, name) DO UPDATE
            SET updated_at = NOW()
            RETURNING id, customer_id, name, is_active
            "#,
        )
        .bind(customer_id)
        .bind(scope_id);

        let strategy = timeout(self.query_timeout, query.fetch_one(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| IntentGeneratorError::InvalidConfig(format!("Database error: {}", e)))?;

        debug!("Created test strategy: {}", strategy.id);
        Ok(strategy)
    }

    /// Get first scope for customer (helper for test strategy creation)
    ///
    /// # Arguments
    /// * `customer_id` - Customer UUID
    ///
    /// # Returns
    /// First scope row if found
    ///
    /// # Errors
    /// Returns `IntentGeneratorError::ScopeNotFound` if no scope exists
    pub async fn get_first_scope(
        &self,
        customer_id: Uuid,
    ) -> Result<ScopeRow, IntentGeneratorError> {
        let query = sqlx::query_as::<_, ScopeRow>(
            r#"
            SELECT id, customer_id, name
            FROM scopes
            WHERE customer_id = $1
            ORDER BY created_at ASC
            LIMIT 1
            "#,
        )
        .bind(customer_id);

        let scope = timeout(self.query_timeout, query.fetch_optional(&self.pool))
            .await
            .map_err(|_| IntentGeneratorError::QueryTimeout)?
            .map_err(|e| IntentGeneratorError::InvalidConfig(format!("Database error: {}", e)))?
            .ok_or_else(|| IntentGeneratorError::ScopeNotFound(customer_id))?;

        Ok(scope)
    }
}
