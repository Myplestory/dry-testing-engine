//! Dry Testing Engine - CLI Entry Point
//!
//! Main entry point for running the dry testing engine as a standalone application.

use dry_testing_engine::DryTestingEngine;
use tracing::{info, error};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "dry_testing_engine=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Dry Testing Engine...");

    // Load environment variables
    dotenv::dotenv().ok();

    // Initialize engine
    let engine = DryTestingEngine::new().await?;

    // Run engine
    if let Err(e) = engine.run().await {
        error!("Engine error: {}", e);
        return Err(e.into());
    }

    Ok(())
}

