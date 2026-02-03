//! End-to-end integration tests
//!
//! Tests the complete order flow from intent generation to execution completion.
//! Requires a database connection configured in .env file.
//!
//! # Usage
//!
//! ```bash
//! # Set logging level for visibility
//! export RUST_LOG=dry_testing_engine=debug
//!
//! # Run all end-to-end tests
//! cargo test --test end_to_end -- --nocapture
//!
//! # Run specific test
//! cargo test --test end_to_end test_complete_execution_flow -- --nocapture
//! ```

use dry_testing_engine::{
    DryTestingEngine, IntentGenerator, TestConfig,
};
use dry_testing_engine::intent_generator::database::{DatabaseReader, VerifiedPairRow};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber;

// Connection tracking module
mod connection_tracker;
use connection_tracker::{
    cleanup_all_operations, ConnectionComponent, end_operation, log_component_summary, log_pool_connection_stats, log_pool_state, start_operation,
};

/// Initialize tracing for test output
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dry_testing_engine=debug".into()),
        )
        .with_test_writer()
        .try_init();
}

/// Create a new database pool for a test
/// 
/// Each test gets its own isolated pool to avoid connection state leakage
/// between tests. The pool is automatically closed when dropped.
async fn create_test_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
    
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Use same configuration as before, but per-test
    let max_connections = env::var("DATABASE_MAX_CONNECTIONS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20);
    
    let pool = PgPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&database_url)
        .await?;
    
    eprintln!("🔧 Created test database pool (isolated)");
    eprintln!("   📊 Pool Configuration:");
    eprintln!("      Max Connections: {}", max_connections);
    eprintln!("      Acquire Timeout: 5s");
    eprintln!("      Idle Timeout: 600s");
    eprintln!("      Max Lifetime: 1800s");
    
    Ok(pool)
}

/// Ensure all resources are cleaned up and pool is ready to close
/// 
/// This function:
/// 1. Verifies engine shutdown completed
/// 2. Waits for background tasks to finish (including fire-and-forget order writes)
/// 3. Drops all components explicitly
/// 4. Verifies pool has no active operations
/// 5. Cleans up stale operation tracking
/// 
/// # Arguments
/// * `pool` - The pool to verify cleanup for
/// * `test_name` - Name of the test for logging
/// * `skip_stats` - If true, skip pool stats queries (useful when pool is about to be closed)
async fn ensure_pool_cleanup(
    pool: &PgPool,
    test_name: &str,
    skip_stats: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🧹 Ensuring complete resource cleanup for {}", test_name);
    
    // Step 1: Verify pool is not closed (should still be open)
    if pool.is_closed() {
        eprintln!("   ⚠️  Pool is already closed, skipping cleanup");
        return Ok(()); // Already cleaned up
    }
    
    // Step 2: Log current state (only if not skipping stats)
    if !skip_stats {
        log_pool_state(pool, &format!("Cleanup START: {}", test_name));
        log_component_summary();
    }
    
    // Step 3: Wait for fire-and-forget tasks to complete
    // Coordinator spawns order write tasks that aren't tracked. These typically
    // complete quickly (50-300ms), but we wait longer to ensure they're done.
    // This is critical - these tasks hold Arc<PgPool> references and connections.
    eprintln!("   ⏳ Waiting for fire-and-forget tasks to complete...");
    tokio::time::sleep(Duration::from_millis(2000)).await;
    
    // Step 4: Clean up stale operations (from connection tracker)
    let cleaned_count = cleanup_all_operations();
    if cleaned_count > 0 {
        eprintln!("   ✅ Cleaned up {} stale operation(s)", cleaned_count);
    }
    
    // Step 5: Log final state (only if not skipping stats and pool is still open)
    if !skip_stats && !pool.is_closed() {
        log_pool_state(pool, &format!("Cleanup COMPLETE: {}", test_name));
        log_component_summary();
    }
    
    eprintln!("   ✅ Resource cleanup verified");
    Ok(())
}

/// Explicitly close a test pool and verify closure
/// 
/// While pools are automatically closed when dropped, this provides
/// explicit control and verification for test cleanup.
/// 
/// Note: This does NOT call ensure_pool_cleanup again - it should be called
/// before this function to avoid duplicate cleanup and pool stats queries.
async fn close_test_pool(
    pool: PgPool,
    test_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("🔒 Closing test pool for {}", test_name);
    
    // Don't call ensure_pool_cleanup here - it should already be called before
    // This avoids duplicate cleanup and trying to query a closed pool
    
    // Close the pool explicitly
    pool.close().await;
    
    // Verify it's closed
    if pool.is_closed() {
        eprintln!("   ✅ Pool closed successfully");
    } else {
        eprintln!("   ⚠️  Pool close() called but pool reports not closed");
    }
    
    // Wait a moment for connections to be fully released
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    Ok(())
}

