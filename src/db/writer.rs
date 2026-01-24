//! Database writer with batching

use crate::types::errors::Result;
use crate::types::order::StateTransition;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::info;
use uuid::Uuid;

/// Database writer with batched writes
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
        })
    }

    /// Write order immediately (need DB ID for tracking)
    pub async fn write_order(&self, order: &crate::types::order::Order) -> Result<uuid::Uuid> {
        // Insert order with ON CONFLICT DO NOTHING for idempotency
        // Note: Database schema uses user_id, but we use customer_id in our types
        // For now, using customer_id as user_id (schema alignment issue to fix separately)
        let order_id = sqlx::query_scalar::<_, Uuid>(
            r#"
            INSERT INTO orders (
                user_id, strategy_id, pair_id, leg, venue, venue_market_id, venue_outcome_id,
                client_order_id, venue_order_id, side, limit_price_int, price_scale,
                size_int, size_scale, status, filled_size_int,
                created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            ON CONFLICT (user_id, venue, client_order_id) DO UPDATE
            SET updated_at = EXCLUDED.updated_at
            RETURNING id
            "#,
        )
        .bind(order.customer_id) // Using customer_id as user_id (schema alignment issue)
        .bind(order.strategy_id)
        .bind(order.pair_id)
        .bind(<crate::types::order::OrderLeg as Into<&str>>::into(
            order.leg,
        ))
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

    /// Flush transitions batch
    async fn flush_transitions(&self) -> Result<()> {
        let mut pending = self.pending_transitions.lock().await;
        if pending.is_empty() {
            return Ok(());
        }

        let batch = pending.drain(..).collect::<Vec<_>>();
        drop(pending);

        // TODO: Batch insert to order_events table
        info!("Flushing {} transitions to database", batch.len());

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

    /// Clone for background task (only clones Arc references)
    fn clone_for_background(&self) -> Self {
        Self {
            db: self.db.clone(),
            transition_batch_size: self.transition_batch_size,
            fill_batch_size: self.fill_batch_size,
            batch_timeout: self.batch_timeout,
            pending_transitions: self.pending_transitions.clone(),
            pending_fills: self.pending_fills.clone(),
        }
    }
}
