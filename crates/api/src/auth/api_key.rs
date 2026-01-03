use axum::{
    extract::Request,
    http::{header::HeaderName, StatusCode},
    response::{IntoResponse, Response},
};
use leadsnebula_models::publisher::Publisher;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

/// Authenticated publisher context
#[derive(Clone)]
pub struct AuthenticatedPublisher {
    pub publisher_id: Uuid,
    pub instance_id: Uuid,
    pub is_documentation_test: bool,
}

/// API key authentication error
#[derive(Debug)]
pub enum ApiKeyError {
    #[allow(dead_code)] // Prepared for future use
    MissingKey,
    InvalidKey,
    PublisherInactive,
}

impl IntoResponse for ApiKeyError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiKeyError::MissingKey => (
                StatusCode::UNAUTHORIZED,
                "Missing API key. Include X-API-Key header.",
            ),
            ApiKeyError::InvalidKey => (StatusCode::UNAUTHORIZED, "Invalid API key."),
            ApiKeyError::PublisherInactive => (
                StatusCode::UNAUTHORIZED,
                "API key is inactive. Contact support.",
            ),
        };

        let body = serde_json::json!({
            "success": false,
            "error": message,
            "status_code": status.as_u16()
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Extract API key from request headers
pub fn extract_api_key(request: &Request) -> Option<String> {
    request
        .headers()
        .get(HeaderName::from_static("x-api-key"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim().to_string())
}

/// Verify API key and load publisher
pub async fn verify_api_key_and_load_publisher(
    api_key: &str,
    pool: &PgPool,
) -> Result<AuthenticatedPublisher, ApiKeyError> {
    // Hash the API key (SHA-256)
    let key_hash = format!("{:x}", Sha256::digest(api_key.trim().as_bytes()));

    // Find publisher by API key hash
    let publisher = sqlx::query_as::<_, Publisher>(
        r#"
        SELECT 
            id, instance_id, name, api_key_hash, api_key_prefix,
            status, is_documentation_test, created_at, updated_at
        FROM publishers
        WHERE api_key_hash = $1 AND deleted_at IS NULL
        LIMIT 1
        "#,
    )
    .bind(&key_hash)
    .fetch_optional(pool)
    .await
    .map_err(|e| {
        warn!("Database error loading publisher: {}", e);
        ApiKeyError::InvalidKey
    })?
    .ok_or(ApiKeyError::InvalidKey)?;

    // Check if publisher is active
    if publisher.status != "active" {
        return Err(ApiKeyError::PublisherInactive);
    }

    // Record request (non-blocking, don't fail if it errors)
    let _ = sqlx::query(
        r#"
        UPDATE publishers
        SET last_request_at = NOW(), total_requests = total_requests + 1
        WHERE id = $1
        "#,
    )
    .bind(publisher.id)
    .execute(pool)
    .await;

    debug!("API key authenticated for publisher: {}", publisher.id);

    Ok(AuthenticatedPublisher {
        publisher_id: publisher.id,
        instance_id: publisher.instance_id,
        is_documentation_test: publisher.is_documentation_test,
    })
}

/// Middleware to authenticate requests via API key
pub async fn api_key_auth_middleware(
    axum::extract::State(pool): axum::extract::State<Arc<PgPool>>,
    mut request: Request,
    next: axum::middleware::Next,
) -> Response {
    // Extract API key
    let api_key = match extract_api_key(&request) {
        Some(k) => k,
        None => {
            // No API key - allow request to proceed (some endpoints don't require API key)
            return next.run(request).await;
        }
    };

    // Verify API key and load publisher
    match verify_api_key_and_load_publisher(&api_key, &pool).await {
        Ok(publisher) => {
            // Attach publisher to request extensions
            request.extensions_mut().insert(publisher);
            next.run(request).await
        }
        Err(e) => {
            debug!("API key authentication failed: {:?}", e);
            e.into_response()
        }
    }
}

/// Extract authenticated publisher from request
#[allow(dead_code)] // Prepared for future use
pub fn extract_publisher(request: &Request) -> Option<AuthenticatedPublisher> {
    request
        .extensions()
        .get::<AuthenticatedPublisher>()
        .cloned()
}

/// Require API key authentication - returns error if publisher is not authenticated
#[allow(dead_code)] // Prepared for future use
pub fn require_api_key(request: &Request) -> Result<AuthenticatedPublisher, ApiKeyError> {
    extract_publisher(request).ok_or(ApiKeyError::MissingKey)
}
