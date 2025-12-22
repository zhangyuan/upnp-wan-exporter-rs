use crate::metrics::MetricsCollector;
use axum::{
    Router,
    extract::Query,
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_index = 0;
    
    while value >= 1024.0 && unit_index < UNITS.len() - 1 {
        value /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", bytes, UNITS[unit_index])
    } else {
        format!("{:.2} {}", value, UNITS[unit_index])
    }
}

pub fn create_app() -> Router {
    Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .route("/stats", get(stats_handler))
}

async fn metrics_handler() -> Response {
    let (output, has_error) = MetricsCollector::collect_metrics().await;

    if has_error {
        axum::response::Response::builder()
            .status(500)
            .body(output.into())
            .unwrap()
    } else {
        axum::response::Response::builder()
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(output.into())
            .unwrap()
    }
}

async fn health_handler() -> impl IntoResponse {
    "OK"
}

#[derive(Deserialize)]
struct StatsQuery {
    format: Option<String>,
}

async fn stats_handler(Query(params): Query<StatsQuery>) -> Response {
    match MetricsCollector::get_stats().await {
        Ok(stats) => match params.format.as_deref() {
            Some("json") => axum::response::Json(stats).into_response(),
            _ => {
                let output = format!(
                    "Bytes Sent: {} / {}\nBytes Received: {} / {}\nPackets Sent: {}\nPackets Received: {}\nConnection: {}",
                    stats.bytes_sent,
                    format_bytes(stats.bytes_sent),
                    stats.bytes_received,
                    format_bytes(stats.bytes_received),
                    stats.packets_sent,
                    stats.packets_received,
                    stats.connection_status
                );

                axum::response::Response::builder()
                    .header("Content-Type", "text/plain")
                    .body(output.into())
                    .unwrap()
            }
        },
        Err(error_msg) => axum::response::Response::builder()
            .status(500)
            .body(error_msg.into())
            .unwrap(),
    }
}
