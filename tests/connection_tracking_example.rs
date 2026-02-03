//! Example integration of connection tracking in tests
//!
//! This shows how to integrate connection tracking into the test suite
//! to diagnose connection pool exhaustion issues.

use super::connection_tracker::{
    ConnectionComponent, end_operation, log_component_summary, log_pool_state, start_operation,
};
use sqlx::PgPool;

/// Example: Track a database operation in a test
pub async fn example_tracked_operation(pool: &PgPool, test_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Log pool state before operation
    log_pool_state(pool, &format!("Before operation in {}", test_name));
    
    // Start tracking the operation
    let op_id = start_operation(
        ConnectionComponent::DatabaseReader,
        "list_active_pairs",
        Some(test_name),
    );
    
    // Perform the actual database operation
    let result = sqlx::query("SELECT 1").execute(pool).await;
    
    // End tracking (connection is released when query completes)
    end_operation(op_id);
    
    // Log pool state after operation
    log_pool_state(pool, &format!("After operation in {}", test_name));
    
    result?;
    Ok(())
}

/// Example: Track health check operations
pub async fn example_tracked_health_check(pool: &PgPool) -> Result<(), Box<dyn std::error::Error>> {
    let op_id = start_operation(
        ConnectionComponent::HealthCheck,
        "ensure_pool_healthy",
        None,
    );
    
    let result = sqlx::query("SELECT 1").execute(pool).await;
    
    end_operation(op_id);
    
    result?;
    Ok(())
}

/// Example: Track test lifecycle
pub fn example_test_lifecycle(pool: &PgPool, test_name: &str) {
    // At test start
    log_pool_state(pool, &format!("Test START: {}", test_name));
    log_component_summary();
    
    // At test end (before cleanup)
    log_pool_state(pool, &format!("Test END (before cleanup): {}", test_name));
    log_component_summary();
    
    // After cleanup
    log_pool_state(pool, &format!("Test END (after cleanup): {}", test_name));
    log_component_summary();
}

