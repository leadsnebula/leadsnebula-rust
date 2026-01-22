// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::models::publisher::Publisher;

pub async fn api_key_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    let api_key = headers
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let api_key = match api_key {
        Some(key) => key,
        None => {
            tracing::warn!("Missing X-API-Key header");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Note: test_before_acquire is enabled in pool config to detect stale connections
    // This should prevent most "expected to read X bytes, got 0" errors from Neon
    let publisher = match Publisher::find_by_api_key(&state.db_pool, &api_key).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::warn!("Invalid API key provided");
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Err(e) => {
            tracing::error!(
                "Database error during API key lookup (after retries): {}",
                e
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    if !publisher.active() {
        tracing::warn!(
            "Publisher {} is not active (status: {})",
            publisher.id,
            publisher.status
        );
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Record request (non-blocking, ignore errors)
    let _ = publisher.record_request(&state.db_pool).await;

    // Attach publisher to request extensions
    request.extensions_mut().insert(publisher);
    next.run(request).await
}
