//! Strait Node — Main binary that wires all components together.
//!
//! Stub — will be implemented in Step 9.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    tracing::info!("Strait node starting up");

    // TODO: Wire everything together in Step 9
    // 1. Load AppConfig
    // 2. Run DB migrations
    // 3. Spawn ingesters
    // 4. Spawn join engine
    // 5. Start API server

    Ok(())
}
