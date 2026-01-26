//! Database writer with batching and retry mechanism

use crate::types::errors::{DryTestingError, Result};
use crate::types::order::StateTransition;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Configuration constants for retry mechanism
const MAX_RETRIES: u32 = 10;
const RETRY_BACKOFF_BASE_MS: u64 = 100;
const RETRY_INTERVAL_MS: u64 = 100;
const ORDER_LOOKUP_TIMEOUT_MS: u64 = 50;

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
        Ok(Self {
            db,
            transition_batch_size: 100,
            fill_batch_size: 10,
            batch_timeout: Duration::from_millis(100),
            pending_transitions: Arc::new(Mutex::new(Vec::new())),
            pending_fills: Arc::new(Mutex::new(Vec::new())),
            failed_transitions: Arc::new(Mutex::new(Vec::new())),
            dead_letter_queue: Arc::new(Mutex::new(Vec::new())),
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

    /// Batch insert events to order_events table
    async fn batch_insert_events(
        &self,
        batch: &[(Uuid, StateTransition)],
    ) -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }

        // For now, insert one by one (can optimize to batch later if needed)
        // This ensures proper error handling per event
        for (order_id, transition) in batch {
            if let Err(e) = self.insert_single_event(*order_id, transition).await {
                // If one fails, return error (will be handled by flush_transitions)
                return Err(e);
            }
        }

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

    /// Flush fills batch
    async fn flush_fills(&self) -> Result<()> {
        let mut pending = self.pending_fills.lock().await;
        if pending.is_empty() {
            return Ok(());
        }

        let batch = pending.drain(..).collect::<Vec<_>>();
        drop(pending);

        // TODO: Batch insert to order_fills table with ON CONFLICT DO NOTHING
        info!("Flushing {} fills to database", batch.len());

        Ok(())
    }

    /// Start background flush task
    pub async fn start_background_flush(&self) {
        let writer = self.clone_for_background();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(writer.batch_timeout);
            loop {
                interval.tick().await;
                let _ = writer.flush_transitions().await;
                let _ = writer.flush_fills().await;
            }
        });
    }

    /// Start retry scheduler task (Phase 7)
    pub async fn start_retry_scheduler(&self) {
        let writer = self.clone_for_background();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(RETRY_INTERVAL_MS));
            loop {
                interval.tick().await;
                let _ = writer.retry_failed_transitions().await;
            }
        });
    }

    /// Retry failed transitions (Phase 7)
    async fn retry_failed_transitions(&self) -> Result<()> {
        let mut failed = self.failed_transitions.lock().await;
        if failed.is_empty() {
            return Ok(());
        }

        let now = Instant::now();
        
        // Filter transitions ready for retry (collect indices in reverse order for safe removal)
        let mut ready_indices: Vec<usize> = Vec::new();
        for (idx, ft) in failed.iter().enumerate() {
            if ft.next_retry <= now {
                ready_indices.push(idx);
            }
        }

        // Process ready transitions (in reverse order to maintain indices)
        for &idx in ready_indices.iter().rev() {
            let ft = failed.remove(idx);
            
            // Look up order_id from client_order_id
            match self.lookup_order_id(&ft.client_order_id).await {
                Ok(Some(order_id)) => {
                    // Retry with correct order_id
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
                Ok(None) => {
                    // Order not found: may have been deleted, move to dead letter
                    warn!(
                        client_order_id = %ft.client_order_id,
                        "Order not found for client_order_id, moving to dead letter queue"
                    );
                    self.move_to_dead_letter(ft).await?;
                }
                Err(e) => {
                    // Lookup error: retry later (put back in queue)
                    warn!(
                        client_order_id = %ft.client_order_id,
                        error = %e,
                        "Failed to lookup order_id, will retry later"
                    );
                    failed.push(ft);
                }
            }
        }

        Ok(())
    }

    /// Look up order_id from client_order_id
    async fn lookup_order_id(
        &self,
        client_order_id: &str,
    ) -> Result<Option<Uuid>> {
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
        }
    }
}
