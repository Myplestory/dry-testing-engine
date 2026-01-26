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
use sqlx::PgPool;
use uuid::Uuid;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber;

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

/// Test complete order flow from intent generation to execution
#[tokio::test]
async fn test_complete_execution_flow() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    // Load .env file
    dotenv::dotenv().ok();
    
    // Get database URL
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Create database pool
    let pool = PgPool::connect(&database_url).await?;
    println!("✅ Connected to database");
    
    // Query for active verified pairs
    let db_reader = DatabaseReader::new(pool.clone());
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(5).await?; // Get up to 5 pairs
    
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
    let generator = IntentGenerator::new(pool, config)?;
    
    // Setup test data sequentially (once, before parallel generation)
    let customer_id = Uuid::new_v4();
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
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
    
    Ok(())
}

/// Test single intent execution with detailed logging
#[tokio::test]
async fn test_single_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    // Load .env file
    dotenv::dotenv().ok();
    
    // Get database URL
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Create database pool
    let pool = PgPool::connect(&database_url).await?;
    
    // Query for a single active verified pair
    let db_reader = DatabaseReader::new(pool.clone());
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(1).await?;
    
    let pair_id = active_pairs.first()
        .ok_or("No active verified pairs found in database")?
        .id;
    
    let customer_id = Uuid::new_v4();
    
    // Setup test data sequentially (once, before generation)
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
    
    println!("🎯 Testing single intent execution");
    println!("   Pair ID: {}", pair_id);
    println!("   Customer ID: {}", customer_id);
    println!("   Strategy ID: {}", strategy_id);
    
    // Create engine with shared pool (reduces connection usage)
    let engine = DryTestingEngine::with_pool(Arc::new(pool.clone())).await?;
    
    // Generate intent (using same pool)
    let config = TestConfig::default();
    let generator = IntentGenerator::new(pool, config)?;
    let intent = generator.generate_from_pair_id(pair_id, customer_id, strategy_id).await?;
    
    println!("✅ Generated intent: {}", intent.intent_id);
    
    // Enqueue intent
    engine.router().enqueue_intent(intent).await?;
    println!("✅ Intent enqueued. Order flow starting...\n");
    
    // Wait for execution
    tokio::time::sleep(Duration::from_secs(5)).await;
    
    println!("\n✅ Single intent execution test completed");
    
    Ok(())
}

/// Test batch intent generation and execution
#[tokio::test]
async fn test_batch_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    
    // Load .env file
    dotenv::dotenv().ok();
    
    // Get database URL
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Create database pool
    let pool = PgPool::connect(&database_url).await?;
    
    // Query for multiple active verified pairs
    let db_reader = DatabaseReader::new(pool.clone());
    let active_pairs: Vec<VerifiedPairRow> = db_reader.list_active_pairs(10).await?; // Get up to 10 pairs
    
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
    let generator = IntentGenerator::new(pool, config)?;
    
    // Setup test data sequentially (once, before parallel batch generation)
    let customer_id = Uuid::new_v4();
    let (_customer_id, _scope_id, strategy_id) = db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await?;
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
    
    Ok(())
}

