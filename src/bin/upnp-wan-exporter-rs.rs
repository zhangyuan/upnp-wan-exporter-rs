use anyhow::Result;
use upnp_wan_exporter_rs::{Config, run_server};

#[tokio::main]
async fn main() -> Result<()> {
    // Try to load config from file, fallback to default
    let config = Config::from_file("config.toml")
        .unwrap_or_else(|_| {
            eprintln!("Warning: Could not load config.toml, using defaults");
            Config::default()
        });

    run_server(config).await
}
