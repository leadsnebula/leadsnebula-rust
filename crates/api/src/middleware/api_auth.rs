use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use leadsnebula_core::models::publisher::Publisher;

pub async fn api_key_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_key = headers
        .get("X-API-Key")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let api_key = match api_key {
        Some(key) => key,
        None => {
            tracing::warn!("Missing X-API-Key header");
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    let publisher = match Publisher::find_by_api_key(&state.db_pool, &api_key).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            tracing::warn!("Invalid API key provided");
            return Err(StatusCode::UNAUTHORIZED);
        }
        Err(e) => {
            tracing::error!("Database error during API key lookup: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    if !publisher.active() {
        tracing::warn!(
            "Publisher {} is not active (status: {})",
            publisher.id,
            publisher.status
        );
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Record request (non-blocking, ignore errors)
    let _ = publisher.record_request(&state.db_pool).await;

    // Attach publisher to request extensions
    request.extensions_mut().insert(publisher);

    Ok(next.run(request).await)
}
