// Note: This middleware is currently unused because we only serve minimal app with /live endpoint
// It will be used when we implement full app serving after AppState initialization
#![allow(dead_code)]

use crate::AppState;
use axum::{
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use leadsnebula_core::models::publisher::Publisher;
use tracing::warn;

pub async fn hmac_verification_middleware(
    State(_state): State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // Allow internal requests (with X-Internal-Buyer-ID) to bypass HMAC verification
    if headers.get("X-Internal-Buyer-ID").is_some() {
        return next.run(request).await;
    }

    // Get publisher from request extensions (set by api_key_auth_middleware)
    let publisher = match request.extensions().get::<Publisher>().cloned() {
        Some(p) => p,
        None => {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // Check if HMAC is required
    if publisher.require_hmac() {
        let hmac_header = headers
            .get("X-HMAC-Signature")
            .and_then(|h| h.to_str().ok());

        if hmac_header.is_none() {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    // If HMAC header is present, verify it
    if let Some(hmac_header) = headers
        .get("X-HMAC-Signature")
        .and_then(|h| h.to_str().ok())
    {
        // Get HMAC secret from publisher (for now, use shared secret from env)
        // TODO: Support per-publisher HMAC secrets
        let hmac_secret = match std::env::var("HMAC_SECRET")
            .or_else(|_| std::env::var("CARINA_HMAC_SECRET"))
        {
            Ok(secret) => secret,
            Err(_) => {
                // If secret is not configured, only fail if publisher requires HMAC
                if publisher.require_hmac() {
                    warn!("HMAC header present and publisher requires HMAC, but secret not configured");
                    return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                } else {
                    // Publisher doesn't require HMAC, so we can skip verification
                    warn!("HMAC header present but secret not configured (publisher doesn't require HMAC, skipping verification)");
                    return next.run(request).await;
                }
            }
        };

        // Get request body (only if we have a secret to verify with)
        let (parts, body) = request.into_parts();
        let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => bytes,
            Err(_) => {
                return StatusCode::BAD_REQUEST.into_response();
            }
        };

        // Parse signature (support "sha256=<hex>" or just "<hex>")
        let provided_signature = hmac_header
            .strip_prefix("sha256=")
            .unwrap_or(hmac_header)
            .trim();

        // Compute expected signature
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = match HmacSha256::new_from_slice(hmac_secret.as_bytes()) {
            Ok(m) => m,
            Err(_) => {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        };
        mac.update(&body_bytes);
        let expected_signature = hex::encode(mac.finalize().into_bytes());

        // Constant-time comparison
        if expected_signature.len() != provided_signature.len() {
            return StatusCode::UNAUTHORIZED.into_response();
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
            return StatusCode::UNAUTHORIZED.into_response();
        }

        // Reconstruct request with body
        let body = Body::from(body_bytes);
        let request = Request::from_parts(parts, body);
        return next.run(request).await;
    }

    next.run(request).await
}
