// Internal buyer service for direct Pulsar calls (skips HTTP overhead)
// This service allows calling Pulsar qualification logic directly without HTTP round-trip
//
// NOTE: Currently, the Pulsar handlers are in the API crate (pulsar.rs) and use AppState.
// For full implementation, either:
// 1. Move Pulsar handler logic to a shared service in core crate
// 2. Refactor handlers to accept dependencies instead of AppState
// 3. Create a direct call interface that bypasses HTTP
//
// For now, this is a placeholder that can be expanded when the architecture is refactored.
// The HTTP optimization (HTTP/2, connection pooling) already provides significant benefits.

use crate::models::campaign::Campaign;
use crate::models::lead::Lead;
use crate::services::buyer_router::BuyerResponse;
use anyhow::Result;
use sqlx::PgPool;
use std::sync::Arc;

pub struct InternalBuyerService;

impl InternalBuyerService {
    /// Direct call to Pulsar for ping requests (bypasses HTTP)
    ///
    /// This method would call the Pulsar qualification engine directly
    /// instead of making an HTTP request. For CPU-heavy qualification work,
    /// use `tokio::task::spawn_blocking` to avoid blocking the async executor.
    pub async fn route_ping_direct(
        _pool: Arc<PgPool>,
        _lead: &Lead,
        _campaign: &Campaign,
    ) -> Result<BuyerResponse> {
        // TODO: Implement direct Pulsar qualification call
        // This would:
        // 1. Load qualification config (already preloaded in ping_tree_router)
        // 2. Run qualification engine (CPU-heavy, use spawn_blocking)
        // 3. Generate ping_id and promise_id
        // 4. Return BuyerResponse directly

        // For now, return an error indicating this needs to be implemented
        Err(anyhow::anyhow!(
            "Direct internal buyer calls not yet implemented. Falling back to HTTP."
        ))
    }

    /// Direct call to Pulsar for post requests (bypasses HTTP)
    pub async fn route_post_direct(
        _pool: Arc<PgPool>,
        _lead: &Lead,
        _campaign: &Campaign,
    ) -> Result<BuyerResponse> {
        // TODO: Implement direct Pulsar post call
        // Similar to route_ping_direct but for post requests

        Err(anyhow::anyhow!(
            "Direct internal buyer calls not yet implemented. Falling back to HTTP."
        ))
    }
}
