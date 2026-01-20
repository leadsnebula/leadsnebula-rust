// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{header::HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::auth::JwtService;
use leadsnebula_core::models::user::User;
use uuid::Uuid;

pub async fn jwt_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Response {
    // Extract token from Authorization header
    let token = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    let token = match token {
        Some(t) => t,
        None => {
            tracing::warn!("Missing Authorization header");
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Decode JWT
    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = match jwt_service.decode(&token) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("JWT decode error: {}", e);
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    // Load user from database
    // Note: instance_users table uses 'status' column, not 'deleted_at'
    // Filter by status = 'active' instead of deleted_at IS NULL
    let user_id = match Uuid::parse_str(&claims.user_id) {
        Ok(id) => id,
        Err(_) => {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    };

    let user = match sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(user_id)
    .fetch_optional(state.db_pool.as_ref())
    .await
    {
        Ok(Some(u)) => u,
        Ok(None) => {
            return StatusCode::UNAUTHORIZED.into_response();
        }
        Err(e) => {
            tracing::error!("Database error during user lookup: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Check if user is active
    if !user.is_active() {
        tracing::warn!("User {} is not active", user.id);
        return StatusCode::UNAUTHORIZED.into_response();
    }

    // Attach user to request extensions
    request.extensions_mut().insert(user);

    next.run(request).await
}
