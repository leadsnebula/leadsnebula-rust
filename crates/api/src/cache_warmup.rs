use crate::config::AppState;
use leadsnebula_core::models::ping_tree::PingTree;
use leadsnebula_core::models::vertical::Vertical;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::{info, warn};

/// Pre-warm cache on startup by loading common data
/// This reduces cold start latency for the first few requests
pub async fn pre_warm_cache(state: &AppState) {
    info!("Starting cache pre-warming...");
    let start = std::time::Instant::now();

    if state.cache.is_none() {
        warn!("Cache not available, skipping pre-warm");
        return;
    }

    let cache = state.cache.as_ref().unwrap();
    let pool = &state.db_pool;

    // Pre-warm verticals (most commonly used)
    let verticals_warmed = pre_warm_verticals(cache, pool).await;

    // Pre-warm active ping trees (limit to top 100 to avoid excessive DB load)
    let ping_trees_warmed = pre_warm_ping_trees(cache, pool).await;

    // Pre-warm SSM encryption keys (eliminates ~300ms delays on first request)
    let ssm_keys_warmed = pre_warm_ssm_keys(&state.ssm, &state.config.environment).await;

    let duration_ms = start.elapsed().as_millis();
    info!(
        "Cache pre-warming completed in {}ms: {} verticals, {} ping trees, {} SSM keys",
        duration_ms, verticals_warmed, ping_trees_warmed, ssm_keys_warmed
    );
}

/// Pre-warm verticals cache
async fn pre_warm_verticals(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Load all active verticals
    match sqlx::query_as::<_, Vertical>(
        "SELECT * FROM verticals WHERE is_active = true ORDER BY created_at DESC LIMIT 50",
    )
    .fetch_all(pool)
    .await
    {
        Ok(verticals) => {
            for vertical in verticals {
                let cache_key = format!("vertical:slug:{}", vertical.slug);
                // Trigger cache by calling get_or_insert_with
                let _ = cache
                    .get_or_insert_with(&cache_key, 86400, || async {
                        Ok::<Vertical, anyhow::Error>(vertical.clone())
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm verticals: {}", e);
        }
    }

    warmed
}

/// Pre-warm ping trees cache (top 100 active ping trees)
async fn pre_warm_ping_trees(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Load active ping trees with their publishers
    // Note: pt.created_at must be in SELECT list when using DISTINCT with ORDER BY
    match sqlx::query(
        r#"
        SELECT DISTINCT pt.id, pt.vertical, ptp.publisher_id, pt.created_at
        FROM ping_trees pt
        INNER JOIN ping_tree_publishers ptp ON pt.id = ptp.ping_tree_id
        WHERE pt.deleted_at IS NULL
          AND pt.status = 'active'
        ORDER BY pt.created_at DESC
        LIMIT 100
        "#,
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let vertical: String = row.get("vertical");
                let publisher_id: uuid::Uuid = row.get("publisher_id");

                let cache_key = format!("pingtree:pub:{}:vert:{}", publisher_id, vertical);
                // Trigger cache by calling get_or_insert_with
                let _ = cache
                    .get_or_insert_with(&cache_key, 21600, || async {
                        PingTree::find_for_routing(pool, &publisher_id, &vertical)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm ping trees: {}", e);
        }
    }

    warmed
}

/// Pre-warm SSM encryption keys
pub async fn pre_warm_ssm_keys(
    ssm: &std::sync::Arc<leadsnebula_core::ssm::SsmService>,
    environment: &str,
) -> usize {
    let mut warmed = 0;

    // Normalize environment name for SSM paths (same as carina.rs)
    let env_norm = leadsnebula_core::normalize_env_for_ssm(environment).to_string();

    // Pre-warm deterministic key
    let det_path = format!(
        "/leadsnebula/{}/carina/encryption/deterministic_key_v1",
        env_norm
    );
    let det_key_cached = match ssm.get_parameter(&det_path, true).await {
        Ok(Some(_)) => {
            warmed += 1;
            true
        }
        Ok(None) => {
            warn!(
                "SSM parameter not found (expected for warmup): {}",
                det_path
            );
            false
        }
        Err(e) => {
            warn!("Failed to pre-warm SSM key {}: {}", det_path, e);
            false
        }
    };

    // Pre-warm key derivation salt
    let salt_path = format!(
        "/leadsnebula/{}/carina/encryption/key_derivation_salt_v1",
        env_norm
    );
    let salt_key_cached = match ssm.get_parameter(&salt_path, true).await {
        Ok(Some(_)) => {
            warmed += 1;
            true
        }
        Ok(None) => {
            warn!(
                "SSM parameter not found (expected for warmup): {}",
                salt_path
            );
            false
        }
        Err(e) => {
            warn!("Failed to pre-warm SSM key {}: {}", salt_path, e);
            false
        }
    };

    // Log cache status after pre-warm
    info!(
        "SSM pre-warm cache keys: det_key={}, salt={}",
        if det_key_cached { "hit" } else { "miss" },
        if salt_key_cached { "hit" } else { "miss" }
    );

    warmed
}

/// Periodic cache warm-up task (runs every 30 minutes)
pub async fn start_periodic_warmup(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1800)); // 30 minutes

    loop {
        interval.tick().await;
        info!("Running periodic cache warm-up...");
        pre_warm_cache(&state).await;
    }
}
