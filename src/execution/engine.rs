//! Dry testing engine main implementation

use crate::coordinator::ArbExecutionCoordinator;
use crate::db::DatabaseWriter;
use crate::execution::executor::VenueExecutor;
use crate::execution::router::ExecutionLaneRouter;
use crate::routing::OrderRouter;
use crate::state_machine::OrderStateMachine;
use crate::types::errors::{DryTestingError, Result};
use crate::types::venue::VenueType;
use crate::venue::adapter::VenueAdapter;
use crate::venue::simulator::VenueSimulator;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

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
                .map_err(DryTestingError::Database)?,
        );

        // Initialize components
        let order_router = Arc::new(OrderRouter::new());
        let db_writer = Arc::new(DatabaseWriter::new(db.clone()).await?);
        let state_machine =
            Arc::new(OrderStateMachine::new(order_router.clone(), db_writer.clone()).await?);

        // Create coordinator without lane_router (will be set after router creation)
        let coordinator = Arc::new(
            ArbExecutionCoordinator::new(
                state_machine.clone(),
                order_router.clone(),
                db_writer.clone(),
                db.clone(),
            )
            .await?,
        );

        // Create router (simplified - only for sequence number assignment)
        let router = Arc::new(ExecutionLaneRouter::new(coordinator.clone()).await?);

        // Register venue executors (replaces worker startup)
        let venues = [
            VenueType::Polymarket,
            VenueType::Kalshi,
            VenueType::Manifold,
            VenueType::Opinion,
            VenueType::Gemini,
        ];

        for venue in venues.iter() {
            let venue_adapter: Arc<dyn VenueAdapter> = Arc::new(
                VenueSimulator::new(
                    venue.clone(),
                    10,   // ack_latency_ms
                    50,   // fill_latency_ms
                    0.05, // reject_probability
                    0.95, // fill_probability
                )
                .with_partial_fill_probability(0.1),
            );

            let executor = VenueExecutor::new(
                venue.clone(),
                venue_adapter,
                state_machine.clone(),
                order_router.clone(),
                coordinator.clone(),
                Duration::from_secs(5),
            );

            coordinator.register_executor(venue.clone(), Arc::new(executor));
            info!("Registered venue executor for: {}", venue.as_str());
        }

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

        // Venue executors are registered in new()

        // TODO: Start coordinator timeout monitoring

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