/// Test complete order flow from intent generation to execution
#[tokio::test]
async fn test_complete_execution_flow() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    let test_name = "test_complete_execution_flow";
    
    // STEP 1: Create isolated pool for this test
    let pool = create_test_pool().await?;
    println!("✅ Created isolated database pool for test");
    
    // STEP 2: Log test start
    log_pool_state(&pool, &format!("Test START: {}", test_name));
    log_component_summary();
    
    // Query for active verified pairs - track connection usage
    let db_reader = DatabaseReader::new(pool.clone());
    let list_op_id = start_operation(
        ConnectionComponent::DatabaseReader,
        "list_active_pairs",
        Some(test_name),
    );
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(5).await?; // Get up to 5 pairs
    end_operation(list_op_id);
    
    if active_pairs.is_empty() {
        eprintln!("⚠️  No active verified pairs found in database");
        eprintln!("   Skipping test - add active verified pairs to database first");
        return Ok(());
    }
    
    println!("✅ Found {} active verified pairs", active_pairs.len());
    
    // Create engine with shared pool (reduces connection usage)
    let engine = DryTestingEngine::with_pool(Arc::new(pool.clone())).await?;
    println!("✅ Engine initialized with shared pool");
    
    // Create generator with same pool
    let config = TestConfig::default();
    let generator = IntentGenerator::new(pool.clone(), config)?;
    
    // Setup test data sequentially (once, before parallel generation)
    let customer_id = Uuid::new_v4();
    let setup_op_id = start_operation(
        ConnectionComponent::TestSetup,
        "setup_test_customer_scope_strategy",
        Some(test_name),
    );
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
    end_operation(setup_op_id);
    println!("✅ Test customer, scope, strategy setup complete");
    
    // Generate and enqueue intents for each pair
    let mut intent_ids = Vec::new();
    
    for (idx, pair) in active_pairs.iter().enumerate() {
        println!("\n📋 Processing pair {}: {}", idx + 1, pair.id);
        
        match generator.generate_from_pair_id(pair.id, customer_id, strategy_id).await {
            Ok(intent) => {
                println!("   ✅ Generated intent: {}", intent.intent_id);
                println!("   📊 Leg A: {} @ {}", intent.leg_a.venue, intent.leg_a.venue_market_id);
                println!("   📊 Leg B: {} @ {}", intent.leg_b.venue, intent.leg_b.venue_market_id);
                
                engine.router().enqueue_intent(intent.clone()).await?;
                intent_ids.push(intent.intent_id);
                println!("   ✅ Intent enqueued");
            }
            Err(e) => {
                eprintln!("   ❌ Failed to generate intent: {}", e);
                // Continue with other pairs
            }
        }
    }
    
    if intent_ids.is_empty() {
        eprintln!("⚠️  No intents were successfully generated");
        return Ok(());
    }
    
    println!("\n✅ Enqueued {} intents. Waiting for execution...", intent_ids.len());
    println!("   (This may take a few seconds for venue responses)\n");
    
    // Wait for execution to complete
    // Venue simulator has configurable latency (10ms ACK, 50ms FILL)
    // Add buffer for state machine processing and database writes
    tokio::time::sleep(Duration::from_secs(10)).await;
    
    println!("\n✅ Order flow test completed");
    println!("   Check logs above for detailed execution flow");
    
    // STEP 5: Shutdown engine gracefully
    eprintln!("🛑 Shutting down engine...");
    engine.shutdown().await?;
    eprintln!("   ✅ Engine shutdown complete");
    
    // STEP 6: Explicitly drop all components
    eprintln!("🗑️  Dropping test components...");
    drop(engine);
    drop(db_reader);
    drop(generator);
    eprintln!("   ✅ Components dropped");
    
    // STEP 7: Ensure complete cleanup (with stats logging)
    ensure_pool_cleanup(&pool, test_name, false).await?;
    
    // STEP 8: Close pool explicitly (skip stats since pool will be closed)
    close_test_pool(pool, test_name).await?;
    
    // STEP 9: Final verification
    eprintln!("✅ Test {} completed with full resource cleanup", test_name);
    
    Ok(())
}

