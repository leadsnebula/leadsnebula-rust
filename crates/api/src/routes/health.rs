use axum::{http::StatusCode, response::Json};
use chrono::Utc;
use serde_json::json;

pub async fn health_check() -> (StatusCode, Json<serde_json::Value>) {
    let response = json!({
        "status": "healthy",
        "timestamp": Utc::now().to_rfc3339(),
        "service": "leadsnebula-api",
        "version": env!("CARGO_PKG_VERSION"),
    });

    (StatusCode::OK, Json(response))
}
