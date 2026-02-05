// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use leadsnebula_core::models::publisher::Publisher;
use serde::Serialize;

#[derive(Serialize)]
struct ApiErrorBody {
    status: ApiErrorStatus,
}

#[derive(Serialize)]
struct ApiErrorStatus {
    success: bool,
    error: String,
}

fn unauthorized_json(message: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiErrorBody {
            status: ApiErrorStatus {
                success: false,
                error: message.to_string(),
            },
        }),
    )
        .into_response()
}

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
            return unauthorized_json("Missing X-API-Key header");
        }
    };

    // Note: test_before_acquire is enabled in pool config to detect stale connections
    // This should prevent most "expected to read X bytes, got 0" errors from Neon
    // CACHE: Publisher lookup by API key (1h TTL - publishers rarely change)
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(api_key.trim().as_bytes());
    let key_hash = hex::encode(hasher.finalize());
    let cache_key = format!("publisher:api_key:{}", key_hash);

    let publisher = if let Some(cache) = &state.cache {
        match cache
            .get_or_insert_with(&cache_key, 3600, || async {
                Publisher::find_by_api_key(&state.db_pool, &api_key)
                    .await
                    .map_err(|e| anyhow::anyhow!("Database error: {}", e))
            })
            .await
        {
            Ok(Some(p)) => p,
            Ok(None) => {
                tracing::warn!("Invalid API key provided");
                return unauthorized_json("Invalid or unknown API key");
            }
            Err(e) => {
                tracing::error!(
                    "Database error during API key lookup (after retries): {}",
                    e
                );
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        // Fallback if cache not available
        match Publisher::find_by_api_key(&state.db_pool, &api_key).await {
            Ok(Some(p)) => p,
            Ok(None) => {
                tracing::warn!("Invalid API key provided");
                return unauthorized_json("Invalid or unknown API key");
            }
            Err(e) => {
                tracing::error!(
                    "Database error during API key lookup (after retries): {}",
                    e
                );
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };

    if !publisher.active() {
        tracing::warn!(
            "Publisher {} is not active (status: {})",
            publisher.id,
            publisher.status
        );
        return unauthorized_json("Publisher account is not active");
    }

    // Record request (non-blocking, ignore errors)
    let _ = publisher.record_request(&state.db_pool).await;

    // Attach publisher to request extensions
    request.extensions_mut().insert(publisher);
    next.run(request).await
}
