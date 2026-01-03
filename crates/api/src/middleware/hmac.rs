use axum::{
    extract::{Request, State},
    http::{header::HeaderName, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::HmacVerifier;
use leadsnebula_models::publisher::Publisher;
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::auth::api_key::AuthenticatedPublisher;

/// HMAC verification state
#[allow(dead_code)] // Prepared for future HMAC verification
#[derive(Clone)]
pub struct HmacState {
    pub pool: Arc<PgPool>,
    pub shared_secret: Option<String>,
}

/// HMAC verification error
#[allow(dead_code)] // Prepared for future HMAC verification
#[derive(Debug)]
pub enum HmacError {
    MissingHeader,
    MissingSecret,
    InvalidSignature,
    BodyReadError,
}

impl IntoResponse for HmacError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            HmacError::MissingHeader => {
                (StatusCode::UNAUTHORIZED, "HMAC signature is required for this publisher. Include X-HMAC-Signature header.")
            }
            HmacError::MissingSecret => {
                (StatusCode::UNAUTHORIZED, "HMAC verification is required but server configuration is incomplete. Contact support.")
            }
            HmacError::InvalidSignature => {
                (StatusCode::UNAUTHORIZED, "Invalid HMAC signature. Request may have been tampered with, or the HMAC secret may not match.")
            }
            HmacError::BodyReadError => {
                (StatusCode::BAD_REQUEST, "Unable to verify HMAC signature. Request body could not be read.")
            }
        };

        let body = serde_json::json!({
            "success": false,
            "error": message,
            "status_code": status.as_u16(),
            "authentication": {
                "method": "HMAC",
                "header": "X-HMAC-Signature",
                "algorithm": "SHA256",
                "verification_status": "failed"
            }
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Extract HMAC signature from request headers
#[allow(dead_code)] // Prepared for future HMAC verification
pub fn extract_hmac_signature(request: &Request) -> Option<String> {
    request
        .headers()
        .get(HeaderName::from_static("x-hmac-signature"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string())
}

/// Get HMAC secret for publisher (per-publisher or shared)
#[allow(dead_code)] // Prepared for future HMAC verification
async fn get_hmac_secret(
    pool: &PgPool,
    publisher_id: Option<Uuid>,
    shared_secret: &Option<String>,
) -> Option<String> {
    // Try per-publisher secret first
    if let Some(pub_id) = publisher_id {
        let _publisher: Option<Publisher> = sqlx::query_as(
            r#"
            SELECT 
                id, instance_id, name, api_key_hash, api_key_prefix,
                status, is_documentation_test, created_at, updated_at
            FROM publishers
            WHERE id = $1
            LIMIT 1
            "#,
        )
        .bind(pub_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        // TODO: Check for per-publisher HMAC secret (encrypted field)
        // For now, fall back to shared secret
    }

    // Fall back to shared secret
    shared_secret.clone()
}

/// Check if publisher requires HMAC
#[allow(dead_code)] // Prepared for future HMAC verification
async fn publisher_requires_hmac(pool: &PgPool, publisher_id: Uuid) -> bool {
    let result: Option<bool> = sqlx::query_scalar(
        r#"
        SELECT hmac_required
        FROM publishers
        WHERE id = $1
        LIMIT 1
        "#,
    )
    .bind(publisher_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    result.unwrap_or(false)
}

/// Middleware to verify HMAC signature for lead requests
/// Runs after API key authentication
#[allow(dead_code)] // Prepared for future HMAC verification
pub async fn hmac_verification_middleware(
    State(state): State<HmacState>,
    request: Request,
    next: Next,
) -> Response {
    // Extract authenticated publisher from request extensions
    let publisher = request
        .extensions()
        .get::<AuthenticatedPublisher>()
        .cloned();

    // If publisher requires HMAC, header must be present
    if let Some(pub_ctx) = &publisher {
        let requires_hmac = publisher_requires_hmac(&state.pool, pub_ctx.publisher_id).await;

        if requires_hmac {
            if extract_hmac_signature(&request).is_none() {
                warn!(
                    "HMAC required for publisher {} but header missing",
                    pub_ctx.publisher_id
                );
                return HmacError::MissingHeader.into_response();
            }
        }
    }

    // If HMAC header is present, verify it
    let request = if let Some(hmac_header) = extract_hmac_signature(&request) {
        // Get HMAC secret
        let publisher_id = publisher.as_ref().map(|p| p.publisher_id);
        let secret = match get_hmac_secret(&state.pool, publisher_id, &state.shared_secret).await {
            Some(s) => s,
            None => {
                warn!("HMAC header present but secret not configured");
                return HmacError::MissingSecret.into_response();
            }
        };

        // Read request body
        let (parts, body) = request.into_parts();
        let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!("Failed to read request body for HMAC verification: {}", e);
                return HmacError::BodyReadError.into_response();
            }
        };

        // Parse signature from header
        let provided_signature = HmacVerifier::parse_signature(&hmac_header);

        // Verify signature
        if !HmacVerifier::verify_signature(&body_bytes, &secret, &provided_signature) {
            warn!("HMAC signature verification failed");
            return HmacError::InvalidSignature.into_response();
        }

        debug!("HMAC signature verified successfully");

        // Reconstruct request with body for downstream handlers
        let body = axum::body::Body::from(body_bytes);
        Request::from_parts(parts, body)
    } else {
        request
    };

    // Continue to next middleware/handler
    next.run(request).await
}
