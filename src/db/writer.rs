//! Database writer with batching and retry mechanism

use crate::types::errors::{DryTestingError, Result};
use crate::types::order::StateTransition;
use sqlx::{PgPool, QueryBuilder};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration constants for retry mechanism
const MAX_RETRIES: u32 = 10;
const RETRY_BACKOFF_BASE_MS: u64 = 100;
const RETRY_INTERVAL_MS: u64 = 100;
const ORDER_LOOKUP_TIMEOUT_MS: u64 = 50;
const RETRY_WAIT_POLL_INTERVAL_MS: u64 = 100;

/// Failed transition with client_order_id for retry
#[derive(Debug, Clone)]
struct FailedTransition {
    client_order_id: String,
    transition: StateTransition,
    retry_count: u32,
    next_retry: Instant,
}

/// Database writer with batched writes and retry mechanism
pub struct DatabaseWriter {
    /// Database connection pool
    db: Arc<PgPool>,

    /// Batching configuration
    transition_batch_size: usize,
    fill_batch_size: usize,
    batch_timeout: Duration,

    /// Pending batches
    pending_transitions: Arc<Mutex<Vec<(uuid::Uuid, StateTransition)>>>,
    pending_fills: Arc<Mutex<Vec<FillRecord>>>,

    /// Failed transitions queue for retry (Phase 7)
    failed_transitions: Arc<Mutex<Vec<FailedTransition>>>,

    /// Dead letter queue for permanently failed writes
    dead_letter_queue: Arc<Mutex<Vec<FailedTransition>>>,

    /// Shutdown signal sender (for graceful shutdown)
    shutdown_tx: watch::Sender<bool>,

    /// Background task handles (for cleanup)
    flush_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    retry_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

/// Fill record for batching
#[derive(Debug, Clone)]
pub(crate) struct FillRecord {
    venue: String,
    trade_id: String,
    order_id: uuid::Uuid,
    venue_order_id: String,
    fill_size_int: i64,
    fill_price_int: i64,
    price_scale: i16,
    size_scale: i16,
    venue_ts: Option<chrono::DateTime<chrono::Utc>>,
}

impl DatabaseWriter {
    /// Create a new database writer
    pub async fn new(db: Arc<PgPool>) -> Result<Self> {
        let (shutdown_tx, _) = watch::channel(false);
        
        Ok(Self {
            db,
            transition_batch_size: 100,
            fill_batch_size: 10,
            batch_timeout: Duration::from_millis(100),
            pending_transitions: Arc::new(Mutex::new(Vec::new())),
            pending_fills: Arc::new(Mutex::new(Vec::new())),
            failed_transitions: Arc::new(Mutex::new(Vec::new())),
            dead_letter_queue: Arc::new(Mutex::new(Vec::new())),
            shutdown_tx,
            flush_handle: Arc::new(Mutex::new(None)),
            retry_handle: Arc::new(Mutex::new(None)),
        })
    }

    /// Write order immediately (need DB ID for tracking)
    pub async fn write_order(&self, order: &crate::types::order::Order) -> Result<uuid::Uuid> {
        // Insert order with ON CONFLICT DO NOTHING for idempotency
        // Database schema uses customer_id (not user_id)
        let order_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO orders (
                customer_id, strategy_id, pair_id, leg, venue, venue_market_id, venue_outcome_id,
                client_order_id, venue_order_id, side, limit_price_int, price_scale,
                size_int, size_scale, status, filled_size_int,
                created_at, updated_at
            )
            VALUES ($1, $2, $3, $4::order_leg, $5::venue_type, $6, $7, $8, $9, $10::order_side, $11, $12, $13, $14, $15::order_status, $16, $17, $18)
            ON CONFLICT (customer_id, venue, client_order_id) DO UPDATE
            SET updated_at = EXCLUDED.updated_at
            RETURNING id
            "#,
        )
        .bind(order.customer_id)
        .bind(order.strategy_id)
        .bind(order.pair_id)
        .bind({
            // Database enum is uppercase ('A', 'B') - keep as-is from OrderLeg
            <crate::types::order::OrderLeg as Into<&str>>::into(order.leg)
        })
        .bind(<crate::types::venue::VenueType as Into<String>>::into(
            order.venue.clone(),
        ))
        .bind(&order.venue_market_id)
        .bind(&order.venue_outcome_id)
        .bind(&order.client_order_id)
        .bind(&order.venue_order_id)
        .bind(<crate::types::order::OrderSide as Into<&str>>::into(
            order.side,
        ))
        .bind(order.limit_price_int)
        .bind(order.price_scale)
        .bind(order.size_int)
        .bind(order.size_scale)
        .bind(<crate::types::order::OrderStatus as Into<&str>>::into(
            order.status,
        ))
        .bind(order.filled_size_int)
        .bind(order.created_at)
        .bind(order.updated_at)
        .fetch_one(&*self.db)
        .await
        .map_err(crate::types::errors::DryTestingError::Database)?;