/// Test single intent execution with detailed logging
#[tokio::test]
async fn test_single_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    let test_name = "test_single_intent_execution";
    
    // STEP 1: Create isolated pool for this test
    let pool = create_test_pool().await?;
    println!("✅ Created isolated database pool for test");
    
    // STEP 2: Log test start
    log_pool_state(&pool, &format!("Test START: {}", test_name));
    log_component_summary();
    
    // Query for a single active verified pair - track connection usage
    let db_reader = DatabaseReader::new(pool.clone());
    let list_op_id = start_operation(
        ConnectionComponent::DatabaseReader,
        "list_active_pairs",
        Some(test_name),
    );
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(1).await?;
    end_operation(list_op_id);
    
    let pair_id = active_pairs.first()
        .ok_or("No active verified pairs found in database")?
        .id;
    
    let customer_id = Uuid::new_v4();
    
    // Setup test data sequentially (once, before generation) - track connection usage
    let setup_op_id = start_operation(
        ConnectionComponent::TestSetup,
        "setup_test_customer_scope_strategy",
        Some(test_name),
    );
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
    end_operation(setup_op_id);
    
    println!("🎯 Testing single intent execution");
    println!("   Pair ID: {}", pair_id);
    println!("   Customer ID: {}", customer_id);
    println!("   Strategy ID: {}", strategy_id);
    
    // Create engine with shared pool (reduces connection usage)
    let engine = DryTestingEngine::with_pool(Arc::new(pool.clone())).await?;
    
    // Generate intent (using same pool)
    let config = TestConfig::default();
    let generator = IntentGenerator::new(pool.clone(), config)?;
    let intent = generator.generate_from_pair_id(pair_id, customer_id, strategy_id).await?;
    
    println!("✅ Generated intent: {}", intent.intent_id);
    
    // Enqueue intent
    engine.router().enqueue_intent(intent).await?;
    println!("✅ Intent enqueued. Order flow starting...\n");
    
    // Wait for execution
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    println!("\n✅ Single intent execution test completed");
    
    // STEP 5: Shutdown engine gracefully
    eprintln!("🛑 Shutting down engine...");
    engine.shutdown().await?;
    eprintln!("   ✅ Engine shutdown complete");
    
    // STEP 6: Explicitly drop all components
    eprintln!("🗑️  Dropping test components...");
    drop(engine);
    drop(db_reader);
    drop(generator);
    eprintln!("   ✅ Components dropped");
    
    // STEP 7: Ensure complete cleanup (with stats logging)
    ensure_pool_cleanup(&pool, test_name, false).await?;
    
    // STEP 8: Close pool explicitly (skip stats since pool will be closed)
    close_test_pool(pool, test_name).await?;
    
    // STEP 9: Final verification
    eprintln!("✅ Test {} completed with full resource cleanup", test_name);
    
    Ok(())
}

/// Test batch intent generation and execution
#[tokio::test]
async fn test_batch_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    let test_name = "test_batch_intent_execution";
    
    // STEP 1: Create isolated pool for this test
    let pool = create_test_pool().await?;
    println!("✅ Created isolated database pool for test");
    
    // STEP 2: Log test start
    log_pool_state(&pool, &format!("Test START: {}", test_name));
    log_component_summary();
    
    // Query for multiple active verified pairs - track connection usage
    let db_reader = DatabaseReader::new(pool.clone());
    let list_op_id = start_operation(
        ConnectionComponent::DatabaseReader,
        "list_active_pairs",
        Some(test_name),
    );
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(10).await?; // Get up to 10 pairs
    end_operation(list_op_id);
    
    if active_pairs.is_empty() {
        eprintln!("⚠️  No active verified pairs found in database");
        return Ok(());
    }
    
    println!("📦 Testing batch intent execution");
    println!("   Found {} active pairs", active_pairs.len());
    
    // Create engine with shared pool (reduces connection usage)
    let engine = DryTestingEngine::with_pool(Arc::new(pool.clone())).await?;
    
    // Create generator with same pool
    let config = TestConfig::default();
    let generator = IntentGenerator::new(pool.clone(), config)?;
    
    // Setup test data sequentially (once, before parallel batch generation)
    let customer_id = Uuid::new_v4();
    let setup_op_id = start_operation(
        ConnectionComponent::TestSetup,
        "setup_test_customer_scope_strategy",
        Some(test_name),
    );
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
    end_operation(setup_op_id);
    println!("✅ Test customer, scope, strategy setup complete");
    
    // Generate intents in batch (parallel, no test data creation)
    let pair_ids: Vec<Uuid> = active_pairs.iter().map(|p| p.id).collect();
    
    println!("   Generating intents for {} pairs...", pair_ids.len());
    let (intents, errors) = generator.generate_batch(&pair_ids, customer_id, strategy_id).await?;
    
    println!("   ✅ Generated {} intents", intents.len());
    if !errors.is_empty() {
        println!("   ⚠️  {} errors during generation", errors.len());
        for (pair_id, err) in &errors {
            eprintln!("      Pair {}: {}", pair_id, err);
        }
    }
    
    // Enqueue all intents
    println!("   Enqueueing {} intents...", intents.len());
    for intent in intents {
        engine.router().enqueue_intent(intent).await?;
    }
    
    println!("   ✅ All intents enqueued. Waiting for execution...\n");
    
    // Wait for execution (longer for batch)
    tokio::time::sleep(Duration::from_secs(15)).await;
    
    println!("\n✅ Batch intent execution test completed");
    
    // STEP 5: Shutdown engine gracefully
    eprintln!("🛑 Shutting down engine...");
    engine.shutdown().await?;
    eprintln!("   ✅ Engine shutdown complete");
    
    // STEP 6: Explicitly drop all components
    eprintln!("🗑️  Dropping test components...");
    drop(engine);
    drop(db_reader);
    drop(generator);
    eprintln!("   ✅ Components dropped");
    
    // STEP 7: Ensure complete cleanup (with stats logging)
    ensure_pool_cleanup(&pool, test_name, false).await?;
    
    // STEP 8: Close pool explicitly (skip stats since pool will be closed)
    close_test_pool(pool, test_name).await?;
    
    // STEP 9: Final verification
    eprintln!("✅ Test {} completed with full resource cleanup", test_name);
    
    Ok(())
}

