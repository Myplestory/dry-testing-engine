//! Connection usage tracking for debugging pool exhaustion issues
//!
//! This module provides utilities to track database connection acquisition and release
//! with unique identifiers to help diagnose connection leaks and pool exhaustion.

use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Unique identifier for a database operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId(Uuid);

impl OperationId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for OperationId {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about an active connection operation
#[derive(Debug, Clone)]
pub struct ActiveOperation {
    pub operation_id: OperationId,
    pub component: String,
    pub operation: String,
    pub started_at: Instant,
    pub test_name: Option<String>,
}

/// Component types that use database connections
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionComponent {
    DatabaseWriter,
    DatabaseReader,
    IntentGenerator,
    HealthCheck,
    TestSetup,
    BackgroundFlush,
    RetryScheduler,
    Coordinator,
    StateMachine,
}

impl ConnectionComponent {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionComponent::DatabaseWriter => "DatabaseWriter",
            ConnectionComponent::DatabaseReader => "DatabaseReader",
            ConnectionComponent::IntentGenerator => "IntentGenerator",
            ConnectionComponent::HealthCheck => "HealthCheck",
            ConnectionComponent::TestSetup => "TestSetup",
            ConnectionComponent::BackgroundFlush => "BackgroundFlush",
            ConnectionComponent::RetryScheduler => "RetryScheduler",
            ConnectionComponent::Coordinator => "Coordinator",
            ConnectionComponent::StateMachine => "StateMachine",
        }
    }
}

/// Global connection tracker (thread-safe)
pub struct ConnectionTracker {
    active_operations: Arc<Mutex<HashMap<OperationId, ActiveOperation>>>,
    operation_counter: Arc<Mutex<u64>>,
}

impl ConnectionTracker {
    /// Create a new connection tracker
    pub fn new() -> Self {
        Self {
            active_operations: Arc::new(Mutex::new(HashMap::new())),
            operation_counter: Arc::new(Mutex::new(0)),
        }
    }

    /// Start tracking a connection operation
    pub fn start_operation(
        &self,
        component: ConnectionComponent,
        operation: impl Into<String>,
        test_name: Option<&str>,
    ) -> OperationId {
        let operation_id = OperationId::new();
        let counter = {
            let mut counter = self.operation_counter.lock().unwrap();
            *counter += 1;
            *counter
        };

        let active_op = ActiveOperation {
            operation_id,
            component: component.as_str().to_string(),
            operation: operation.into(),
            started_at: Instant::now(),
            test_name: test_name.map(|s| s.to_string()),
        };

        {
            let mut ops = self.active_operations.lock().unwrap();
            ops.insert(operation_id, active_op.clone());
        }

        eprintln!(
            "   🔌 [CONN-{:04}] ACQUIRE | Component: {} | Operation: {} | Test: {} | Total Active: {}",
            counter,
            active_op.component,
            active_op.operation,
            active_op.test_name.as_deref().unwrap_or("N/A"),
            {
                let ops = self.active_operations.lock().unwrap();
                ops.len()
            }
        );

        operation_id
    }

    /// Stop tracking a connection operation
    pub fn end_operation(&self, operation_id: OperationId) {
        let (component, operation, duration, test_name) = {
            let mut ops = self.active_operations.lock().unwrap();
            if let Some(op) = ops.remove(&operation_id) {
                let duration = op.started_at.elapsed();
                (
                    op.component,
                    op.operation,
                    duration,
                    op.test_name,
                )
            } else {
                eprintln!(
                    "   ⚠️  [CONN] RELEASE | Operation ID {} not found in tracker",
                    operation_id.as_uuid()
                );
                return;
            }
        };

        let remaining = {
            let ops = self.active_operations.lock().unwrap();
            ops.len()
        };

        eprintln!(
            "   🔓 [CONN] RELEASE | Component: {} | Operation: {} | Duration: {:?} | Test: {} | Remaining Active: {}",
            component,
            operation,
            duration,
            test_name.as_deref().unwrap_or("N/A"),
            remaining
        );
    }

    /// Get snapshot of all active operations
    pub fn get_active_operations(&self) -> Vec<ActiveOperation> {
        let ops = self.active_operations.lock().unwrap();
        ops.values().cloned().collect()
    }

    /// Log current pool state and active operations
    pub fn log_pool_state(&self, pool: &PgPool, context: &str) {
        let active_ops = self.get_active_operations();
        let active_count = active_ops.len();
        let is_closed = pool.is_closed();

        eprintln!(
            "   📊 [POOL-STATE] {} | Pool Closed: {} | Active Operations: {}",
            context, is_closed, active_count
        );

        // Log connection statistics (spawn async task since this is async)
        // Only spawn if pool is not closed to avoid querying a closed pool
        if !is_closed {
            let pool_clone = pool.clone();
            let context_clone = context.to_string();
            tokio::spawn(async move {
                if let Err(e) = log_pool_connection_stats(&pool_clone, &context_clone).await {
                    eprintln!("   ⚠️  Failed to log pool connection stats: {}", e);
                }
            });
        } else {
            eprintln!(
                "   ⚠️  [POOL-STATS] {} | Pool is closed, skipping connection stats",
                context
            );
        }

        if !active_ops.is_empty() {
            eprintln!("   📋 Active Operations:");
            for (idx, op) in active_ops.iter().enumerate() {
                let duration = op.started_at.elapsed();
                eprintln!(
                    "      {}. [{}] {} | {} | Duration: {:?} | Test: {}",
                    idx + 1,
                    op.operation_id.as_uuid(),
                    op.component,
                    op.operation,
                    duration,
                    op.test_name.as_deref().unwrap_or("N/A")
                );
            }
        }
    }

