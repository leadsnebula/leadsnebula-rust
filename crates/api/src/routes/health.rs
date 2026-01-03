use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::json;

use crate::AppState;

pub fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(health_check))
}

async fn health_check(State(state): State<AppState>) -> Response {
    let mut status = "healthy".to_string();
    let mut checks = json!({
        "database": "ok",
        "redis": "ok",
    });

    // Check database connection
    if let Err(e) = sqlx::query("SELECT 1").execute(&*state.db_pool).await {
        status = "unhealthy".to_string();
        checks["database"] = json!(format!("error: {}", e));
    }

    // Check Redis connection if available
    if let Some(redis) = &state.redis {
        match redis.ping().await {
            Ok(_) => {
                checks["redis"] = json!("ok");
            }
            Err(e) => {
                status = "unhealthy".to_string();
                checks["redis"] = json!(format!("error: {}", e));
            }
        }
    } else {
        // Redis is optional, so mark as "not configured" rather than unhealthy
        checks["redis"] = json!("not configured");
    }

    let status_code = if status == "healthy" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    let body = json!({
        "status": status,
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "checks": checks,
    });

    (status_code, axum::Json(body)).into_response()
}
