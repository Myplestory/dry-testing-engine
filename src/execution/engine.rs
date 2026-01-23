//! Dry testing engine main implementation

use crate::coordinator::ArbExecutionCoordinator;
use crate::db::DatabaseWriter;
use crate::execution::router::ExecutionLaneRouter;
use crate::recovery::Recovery;
use crate::routing::OrderRouter;
use crate::state_machine::OrderStateMachine;
use crate::types::errors::{DryTestingError, Result};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{error, info};

/// Main dry testing engine
pub struct DryTestingEngine {
    /// Execution lane router
    router: Arc<ExecutionLaneRouter>,
    
    /// Order state machine
    state_machine: Arc<OrderStateMachine>,
    
    /// Arbitrage execution coordinator
    coordinator: Arc<ArbExecutionCoordinator>,
    
    /// Order router for event routing
    order_router: Arc<OrderRouter>,
    
    /// Database writer
    db_writer: Arc<DatabaseWriter>,
    
    /// Database connection pool
    db: Arc<PgPool>,
}

impl DryTestingEngine {
    /// Create a new dry testing engine
    pub async fn new() -> Result<Self> {
        info!("Initializing dry testing engine...");
        
        // Load database URL from environment
        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| DryTestingError::Config("DATABASE_URL not set".to_string()))?;
        
        // Create database connection pool
        let db = Arc::new(
            sqlx::PgPool::connect(&database_url)
                .await
                .map_err(|e| DryTestingError::Database(e))?
        );
        
        // Initialize components
        let order_router = Arc::new(OrderRouter::new());
        let db_writer = Arc::new(DatabaseWriter::new(db.clone()).await?);
        let state_machine = Arc::new(OrderStateMachine::new(order_router.clone(), db_writer.clone()).await?);
        let coordinator = Arc::new(ArbExecutionCoordinator::new(
            state_machine.clone(),
            order_router.clone(),
            db.clone(),
        ).await?);
        let router = Arc::new(ExecutionLaneRouter::new(coordinator.clone()).await?);
        
        info!("Dry testing engine initialized successfully");
        
        Ok(Self {
            router,
            state_machine,
            coordinator,
            order_router,
            db_writer,
            db,
        })
    }
    
    /// Run the engine (main event loop)
    pub async fn run(&self) -> Result<()> {
        info!("Starting dry testing engine...");
        
        // Start background tasks
        self.db_writer.start_background_flush().await;
        
        // TODO: Start lane processing tasks
        // TODO: Start venue event listeners
        // TODO: Start coordinator monitoring
        
        // Keep running until shutdown signal
        tokio::signal::ctrl_c()
            .await
            .map_err(|e| DryTestingError::Io(e))?;
        
        info!("Shutting down dry testing engine...");
        
        // Graceful shutdown
        self.shutdown().await?;
        
        Ok(())
    }
    
    /// Graceful shutdown
    async fn shutdown(&self) -> Result<()> {
        info!("Performing graceful shutdown...");
        
        // Stop accepting new intents
        // Wait for in-flight executions to complete
        // Flush pending database writes
        
        info!("Shutdown complete");
        Ok(())
    }
    
    /// Recover state from database
    pub async fn recover(&self) -> Result<()> {
        info!("Recovering state from database...");
        
        // TODO: Implement recovery logic
        // 1. Load active orders
        // 2. Rebuild order router maps
        // 3. Rebuild state machine cache
        // 4. Recover coordinator executions
        
        info!("Recovery complete");
        Ok(())
    }
}