        Ok(order_id)
    }

    /// Queue transition for batching
    pub async fn queue_transition(
        &self,
        order_id: uuid::Uuid,
        transition: StateTransition,
    ) -> Result<()> {
        let mut pending = self.pending_transitions.lock().await;
        pending.push((order_id, transition));

        // Flush if batch size reached
        if pending.len() >= self.transition_batch_size {
            drop(pending);
            self.flush_transitions().await?;
        }

        Ok(())
    }

    /// Queue fill for batching
    pub async fn queue_fill(&self, fill: FillRecord) -> Result<()> {
        let mut pending = self.pending_fills.lock().await;
        pending.push(fill);

        // Flush if batch size reached
        if pending.len() >= self.fill_batch_size {
            drop(pending);
            self.flush_fills().await?;
        }

        Ok(())
    }

    /// Flush transitions batch (Phase 6: with error handling and retry queue)
    async fn flush_transitions(&self) -> Result<()> {
        // Check if pool is closed before proceeding
        if self.db.is_closed() {
            debug!("Database pool is closed, skipping flush");
            return Ok(());
        }

        let mut pending = self.pending_transitions.lock().await;
        if pending.is_empty() {
            return Ok(());
        }

        let batch = pending.drain(..).collect::<Vec<_>>();
        drop(pending);

        // Attempt batch insert
        match self.batch_insert_events(&batch).await {
            Ok(_) => {
                info!("Flushed {} transitions to database", batch.len());
                Ok(())
            }
            Err(e) => {
                // Classify error: foreign key violation vs other
                let is_fk_violation = if let DryTestingError::Database(db_err) = &e {
                    self.is_foreign_key_violation(db_err)
                } else {
                    false
                };
                
                if is_fk_violation {
                    warn!(
                        "Batch insert failed with foreign key violation (wrong order_id), queueing for retry: {:?}",
                        e
                    );
                } else {
                    error!("Batch insert failed with other error, queueing for retry: {:?}", e);
                }
                
                // Queue failed transitions for retry
                self.queue_failed_transitions(batch, e).await?;
                Ok(()) // Don't fail, retry will handle it
            }
        }
    }

    /// Batch insert events to order_events table using true batch SQL
    ///
    /// Optimized to use multi-row INSERT instead of sequential queries.
    /// Splits batch by event_id presence (with/without) for proper idempotency handling.
    async fn batch_insert_events(
        &self,
        batch: &[(Uuid, StateTransition)],
    ) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // Split batch by event_id presence (needed for different SQL patterns)
        let mut with_event_id: Vec<(Uuid, &StateTransition)> = Vec::new();
        let mut without_event_id: Vec<(Uuid, &StateTransition)> = Vec::new();

        for (order_id, transition) in batch {
            if transition.event_id.is_some() {
                with_event_id.push((*order_id, transition));
            } else {
                without_event_id.push((*order_id, transition));
            }
        }

        // Batch insert events with event_id (idempotent)
        if !with_event_id.is_empty() {
            if let Err(e) = self.batch_insert_events_with_id(&with_event_id).await {
                // If batch insert fails, fall back to queueing for retry
                return Err(e);
            }
        }

        // Batch insert events without event_id (legacy)
        if !without_event_id.is_empty() {
            if let Err(e) = self.batch_insert_events_without_id(&without_event_id).await {
                // If batch insert fails, fall back to queueing for retry
                return Err(e);
            }
        }

