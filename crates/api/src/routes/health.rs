use axum::routing::get;
use axum::Router;
use serde_json::json;

use crate::AppState;

pub fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}

async fn health_check() -> axum::Json<serde_json::Value> {
    axum::Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

