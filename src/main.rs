mod db;
mod ingestor;
mod nntp;
mod settings;

use db::Database;
use ingestor::Ingestor;
use settings::Settings;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    info!("Starting Sashiko...");

    // Load settings
    let settings = match Settings::new() {
        Ok(s) => {
            info!("Settings loaded successfully");
            s
        }
        Err(e) => {
            error!("Failed to load settings: {}", e);
            return Err(e.into());
        }
    };

    info!("Settings: {:?}", settings);

    // Initialize Database
    let db = Database::new(settings.database).await?;
    db.migrate().await?;

    // Start Ingestor
    let ingestor = Ingestor::new(settings.nntp, db);
    tokio::spawn(async move {
        if let Err(e) = ingestor.run().await {
            error!("Ingestor fatal error: {}", e);
        }
    });

    // Keep the main thread running
    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");

    Ok(())
}