    /// Log summary of operations by component
    pub fn log_component_summary(&self) {
        let ops = self.get_active_operations();
        let mut by_component: HashMap<String, Vec<&ActiveOperation>> = HashMap::new();

        for op in &ops {
            by_component
                .entry(op.component.clone())
                .or_insert_with(Vec::new)
                .push(op);
        }

        if !by_component.is_empty() {
            eprintln!("   📊 [SUMMARY] Active Operations by Component:");
            for (component, operations) in by_component.iter() {
                eprintln!(
                    "      {}: {} operation(s)",
                    component,
                    operations.len()
                );
                for op in operations.iter().take(3) {
                    let duration = op.started_at.elapsed();
                    eprintln!(
                        "         - {} (Duration: {:?})",
                        op.operation,
                        duration
                    );
                }
                if operations.len() > 3 {
                    eprintln!("         ... and {} more", operations.len() - 3);
                }
            }
        }
    }
}

impl Default for ConnectionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Global connection tracker instance
use std::sync::OnceLock;

static CONNECTION_TRACKER: OnceLock<ConnectionTracker> = OnceLock::new();

fn get_tracker() -> &'static ConnectionTracker {
    CONNECTION_TRACKER.get_or_init(|| ConnectionTracker::new())
}

/// Helper function to log pool state at key points
pub fn log_pool_state(pool: &PgPool, context: &str) {
    get_tracker().log_pool_state(pool, context);
}

/// Helper function to log component summary
pub fn log_component_summary() {
    get_tracker().log_component_summary();
}

/// Start tracking a connection operation
pub fn start_operation(
    component: ConnectionComponent,
    operation: impl Into<String>,
    test_name: Option<&str>,
) -> OperationId {
    get_tracker().start_operation(component, operation, test_name)
}

/// End tracking a connection operation
pub fn end_operation(operation_id: OperationId) {
    get_tracker().end_operation(operation_id);
}

/// Clean up all active operations (for test cleanup)
/// 
/// This is safe to call multiple times and from any thread.
/// Returns the number of operations cleaned up.
pub fn cleanup_all_operations() -> usize {
    let tracker = get_tracker();
    let ops = tracker.get_active_operations();
    let count = ops.len();
    
    if count > 0 {
        eprintln!("   🧹 Cleaning up {} stale operation(s)...", count);
        for op in ops {
            eprintln!(
                "      - {} | {} | Duration: {:?} | Test: {}",
                op.component,
                op.operation,
                op.started_at.elapsed(),
                op.test_name.as_deref().unwrap_or("N/A")
            );
            end_operation(op.operation_id);
        }
    }
    
    count
}

/// Get pool connection statistics by querying PostgreSQL
/// 
/// Note: sqlx doesn't expose connection counts directly, so we query
/// PostgreSQL's pg_stat_activity to estimate connection usage
/// 
/// This function is non-blocking and will not block test execution even if the pool is exhausted.
/// Uses a short timeout (500ms) to prevent diagnostic queries from consuming connection attempts.
/// 
/// # Safety
/// This function will gracefully handle closed pools and return Ok(()) if the pool is closed.
pub async fn log_pool_connection_stats(pool: &PgPool, context: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check if pool is closed before attempting queries
    if pool.is_closed() {
        eprintln!(
            "   ⚠️  [POOL-STATS] {} | Pool is closed, skipping connection stats",
            context
        );
        return Ok(());
    }
    // Use short timeout to prevent blocking when pool is exhausted
    // This ensures diagnostic queries don't consume connection attempts or block test execution
    const STATS_TIMEOUT: Duration = Duration::from_millis(500);
    
    // Query PostgreSQL for active connections to this database
    // This gives us an estimate of how many connections are in use
    let query = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) 
        FROM pg_stat_activity 
        WHERE datname = current_database()
          AND state = 'active'
        "#
    );
    
    // Use timeout to prevent blocking when pool is exhausted
    let active_result = tokio::time::timeout(STATS_TIMEOUT, query.fetch_one(pool)).await;
    
    match active_result {
        Ok(Ok(active_connections)) => {
            eprintln!(
                "   📊 [POOL-STATS] {} | Active DB Connections: {} | Pool Closed: {}",
                context, active_connections, pool.is_closed()
            );
        }
        Ok(Err(e)) => {
            eprintln!(
                "   ⚠️  [POOL-STATS] {} | Failed to query active connections: {}",
                context, e
            );
        }
        Err(_) => {
            // Timeout - pool may be exhausted, don't block test execution
            eprintln!(
                "   ⚠️  [POOL-STATS] {} | Query timeout (pool may be exhausted) - skipping remaining stats",
                context
            );
            // Don't try idle query if active query timed out
            return Ok(());
        }
    }
    
    // Only try idle query if active query succeeded (didn't timeout)
    let idle_query = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) 
        FROM pg_stat_activity 
        WHERE datname = current_database()
          AND state = 'idle'
          AND wait_event_type IS NULL
        "#
    );
    
    // Use timeout for idle query as well
    let idle_result = tokio::time::timeout(STATS_TIMEOUT, idle_query.fetch_one(pool)).await;
    
    match idle_result {
        Ok(Ok(idle_connections)) => {
            eprintln!(
                "   📊 [POOL-STATS] {} | Idle DB Connections: {}",
                context, idle_connections
            );
        }
        Ok(Err(e)) => {
            // Non-fatal - some PostgreSQL versions may not support this
            eprintln!(
                "   ⚠️  [POOL-STATS] {} | Failed to query idle connections: {}",
                context, e
            );
        }
        Err(_) => {
            // Timeout - don't block test execution
            eprintln!(
                "   ⚠️  [POOL-STATS] {} | Idle query timeout - skipping",
                context
            );
        }
    }
    
    // Always return Ok - diagnostic queries should never fail tests
    Ok(())
}

