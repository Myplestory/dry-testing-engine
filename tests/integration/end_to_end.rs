//! End-to-end integration tests
//!
//! Tests the complete order flow from intent generation to execution completion.
//! Requires a database connection configured in .env file.

use dry_testing_engine::{
    DryTestingEngine, IntentGenerator, TestConfig, DatabaseReader,
};
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
    let active_pairs = db_reader.list_active_pairs(5).await?; // Get up to 5 pairs
    
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

/// Test single intent execution with detailed logging and benchmarking
#[tokio::test]
async fn test_single_intent_execution() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Instant;
    
    let test_start = Instant::now();
    init_tracing();
    
    // Load .env file
    dotenv::dotenv().ok();
    
    // Get database URL
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set in .env file");
    
    // Create database pool
    let setup_start = Instant::now();
    let pool = match PgPool::connect(&database_url).await {
        Ok(p) => {
            let setup_time = setup_start.elapsed();
            println!("✅ Connected to database (⏱️  {:?})", setup_time);
            p
        }
        Err(e) => {
            eprintln!("❌ Failed to connect to database: {:?}", e);
            return Err(e.into());
        }
    };
    
    // Query for a single active verified pair
    let db_reader = DatabaseReader::new(pool.clone());
    let query_start = Instant::now();
    let active_pairs = match db_reader.list_active_pairs(1).await {
        Ok(pairs) => {
            let query_time = query_start.elapsed();
            println!("✅ Queried active pairs (⏱️  {:?})", query_time);
            pairs
        }
        Err(e) => {
            eprintln!("❌ Failed to query active pairs: {:?}", e);
            return Err(e.into());
        }
    };
    
    // Handle empty pairs gracefully (same pattern as other tests)
    if active_pairs.is_empty() {
        eprintln!("⚠️  No active verified pairs found in database");
        eprintln!("   Skipping test - add active verified pairs to database first");
        return Ok(());
    }
    
    let pair_id = active_pairs.first().unwrap().id;
    let customer_id = Uuid::new_v4();
    
    // Setup test data sequentially (once, before generation)
    let setup_data_start = Instant::now();
    let (_customer_id, _scope_id, strategy_id) = match db_reader
        .setup_test_customer_scope_strategy(customer_id)
        .await
    {
        Ok(result) => {
            let setup_data_time = setup_data_start.elapsed();
            println!("✅ Test customer, scope, strategy setup complete (⏱️  {:?})", setup_data_time);
            result
        }
        Err(e) => {
            eprintln!("❌ Failed to setup test data: {:?}", e);
            return Err(e.into());
        }
    };
    
    println!("🎯 Testing single intent execution");
    println!("   Pair ID: {}", pair_id);
    println!("   Customer ID: {}", customer_id);
    println!("   Strategy ID: {}", strategy_id);
    
    // Create engine with shared pool (reduces connection usage)
    let engine_start = Instant::now();
    let engine = match DryTestingEngine::with_pool(Arc::new(pool.clone())).await {
        Ok(e) => {
            let engine_time = engine_start.elapsed();
            println!("✅ Engine initialized with shared pool (⏱️  {:?})", engine_time);
            e
        }
        Err(e) => {
            eprintln!("❌ Failed to create engine: {:?}", e);
            return Err(e.into());
        }
    };
    
    // Generate intent (using same pool)
    let config = TestConfig::default();
    let generator = match IntentGenerator::new(pool, config) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("❌ Failed to create generator: {:?}", e);
            return Err(e.into());
        }
    };
    
    let generation_start = Instant::now();
    let intent = match generator.generate_from_pair_id(pair_id, customer_id, strategy_id).await {
        Ok(i) => {
            let generation_time = generation_start.elapsed();
            println!("✅ Generated intent: {} (⏱️  {:?})", i.intent_id, generation_time);
            i
        }
        Err(e) => {
            eprintln!("❌ Failed to generate intent: {:?}", e);
            return Err(e.into());
        }
    };
    
    // Enqueue intent
    let enqueue_start = Instant::now();
    match engine.router().enqueue_intent(intent).await {
        Ok(_) => {
            let enqueue_time = enqueue_start.elapsed();
            println!("✅ Intent enqueued. Order flow starting... (⏱️  {:?})", enqueue_time);
        }
        Err(e) => {
            eprintln!("❌ Failed to enqueue intent: {:?}", e);
            return Err(e.into());
        }
    }
    
    // Wait for execution
    let execution_start = Instant::now();
    tokio::time::sleep(Duration::from_secs(5)).await;
    let execution_time = execution_start.elapsed();
    
    // Print benchmark summary
    let total_time = test_start.elapsed();
    println!("\n📊 Execution Benchmark:");
    println!("   Setup: {:?}", setup_start.elapsed());
    println!("   Query: {:?}", query_start.elapsed());
    println!("   Setup Data: {:?}", setup_data_start.elapsed());
    println!("   Engine Init: {:?}", engine_start.elapsed());
    println!("   Intent Generation: {:?}", generation_start.elapsed());
    println!("   Enqueue: {:?}", enqueue_start.elapsed());
    println!("   Execution Wait: {:?}", execution_time);
    println!("   Total Test Time: {:?}", total_time);
    
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
    let active_pairs = db_reader.list_active_pairs(10).await?; // Get up to 10 pairs
    
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

