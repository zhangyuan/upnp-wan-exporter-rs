pub mod config;
pub mod metrics;
pub mod server;
pub mod upnp;

pub use config::{Config, UpnpConfig};
pub use metrics::{MetricsCollector, init_metrics};
pub use server::create_app;
pub use upnp::{TrafficStats, UpnpClient, UpnpDevice};

use anyhow::Result;
use std::net::SocketAddr;

/// Initialize and run the UPnP WAN exporter server
pub async fn run_server(config: Config) -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Apply UPnP discovery configuration (explicit host / location, if any).
    upnp::configure(config.upnp.clone());

    // Initialize Prometheus metrics
    init_metrics();

    tracing::info!("Starting UPnP WAN Exporter");

    // Build the router
    let app = create_app();

    // Start the server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.server.port));
    tracing::info!("Server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
