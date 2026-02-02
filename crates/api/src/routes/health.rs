// Note: health_routes() and health_check() are currently unused because we only serve minimal app
// They will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

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
        .route("/poke", get(poke_endpoint))
        .route("/metrics", get(metrics_endpoint))
}

/// Lightweight DB warmup endpoint for form flows (e.g. only.solar).
/// Runs SELECT 1 to wake Neon/connection pool; does not check Redis.
/// Use /health for full health checks; use /poke to avoid overloading /health.
async fn poke_endpoint(State(state): State<AppState>) -> Response {
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        sqlx::query("SELECT 1").execute(&*state.db_pool),
    )
    .await
    {
        Ok(Ok(_)) => (StatusCode::OK, axum::Json(json!({ "ok": true }))).into_response(),
        _ => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(json!({ "ok": false })),
        )
            .into_response(),
    }
}

// Export liveness_check for use in main.rs when AppState is unavailable
// Simple liveness check - just confirms the app is running
// This endpoint doesn't require AppState, so it works even if app initialization fails
pub async fn liveness_check() -> Response {
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

/// Metrics endpoint for production monitoring (Phase 7.3)
/// Returns performance metrics in JSON format suitable for Prometheus/Grafana
async fn metrics_endpoint(State(state): State<AppState>) -> Response {
    // Get database pool metrics
    let pool_size = state.db_pool.size() as usize;
    let num_idle = state.db_pool.num_idle();

    // Get Redis metrics if available
    let redis_configured = state.redis.is_some();
    let redis_status = if let Some(redis) = &state.redis {
        match redis.ping().await {
            Ok(_) => "connected",
            Err(_) => "disconnected",
        }
    } else {
        "not_configured"
    };

    let body = json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "metrics": {
            "database": {
                "pool_size": pool_size,
                "idle_connections": num_idle,
                "active_connections": pool_size.saturating_sub(num_idle),
            },
            "cache": {
                "configured": redis_configured,
                "status": redis_status,
            },
            "performance": {
                "note": "Request-level metrics (ping_auction_time, post_time, cache_hit_rate) are tracked per-request via DiagnosticMetrics. Aggregate metrics can be collected from logs or by storing DiagnosticMetrics in AppState."
            }
        },
        "format": "json",
        "version": "1.0",
        "monitoring": {
            "health_endpoint": "/health",
            "metrics_endpoint": "/metrics",
            "alerting_thresholds": {
                "auction_time_ms": 200,
                "cache_hit_rate_percent": 90.0,
                "db_query_time_ms": 100
            },
            "alerts": {
                "auction_time_high": pool_size > 0 && num_idle < (pool_size / 10), // Less than 10% idle = potential issue
                "cache_miss_rate_high": !redis_configured || redis_status != "connected",
                "db_pool_exhausted": num_idle == 0 && pool_size > 0
            }
        }
    });

    (StatusCode::OK, axum::Json(body)).into_response()
}
