use axum::{extract::Request, middleware::Next, response::Response};
use leadsnebula_core::RlsContext;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::debug;

use crate::auth::api_key::AuthenticatedPublisher;
use crate::auth::AuthenticatedUser;

/// RLS context state - holds database pool
#[derive(Clone)]
pub struct RlsState {
    pub pool: Arc<PgPool>,
}

/// Middleware to set RLS context before request handling
/// Extracts authentication from request and sets appropriate RLS context
pub async fn rls_middleware(
    axum::extract::State(state): axum::extract::State<RlsState>,
    request: Request,
    next: Next,
) -> Response {
    // Extract authenticated user from request extensions (set by auth_middleware)
    if let Some(user) = request.extensions().get::<AuthenticatedUser>() {
        // User is authenticated - set user context
        if let Err(e) = RlsContext::set_user_context(
            &state.pool,
            user.user_id,
            user.instance_id,
            user.is_admin,
            user.publisher_id,
            user.is_documentation_test,
        )
        .await
        {
            debug!("Failed to set user RLS context: {}", e);
        }
    } else if let Some(publisher) = request.extensions().get::<AuthenticatedPublisher>() {
        // Publisher is authenticated via API key - set publisher context
        if let Err(e) = RlsContext::set_publisher_context(
            &state.pool,
            publisher.publisher_id,
            publisher.instance_id,
            publisher.is_documentation_test,
        )
        .await
        {
            debug!("Failed to set publisher RLS context: {}", e);
        }
    } else {
        // No authentication - set system context
        if let Err(e) = RlsContext::set_system_context(&state.pool).await {
            debug!("Failed to set system RLS context: {}", e);
        }
    }

    // Process request
    let response = next.run(request).await;

    // Clear RLS context after request
    if let Err(e) = RlsContext::clear_context(&state.pool).await {
        debug!("Failed to clear RLS context: {}", e);
    }

    response
}
