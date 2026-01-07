// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use leadsnebula_core::models::publisher::Publisher;
use tracing::warn;

pub async fn hmac_verification_middleware(
    State(_state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Get publisher from request extensions (set by api_key_auth_middleware)
    let publisher = request
        .extensions()
        .get::<Publisher>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check if HMAC is required
    if publisher.require_hmac() {
        let hmac_header = headers
            .get("X-HMAC-Signature")
            .and_then(|h| h.to_str().ok());

        if hmac_header.is_none() {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // If HMAC header is present, verify it
    if let Some(hmac_header) = headers
        .get("X-HMAC-Signature")
        .and_then(|h| h.to_str().ok())
    {
        // Get request body
        let (parts, body) = request.into_parts();
        let body_bytes = axum::body::to_bytes(body, usize::MAX)
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;

        // Get HMAC secret from publisher (for now, use shared secret from env)
        // TODO: Support per-publisher HMAC secrets
        let hmac_secret = std::env::var("HMAC_SECRET")
            .or_else(|_| std::env::var("CARINA_HMAC_SECRET"))
            .map_err(|_| {
                warn!("HMAC header present but secret not configured");
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        // Parse signature (support "sha256=<hex>" or just "<hex>")
        let provided_signature = hmac_header
            .strip_prefix("sha256=")
            .unwrap_or(hmac_header)
            .trim();

        // Compute expected signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(hmac_secret.as_bytes())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        mac.update(&body_bytes);
        let expected_signature = hex::encode(mac.finalize().into_bytes());

        // Constant-time comparison
        if expected_signature.len() != provided_signature.len() {
            return Err(StatusCode::UNAUTHORIZED);
        }

        let mut equal = 0u8;
        for (a, b) in expected_signature
            .as_bytes()
            .iter()
            .zip(provided_signature.as_bytes())
        {
            equal |= a ^ b;
        }

        if equal != 0 {
            return Err(StatusCode::UNAUTHORIZED);
        }

        // Reconstruct request with body
        let body = Body::from(body_bytes);
        let request = Request::from_parts(parts, body);
        return Ok(next.run(request).await);
    }

    Ok(next.run(request).await)
}