        Ok(())
    }

    /// Batch insert events that have event_id (with ON CONFLICT for idempotency)
    async fn batch_insert_events_with_id(
        &self,
        batch: &[(Uuid, &StateTransition)],
    ) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // Build multi-row INSERT with VALUES clause
        // PostgreSQL supports up to 65535 parameters, so we're safe with batch size 100
        let mut query_builder = QueryBuilder::new(
            r#"
            INSERT INTO order_events (order_id, event_type, payload_json, venue_ts, created_at, event_id)
            VALUES
            "#
        );

        // Build each row manually, using separated only for values within rows
        for (idx, (order_id, transition)) in batch.iter().enumerate() {
            if idx > 0 {
                query_builder.push(", ");
            }

            let payload = serde_json::json!({
                "from": transition.from.to_string(),
                "to": transition.to.to_string(),
                "source": transition.source,
            });

            query_builder.push("(");
            let mut value_separated = query_builder.separated(", ");
            value_separated.push_bind(*order_id);
            value_separated.push_bind(transition.to.to_string());
            value_separated.push_bind(payload);
            value_separated.push_bind(transition.timestamp);
            value_separated.push_bind(transition.timestamp);
            value_separated.push_bind(transition.event_id.unwrap());
            query_builder.push(")");
        }

        query_builder.push(" ON CONFLICT (event_id) DO NOTHING");

        let result = query_builder
            .build()
            .execute(&*self.db)
            .await
            .map_err(crate::types::errors::DryTestingError::Database)?;

        // Check if any rows were inserted (conflicts are OK due to idempotency)
        if result.rows_affected() == 0 && !batch.is_empty() {
            debug!("All events in batch already exist (idempotent), skipping insert");
        } else {
            debug!(
                "Batch inserted {} events ({} rows affected)",
                batch.len(),
                result.rows_affected()
            );
        }

        Ok(())
    }

    /// Batch insert events without event_id (legacy events)
    async fn batch_insert_events_without_id(
        &self,
        batch: &[(Uuid, &StateTransition)],
    ) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // Build multi-row INSERT with VALUES clause
        let mut query_builder = QueryBuilder::new(
            r#"
            INSERT INTO order_events (order_id, event_type, payload_json, venue_ts, created_at, event_id)
            VALUES
            "#
        );

        // Build each row manually, using separated only for values within rows
        for (idx, (order_id, transition)) in batch.iter().enumerate() {
            if idx > 0 {
                query_builder.push(", ");
            }

            let payload = serde_json::json!({
                "from": transition.from.to_string(),
                "to": transition.to.to_string(),
                "source": transition.source,
            });

            query_builder.push("(");
            let mut value_separated = query_builder.separated(", ");
            value_separated.push_bind(*order_id);
            value_separated.push_bind(transition.to.to_string());
            value_separated.push_bind(payload);
            value_separated.push_bind(transition.timestamp);
            value_separated.push_bind(transition.timestamp);
            value_separated.push_bind(Option::<Uuid>::None);
            query_builder.push(")");
        }

        let result = query_builder
            .build()
            .execute(&*self.db)
            .await
            .map_err(crate::types::errors::DryTestingError::Database)?;

        debug!(
            "Batch inserted {} legacy events ({} rows affected)",
            batch.len(),
            result.rows_affected()
        );

        Ok(())
    }

    /// Check if error is a foreign key violation
    fn is_foreign_key_violation(&self, error: &sqlx::Error) -> bool {
        let error_msg = error.to_string().to_lowercase();
        error_msg.contains("foreign key") 
            || error_msg.contains("violates foreign key constraint")
            || error_msg.contains("referenced")
    }

    /// Queue failed transitions for retry
    async fn queue_failed_transitions(
        &self,
        batch: Vec<(Uuid, StateTransition)>,
        _error: DryTestingError,
    ) -> Result<()> {
        let mut failed = self.failed_transitions.lock().await;
        let now = Instant::now();

        for (_order_id, transition) in batch {
            let failed_transition = FailedTransition {
                client_order_id: transition.client_order_id.clone(),
                transition,
                retry_count: 0,
                next_retry: now + Duration::from_millis(RETRY_BACKOFF_BASE_MS),
            };
            failed.push(failed_transition);
        }

        Ok(())
    }

    /// Flush fills batch using true batch SQL insert
    async fn flush_fills(&self) -> Result<()> {
        // Check if pool is closed before proceeding
        if self.db.is_closed() {
            debug!("Database pool is closed, skipping fill flush");
            return Ok(());
        }

        let mut pending = self.pending_fills.lock().await;
        if pending.is_empty() {
            return Ok(());
        }

        let batch = pending.drain(..).collect::<Vec<_>>();
        drop(pending);

        if batch.is_empty() {
            return Ok(());
        }

        // Build multi-row INSERT with VALUES clause
        let mut query_builder = QueryBuilder::new(
            r#"
            INSERT INTO order_fills (
                venue, trade_id, order_id, venue_order_id,
                fill_size_int, fill_price_int, price_scale, size_scale, venue_ts
            )
            VALUES
            "#
        );

        // Build each row manually, using separated only for values within rows
        for (idx, fill) in batch.iter().enumerate() {
            if idx > 0 {
                query_builder.push(", ");
            }

            query_builder.push("(");
            let mut value_separated = query_builder.separated(", ");
            value_separated.push_bind(&fill.venue);
            value_separated.push_bind(&fill.trade_id);
            value_separated.push_bind(fill.order_id);
            value_separated.push_bind(&fill.venue_order_id);
            value_separated.push_bind(fill.fill_size_int);
            value_separated.push_bind(fill.fill_price_int);
            value_separated.push_bind(fill.price_scale);
            value_separated.push_bind(fill.size_scale);
            value_separated.push_bind(fill.venue_ts);
            query_builder.push(")");
        }

        query_builder.push(" ON CONFLICT (venue, trade_id) DO NOTHING");

        match query_builder
            .build()
            .execute(&*self.db)
            .await
        {
            Ok(result) => {
                info!(
                    "Batch inserted {} fills ({} rows affected)",
                    batch.len(),
                    result.rows_affected()
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    error = %e,
                    count = batch.len(),
                    "Batch fill insert failed"
                );
                // Don't fail - fills are not critical for execution
                // They can be retried or logged separately if needed
                Ok(())
            }
        }
    }

    /// Start background flush task
    pub async fn start_background_flush(&self) {
        let writer = self.clone_for_background();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle_guard = self.flush_handle.clone();
        
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(writer.batch_timeout);
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = writer.flush_transitions().await;
                        let _ = writer.flush_fills().await;
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            debug!("Background flush task received shutdown signal");
                            break;
                        }
                    }
                }
            }
            
            // Final flush before shutdown
            let _ = writer.flush_transitions().await;
            let _ = writer.flush_fills().await;
            debug!("Background flush task stopped");
        });
        
        // Store handle
        *handle_guard.lock().await = Some(handle);
    }

    /// Start retry scheduler task (Phase 7)
    pub async fn start_retry_scheduler(&self) {
        let writer = self.clone_for_background();
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let handle_guard = self.retry_handle.clone();
        
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(RETRY_INTERVAL_MS));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let _ = writer.retry_failed_transitions().await;
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            debug!("Retry scheduler task received shutdown signal");
                            break;
                        }
                    }
                }
            }
            
            // Final retry attempt before shutdown
            let _ = writer.retry_failed_transitions().await;
            debug!("Retry scheduler task stopped");
        });
        
        // Store handle
        *handle_guard.lock().await = Some(handle);
    }

    /// Retry failed transitions (Phase 7) with optimized batch lookups
    async fn retry_failed_transitions(&self) -> Result<()> {
        // Check if pool is closed before proceeding
        if self.db.is_closed() {
            debug!("Database pool is closed, skipping retry");
            return Ok(());
        }

        let mut failed = self.failed_transitions.lock().await;
        if failed.is_empty() {
            return Ok(());
        }

        let now = Instant::now();
        
        // Filter transitions ready for retry (collect indices in reverse order for safe removal)
        let mut ready_transitions: Vec<FailedTransition> = Vec::new();
        let mut ready_indices: Vec<usize> = Vec::new();
        for (idx, ft) in failed.iter().enumerate() {
            if ft.next_retry <= now {
                ready_indices.push(idx);
            }
        }

        // Remove ready transitions from queue (in reverse order to maintain indices)
        for &idx in ready_indices.iter().rev() {
            ready_transitions.push(failed.remove(idx));
        }
        drop(failed); // Release lock before database operations

        if ready_transitions.is_empty() {
            return Ok(());
        }

        // Batch lookup: collect all client_order_ids and query in one go
        let client_order_ids: Vec<String> = ready_transitions
            .iter()
            .map(|ft| ft.client_order_id.clone())
            .collect();

        // Perform batch lookup
        let order_id_map = match self.batch_lookup_order_ids(&client_order_ids).await {
            Ok(map) => map,
            Err(e) => {
                // Batch lookup failed: fall back to individual lookups or retry later
                warn!(
                    error = %e,
                    count = ready_transitions.len(),
                    "Batch lookup failed, will retry individual lookups later"
                );
                // Put all transitions back in queue for retry
                let mut failed = self.failed_transitions.lock().await;
                for ft in ready_transitions {
                    failed.push(ft);
                }
                return Ok(());
            }
        };

        // Process retries with mapped order_ids
        let mut failed = self.failed_transitions.lock().await;
        for ft in ready_transitions {
            match order_id_map.get(&ft.client_order_id) {
                Some(&order_id) => {
                    // Found order_id: retry insert
                    match self.insert_single_event(order_id, &ft.transition).await {
                        Ok(_) => {
                            // Success: transition persisted
                            debug!(
                                client_order_id = %ft.client_order_id,
                                retry_count = ft.retry_count,
                                "Successfully retried transition"
                            );
                        }
                        Err(e) => {
                            // Still failed: update retry count and next_retry
                            let mut new_ft = ft;
                            new_ft.retry_count += 1;
                            let retry_count = new_ft.retry_count;
                            let client_order_id = new_ft.client_order_id.clone();
                            
                            if retry_count >= MAX_RETRIES {
                                // Move to dead letter queue
                                warn!(
                                    client_order_id = %client_order_id,
                                    retry_count = retry_count,
                                    "Max retries exceeded, moving to dead letter queue"
                                );
                                self.move_to_dead_letter(new_ft).await?;
                            } else {
                                // Calculate exponential backoff
                                let backoff_ms = if retry_count <= 10 {
                                    RETRY_BACKOFF_BASE_MS * (1 << (retry_count - 1).min(8))
                                } else {
                                    30_000 // 30 seconds for retries > 10
                                };
                                new_ft.next_retry = Instant::now() + Duration::from_millis(backoff_ms);
                                failed.push(new_ft);
                                
                                warn!(
                                    client_order_id = %client_order_id,
                                    retry_count = retry_count,
                                    error = %e,
                                    "Retry failed, will retry again"
                                );
                            }
                        }
                    }
                }
                None => {
                    // Order not found: may have been deleted, move to dead letter
                    warn!(
                        client_order_id = %ft.client_order_id,
                        "Order not found for client_order_id, moving to dead letter queue"
                    );
                    self.move_to_dead_letter(ft).await?;
                }
            }
        }

        Ok(())
    }

    /// Batch lookup order_ids from client_order_ids (optimized)
    ///
    /// Returns a HashMap mapping client_order_id -> order_id for efficient lookup.
    async fn batch_lookup_order_ids(
        &self,
        client_order_ids: &[String],
    ) -> Result<HashMap<String, Uuid>> {
        if client_order_ids.is_empty() {
            return Ok(HashMap::new());
        }

        // Check pool state
        if self.db.is_closed() {
            return Err(DryTestingError::Config("Pool is closed".to_string()));
        }

        // Use ANY(array) for efficient batch lookup
        let query = sqlx::query_as::<_, (Uuid, String)>(
            "SELECT id, client_order_id FROM orders WHERE client_order_id = ANY($1::text[])"
        )
        .bind(&client_order_ids[..]);

        // Apply timeout
        let results = match tokio::time::timeout(
            Duration::from_millis(ORDER_LOOKUP_TIMEOUT_MS * client_order_ids.len().max(1) as u64),
            query.fetch_all(&*self.db),
        )
        .await
        {
            Ok(Ok(rows)) => rows,
            Ok(Err(e)) => {
                warn!(
                    count = client_order_ids.len(),
                    error = %e,
                    "Batch order lookup failed"
                );
                return Err(crate::types::errors::DryTestingError::Database(e));
            }
            Err(_) => {
                warn!(
                    count = client_order_ids.len(),
                    "Batch order lookup timed out"
                );
                return Err(crate::types::errors::DryTestingError::Database(
                    sqlx::Error::PoolClosed,
                ));
            }
        };

        // Build HashMap for efficient lookup
        let mut order_id_map = HashMap::with_capacity(results.len());
        for (order_id, client_order_id) in results {
            order_id_map.insert(client_order_id, order_id);
        }

        Ok(order_id_map)
    }

    /// Look up order_id from client_order_id
    async fn lookup_order_id(
        &self,
        client_order_id: &str,
    ) -> Result<Option<Uuid>> {
        // Check pool state
        if self.db.is_closed() {
            return Err(DryTestingError::Config("Pool is closed".to_string()));
        }

        let query = sqlx::query_scalar::<_, Uuid>(
            "SELECT id FROM orders WHERE client_order_id = $1 LIMIT 1"
        )
        .bind(client_order_id);

        // Apply timeout
        match tokio::time::timeout(
            Duration::from_millis(ORDER_LOOKUP_TIMEOUT_MS),
            query.fetch_optional(&*self.db),
        )
        .await
        {
            Ok(Ok(Some(order_id))) => Ok(Some(order_id)),
            Ok(Ok(None)) => Ok(None),
            Ok(Err(e)) => Err(crate::types::errors::DryTestingError::Database(e)),
            Err(_) => {
                warn!(
                    client_order_id = %client_order_id,
                    "Order lookup timed out"
                );
                Err(crate::types::errors::DryTestingError::Database(
                    sqlx::Error::PoolClosed,
                ))
            }
        }
    }

    /// Insert single event (for retry)
    async fn insert_single_event(
        &self,
        order_id: Uuid,
        transition: &StateTransition,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "from": transition.from.to_string(),
            "to": transition.to.to_string(),
            "source": transition.source,
        });

        // Use conditional INSERT: only insert if event_id is not NULL and not already exists
        if let Some(event_id) = transition.event_id {
            // Insert with ON CONFLICT for idempotency
            let result = sqlx::query(
                r#"
                INSERT INTO order_events (order_id, event_type, payload_json, venue_ts, created_at, event_id)
                VALUES ($1, $2, $3, $4, $5, $6)
                ON CONFLICT (event_id) DO NOTHING
                "#,
            )
            .bind(order_id)
            .bind(transition.to.to_string())
            .bind(payload)
            .bind(transition.timestamp)
            .bind(transition.timestamp)
            .bind(event_id)
            .execute(&*self.db)
            .await
            .map_err(crate::types::errors::DryTestingError::Database)?;

            // If no rows were inserted (conflict), that's OK (idempotent)
            if result.rows_affected() == 0 {
                debug!("Event already exists (idempotent), skipping insert");
            }
        } else {
            // No event_id: insert without idempotency check (legacy events)
            sqlx::query(
                r#"
                INSERT INTO order_events (order_id, event_type, payload_json, venue_ts, created_at, event_id)
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(order_id)
            .bind(transition.to.to_string())
            .bind(payload)
            .bind(transition.timestamp)
            .bind(transition.timestamp)
            .bind(transition.event_id)
            .execute(&*self.db)
            .await
            .map_err(crate::types::errors::DryTestingError::Database)?;
        }

        Ok(())
    }

    /// Move failed transition to dead letter queue
    async fn move_to_dead_letter(&self, ft: FailedTransition) -> Result<()> {
        let mut dlq = self.dead_letter_queue.lock().await;
        error!(
            client_order_id = %ft.client_order_id,
            retry_count = ft.retry_count,
            "Moving transition to dead letter queue (permanent failure)"
        );
        dlq.push(ft);
        Ok(())
    }

    /// Wait for all pending retries to complete (for testing/shutdown)
    ///
    /// This method polls the failed_transitions queue until it's empty
    /// or a timeout is reached. Useful for ensuring test data integrity
    /// before shutting down the database pool.
    ///
    /// # Arguments
    /// * `timeout` - Maximum time to wait for retries to complete
    ///
    /// # Returns
    /// * `Ok(())` - All retries completed successfully
    /// * `Err(DryTestingError)` - Timeout reached or pool closed
    ///
    /// # Notes
    /// - This method is safe to call in production (non-blocking for hot path)
    /// - New failures may be added during wait, but we wait for current queue to drain
    /// - Respects shutdown signal (will return early if shutdown is signaled)
    /// - Proper lock scoping ensures retry scheduler can continue processing
    pub async fn wait_for_retries(&self, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        let shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            // Check shutdown signal (informational - shutdown may already be signaled)
            if *shutdown_rx.borrow() {
                debug!("Shutdown signaled, waiting for retries to complete");
            }

            // Check if pool is closed
            if self.db.is_closed() {
                let pending_count = {
                    let failed = self.failed_transitions.lock().await;
                    failed.len()
                };
                return Err(DryTestingError::Config(format!(
                    "Pool is closed with {} pending retries",
                    pending_count
                )));
            }

            // Check if retries are complete (lock scoped to minimal operations)
            let is_empty = {
                let failed = self.failed_transitions.lock().await;
                failed.is_empty()
            };

            if is_empty {
                debug!("All retries completed");
                return Ok(());
            }

            // Check timeout
            if start.elapsed() >= timeout {
                let pending_count = {
                    let failed = self.failed_transitions.lock().await;
                    failed.len()
                };
                warn!(
                    pending_retries = pending_count,
                    timeout_secs = timeout.as_secs(),
                    "Timeout waiting for retries to complete"
                );
                return Err(DryTestingError::Config(format!(
                    "Timeout waiting for {} retries to complete (timeout: {}s)",
                    pending_count,
                    timeout.as_secs()
                )));
            }

            // Release lock before sleep (critical for performance)
            tokio::time::sleep(Duration::from_millis(RETRY_WAIT_POLL_INTERVAL_MS)).await;
        }
    }

    /// Gracefully shutdown background tasks
    ///
    /// Shutdown sequence:
    /// 1. Signal shutdown to all tasks (stops new work)
    /// 2. Wait for flush task to stop (producer stops, no new failures)
    /// 3. Final flush of any pending writes
    /// 4. Wait for retries to complete (consumers finish)
    /// 5. Wait for retry scheduler task to stop
    ///
    /// This ensures proper cleanup: producers stop first, then consumers finish.
    ///
    /// # Returns
    /// * `Ok(())` - Shutdown completed successfully
    /// * `Err(DryTestingError)` - Shutdown failed
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down DatabaseWriter background tasks...");

        // Step 1: Signal shutdown to ALL tasks (stops new work)
        // This prevents flush task from adding new failures to the queue
        self.shutdown_tx.send(true)
            .map_err(|e| DryTestingError::Config(format!("Failed to send shutdown signal: {}", e)))?;

        // Step 2: Wait for flush task to stop (producer stops)
        // This ensures no new failures will be added to the queue
        let shutdown_timeout = Duration::from_secs(2);

        let flush_handle = {
            let mut guard = self.flush_handle.lock().await;
            guard.take()
        };

        if let Some(mut handle) = flush_handle {
            tokio::select! {
                result = &mut handle => {
                    match result {
                        Ok(_) => debug!("Flush task stopped gracefully"),
                        Err(e) => warn!("Flush task error during shutdown: {}", e),
                    }
                }
                _ = tokio::time::sleep(shutdown_timeout) => {
                    warn!("Flush task shutdown timeout, aborting");
                    handle.abort();
                }
            }
        }

        // Step 3: Perform final flush of any pending writes
        // This ensures any in-flight batches are written before waiting for retries
        self.flush_transitions().await?;
        self.flush_fills().await?;

        // Step 4: Wait for retries to complete (consumers finish)
        // NOW safe to wait - no new failures will be added (flush task stopped)
        const RETRY_WAIT_TIMEOUT: Duration = Duration::from_secs(5);
        if let Err(e) = self.wait_for_retries(RETRY_WAIT_TIMEOUT).await {
            warn!(
                error = %e,
                "Some retries may not have completed before shutdown"
            );
            // Continue with shutdown anyway (timeout protection prevents indefinite blocking)
        }

        // Step 5: Wait for retry scheduler task to stop
        let retry_handle = {
            let mut guard = self.retry_handle.lock().await;
            guard.take()
        };

        if let Some(mut handle) = retry_handle {
            tokio::select! {
                result = &mut handle => {
                    match result {
                        Ok(_) => debug!("Retry task stopped gracefully"),
                        Err(e) => warn!("Retry task error during shutdown: {}", e),
                    }
                }
                _ = tokio::time::sleep(shutdown_timeout) => {
                    warn!("Retry task shutdown timeout, aborting");
                    handle.abort();
                }
            }
        }

        info!("DatabaseWriter shutdown complete");
        Ok(())
    }

    /// Clone for background task (only clones Arc references)
    fn clone_for_background(&self) -> Self {
        Self {
            db: self.db.clone(),
            transition_batch_size: self.transition_batch_size,
            fill_batch_size: self.fill_batch_size,
            batch_timeout: self.batch_timeout,
            pending_transitions: self.pending_transitions.clone(),
            pending_fills: self.pending_fills.clone(),
            failed_transitions: self.failed_transitions.clone(),
            dead_letter_queue: self.dead_letter_queue.clone(),
            shutdown_tx: self.shutdown_tx.clone(),
            flush_handle: Arc::new(Mutex::new(None)),
            retry_handle: Arc::new(Mutex::new(None)),
        }
    }
}
