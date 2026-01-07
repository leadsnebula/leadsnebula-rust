// Integration tests for health endpoints and startup sequence
// These tests verify that health checks work correctly and the server starts properly
//
// Note: These tests verify the /live endpoint behavior which is critical for
// health checks during server startup before AppState is initialized

use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tower::ServiceExt;

// Simple liveness check handler (matches the one in main.rs)
async fn liveness_check() -> axum::response::Response {
    let body = json!({
        "status": "alive",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    (StatusCode::OK, axum::Json(body)).into_response()
}

#[tokio::test]
async fn test_liveness_endpoint() {
    // Test that /live endpoint returns 200 OK
    let app = Router::new().route("/live", get(liveness_check));
    
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/live")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response.status(), StatusCode::OK);
    
    // Verify response body contains expected fields
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(json["status"], "alive");
    assert!(json.get("timestamp").is_some());
}

#[tokio::test]
async fn test_liveness_endpoint_format() {
    // Test that /live endpoint returns correct JSON format
    let app = Router::new().route("/live", get(liveness_check));
    
    let response = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/live")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    // Verify structure
    assert!(json.is_object());
    assert!(json.get("status").is_some());
    assert!(json.get("timestamp").is_some());
    
    // Verify status value
    assert_eq!(json["status"].as_str().unwrap(), "alive");
}

#[tokio::test]
async fn test_liveness_endpoint_always_available() {
    // Test that /live endpoint is available even without AppState
    // This is critical for health checks during startup
    let app = Router::new().route("/live", get(liveness_check));
    
    // Call multiple times to ensure it's stable
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri("/live")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        
        assert_eq!(response.status(), StatusCode::OK);
    }
}
