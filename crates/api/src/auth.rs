use crate::webauthn_challenge::ChallengeStore;
use axum::{
    extract::{Request, State},
    http::{header::AUTHORIZATION, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::{JwtHelper, JwtSecret, WebauthnService};
use leadsnebula_models::instance_user::InstanceUser;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// Authentication state
#[derive(Clone)]
pub struct AuthState {
    pub pool: Arc<PgPool>,
    pub jwt_secret: Arc<JwtSecret>,
    pub webauthn: Arc<WebauthnService>,
    pub challenge_store: Arc<ChallengeStore>,
}

/// Authenticated user context
#[derive(Clone)]
pub struct AuthenticatedUser {
    pub user_id: Uuid,
    pub email: String,
    pub instance_id: Option<Uuid>,
    pub is_admin: bool,
    pub publisher_id: Option<Uuid>,
    pub is_documentation_test: bool,
}

/// Authentication error
#[derive(Debug)]
pub enum AuthError {
    #[allow(dead_code)] // Prepared for future use
    MissingToken,
    InvalidToken,
    ExpiredToken,
    UserNotFound,
    UserInactive,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthError::MissingToken => (StatusCode::UNAUTHORIZED, "Authentication required"),
            AuthError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid or expired token"),
            AuthError::ExpiredToken => (StatusCode::UNAUTHORIZED, "Token expired"),
            AuthError::UserNotFound => (StatusCode::UNAUTHORIZED, "User not found"),
            AuthError::UserInactive => (StatusCode::UNAUTHORIZED, "User account is inactive"),
        };

        let body = serde_json::json!({
            "success": false,
            "error": message,
            "status_code": status.as_u16()
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Extract JWT token from request
pub fn extract_token(request: &Request) -> Option<String> {
    // Try Authorization header first (Bearer token)
    if let Some(auth_header) = request.headers().get(AUTHORIZATION) {
        if let Ok(header_value) = auth_header.to_str() {
            if header_value.starts_with("Bearer ") {
                return Some(header_value[7..].trim().to_string());
            }
        }
    }

    // Fallback to cookie (if needed in future)
    // For now, only support Bearer token
    None
}

/// Verify JWT token and load user
pub async fn verify_token_and_load_user(
    token: &str,
    pool: &PgPool,
    jwt_secret: &JwtSecret,
) -> Result<AuthenticatedUser, AuthError> {
    // Decode token
    let claims = JwtHelper::decode(token, jwt_secret).map_err(|_| AuthError::InvalidToken)?;

    // Check expiration
    if claims.is_expired() {
        return Err(AuthError::ExpiredToken);
    }

    // Load user from database
    let user = sqlx::query_as::<_, InstanceUser>(
        r#"
        SELECT 
            id, email, encrypted_password, first_name, last_name,
            status, created_at, updated_at, last_password_change_at,
            preferred_2fa_method, passwordless_login_enabled
        FROM instance_users
        WHERE id = $1
        "#,
    )
    .bind(Uuid::parse_str(&claims.sub).map_err(|_| AuthError::UserNotFound)?)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        warn!("Database error loading user: {}", e);
        AuthError::UserNotFound
    })?
    .ok_or(AuthError::UserNotFound)?;

    // Check if user is active
    if user.status != "active" {
        return Err(AuthError::UserInactive);
    }

    // Load user's instance and role information
    let instance_role: Option<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT instance_id, role
        FROM instance_user_roles
        WHERE instance_user_id = $1
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let instance_id = instance_role.as_ref().map(|r| r.0);
    let is_admin = instance_role
        .as_ref()
        .map(|r| r.1 == "admin")
        .unwrap_or(false);

    // Check if user has a publisher
    let publisher: Option<(Uuid, Uuid, bool)> = sqlx::query_as(
        r#"
        SELECT id, instance_id, is_documentation_test
        FROM publishers
        WHERE instance_user_id = $1
        LIMIT 1
        "#,
    )
    .bind(user.id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let publisher_id = publisher.as_ref().map(|p| p.0);
    let is_documentation_test = publisher.as_ref().map(|p| p.2).unwrap_or(false);

    Ok(AuthenticatedUser {
        user_id: user.id,
        email: user.email,
        instance_id,
        is_admin,
        publisher_id,
        is_documentation_test,
    })
}

/// Middleware to authenticate requests via JWT
pub async fn auth_middleware(
    State(state): State<AuthState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Extract token
    let token = match extract_token(&request) {
        Some(t) => t,
        None => {
            // No token - allow request to proceed (some endpoints don't require auth)
            return next.run(request).await;
        }
    };

    // Verify token and load user
    match verify_token_and_load_user(&token, &state.pool, &state.jwt_secret).await {
        Ok(user) => {
            // Attach user to request extensions
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(e) => {
            debug!("Authentication failed: {:?}", e);
            e.into_response()
        }
    }
}

/// Extract authenticated user from request
#[allow(dead_code)] // Prepared for future use
pub fn extract_user(request: &Request) -> Option<AuthenticatedUser> {
    request.extensions().get::<AuthenticatedUser>().cloned()
}

/// Require authentication - returns error if user is not authenticated
#[allow(dead_code)] // Prepared for future use
pub fn require_auth(request: &Request) -> Result<AuthenticatedUser, AuthError> {
    extract_user(request).ok_or(AuthError::MissingToken)
}

// Re-export API key authentication
pub mod api_key;
// Note: These are exported for potential future use
#[allow(unused_imports)]
pub use api_key::{extract_publisher, require_api_key, AuthenticatedPublisher};
