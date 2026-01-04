use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::json;

use crate::AppState;

pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health", get(health_check))
        .route("/live", get(liveness_check))
}

// Simple liveness check - just confirms the app is running
// This endpoint doesn't require AppState, so it works even if app initialization fails
async fn liveness_check() -> Response {
    let body = json!({
        "status": "alive",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    (StatusCode::OK, axum::Json(body)).into_response()
}

async fn health_check(State(state): State<AppState>) -> Response {
    let mut status = "healthy".to_string();
    let mut checks = json!({
        "database": "ok",
        "redis": "ok",
    });

    // Check database connection with timeout
    match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sqlx::query("SELECT 1").execute(&*state.db_pool),
    )
    .await
    {
        Ok(Ok(_)) => {
            checks["database"] = json!("ok");
        }
        Ok(Err(e)) => {
            status = "unhealthy".to_string();
            checks["database"] = json!(format!("error: {}", e));
        }
        Err(_) => {
            status = "unhealthy".to_string();
            checks["database"] = json!("error: connection timeout");
        }
    }

    // Check Redis connection if available (Redis failures don't make app unhealthy)
    if let Some(redis) = &state.redis {
        match tokio::time::timeout(std::time::Duration::from_secs(2), redis.ping()).await {
            Ok(Ok(_)) => {
                checks["redis"] = json!("ok");
            }
            Ok(Err(e)) => {
                // Redis is optional, so don't mark as unhealthy
                checks["redis"] = json!(format!("warning: {}", e));
            }
            Err(_) => {
                checks["redis"] = json!("warning: connection timeout");
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
