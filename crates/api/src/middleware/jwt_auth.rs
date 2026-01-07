// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{header::HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use leadsnebula_core::auth::JwtService;
use leadsnebula_core::models::user::User;
use uuid::Uuid;

pub async fn jwt_auth_middleware(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
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
            return Err(StatusCode::UNAUTHORIZED);
        }
    };

    // Decode JWT
    let jwt_service = JwtService::new(state.config.jwt_secret.clone());
    let claims = jwt_service.decode(&token).map_err(|e| {
        tracing::warn!("JWT decode error: {}", e);
        StatusCode::UNAUTHORIZED
    })?;

    // Load user from database
    // Note: instance_users table uses 'status' column, not 'deleted_at'
    // Filter by status = 'active' instead of deleted_at IS NULL
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM instance_users WHERE id = $1 AND status = 'active'",
    )
    .bind(Uuid::parse_str(&claims.user_id).map_err(|_| StatusCode::UNAUTHORIZED)?)
    .fetch_optional(state.db_pool.as_ref())
    .await
    .map_err(|e| {
        tracing::error!("Database error during user lookup: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Check if user is active
    if !user.is_active() {
        tracing::warn!("User {} is not active", user.id);
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Attach user to request extensions
    request.extensions_mut().insert(user);

    Ok(next.run(request).await)
}
