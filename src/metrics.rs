use lazy_static::lazy_static;
use prometheus::{Gauge, Registry, TextEncoder};
use crate::upnp::{UpnpClient, TrafficStats};
use tracing::{error, info};

lazy_static! {
    static ref REGISTRY: Registry = Registry::new();
    static ref BYTES_SENT: Gauge = Gauge::new(
        "upnp_wan_bytes_sent_total",
        "Total bytes sent through WAN connection"
    ).expect("metric can be created");
    static ref BYTES_RECEIVED: Gauge = Gauge::new(
        "upnp_wan_bytes_received_total", 
        "Total bytes received through WAN connection"
    ).expect("metric can be created");
    static ref PACKETS_SENT: Gauge = Gauge::new(
        "upnp_wan_packets_sent_total",
        "Total packets sent through WAN connection"
    ).expect("metric can be created");
    static ref PACKETS_RECEIVED: Gauge = Gauge::new(
        "upnp_wan_packets_received_total",
        "Total packets received through WAN connection"
    ).expect("metric can be created");
    static ref CONNECTION_STATUS: Gauge = Gauge::new(
        "upnp_wan_connection_status",
        "WAN connection status (1 = connected, 0 = disconnected)"
    ).expect("metric can be created");
    static ref SCRAPE_ERROR: Gauge = Gauge::new(
        "upnp_wan_scrape_error",
        "Indicates if there was an error scraping UPnP metrics (1 = error, 0 = success)"
    ).expect("metric can be created");
}

pub struct MetricsCollector;

impl MetricsCollector {
    pub async fn collect_metrics() -> (String, bool) {
        // Try to get fresh metrics
        let mut client = UpnpClient::new();
        let mut has_error = false;
        
        match client.discover_device().await {
            Ok(()) => {
                match client.get_traffic_stats().await {
                    Ok(stats) => {
                        Self::update_metrics(&stats);
                        info!("Updated metrics: bytes_sent={}, bytes_received={}, packets_sent={}, packets_received={}, connection={}", 
                              stats.bytes_sent, stats.bytes_received, stats.packets_sent, stats.packets_received, stats.connection_status);
                    }
                    Err(e) => {
                        error!("Failed to get traffic stats: {}", e);
                        has_error = true;
                        CONNECTION_STATUS.set(0.0);
                    }
                }
            }
            Err(e) => {
                error!("Failed to discover UPnP device: {}", e);
                has_error = true;
                CONNECTION_STATUS.set(0.0);
            }
        }
        
        // Set error metric
        SCRAPE_ERROR.set(if has_error { 1.0 } else { 0.0 });

        // Encode metrics in Prometheus format
        let encoder = TextEncoder::new();
        let metric_families = REGISTRY.gather();
        
        match encoder.encode_to_string(&metric_families) {
            Ok(output) => (output, false),
            Err(e) => {
                error!("Failed to encode metrics: {}", e);
                ("Internal Server Error".to_string(), true)
            }
        }
    }

    fn update_metrics(stats: &TrafficStats) {
        BYTES_SENT.set(stats.bytes_sent as f64);
        BYTES_RECEIVED.set(stats.bytes_received as f64);
        PACKETS_SENT.set(stats.packets_sent as f64);
        PACKETS_RECEIVED.set(stats.packets_received as f64);
        CONNECTION_STATUS.set(if stats.connection_status == "Up" { 1.0 } else { 0.0 });
    }

    pub async fn get_stats() -> Result<TrafficStats, String> {
        let mut client = UpnpClient::new();
        
        match client.discover_device().await {
            Ok(()) => {
                match client.get_traffic_stats().await {
                    Ok(stats) => Ok(stats),
                    Err(e) => {
                        error!("Failed to get stats: {}", e);
                        Err(format!("Error: {}", e))
                    }
                }
            }
            Err(e) => {
                error!("Failed to discover device: {}", e);
                Err(format!("Device discovery failed: {}", e))
            }
        }
    }
}

pub fn init_metrics() {
    REGISTRY.register(Box::new(BYTES_SENT.clone())).expect("collector can be registered");
    REGISTRY.register(Box::new(BYTES_RECEIVED.clone())).expect("collector can be registered");
    REGISTRY.register(Box::new(PACKETS_SENT.clone())).expect("collector can be registered");
    REGISTRY.register(Box::new(PACKETS_RECEIVED.clone())).expect("collector can be registered");
    REGISTRY.register(Box::new(CONNECTION_STATUS.clone())).expect("collector can be registered");
    REGISTRY.register(Box::new(SCRAPE_ERROR.clone())).expect("collector can be registered");
}