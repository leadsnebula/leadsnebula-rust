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

    // Pre-warm buyer and campaign names (used in response messages)
    let buyers_warmed = pre_warm_buyer_names(cache, pool).await;
    let campaigns_warmed = pre_warm_campaign_names(cache, pool).await;

    // Pre-warm buyer integrations and qualification configs (used in routing)
    let integrations_warmed = pre_warm_buyer_integrations(cache, pool).await;
    let qual_configs_warmed = pre_warm_qualification_configs(cache, pool).await;

    // Pre-warm buyer IDs (used in buyer_router for buyer lookups)
    let buyer_ids_warmed = pre_warm_buyer_ids(cache, pool).await;

    let duration_ms = start.elapsed().as_millis();
    info!(
        "Cache pre-warming completed in {}ms: {} verticals, {} ping trees, {} SSM keys, {} buyers, {} campaigns, {} integrations, {} qual configs, {} buyer_ids",
        duration_ms, verticals_warmed, ping_trees_warmed, ssm_keys_warmed, buyers_warmed, campaigns_warmed, integrations_warmed, qual_configs_warmed, buyer_ids_warmed
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

/// Pre-warm buyer names (1h TTL - names rarely change)
async fn pre_warm_buyer_names(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    match sqlx::query(
        "SELECT id, name FROM buyers WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let buyer_id: uuid::Uuid = row.get("id");
                let name: String = row.get("name");
                let cache_key = format!("buyer:name:{}", buyer_id);
                let _ = cache
                    .get_or_insert_with(&cache_key, 3600, || async {
                        Ok::<String, anyhow::Error>(name.clone())
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm buyer names: {}", e);
        }
    }

    warmed
}

/// Pre-warm campaign names (1h TTL - names rarely change)
async fn pre_warm_campaign_names(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    match sqlx::query(
        "SELECT id, name FROM campaigns WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 200"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let campaign_id: uuid::Uuid = row.get("id");
                let name: String = row.get("name");
                let cache_key = format!("campaign:name:{}", campaign_id);
                let _ = cache
                    .get_or_insert_with(&cache_key, 3600, || async {
                        Ok::<String, anyhow::Error>(name.clone())
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm campaign names: {}", e);
        }
    }

    warmed
}

/// Pre-warm buyer integrations (1h TTL - integrations rarely change)
/// Note: Buyer integrations are cached by integration_id in buyer_router, not buyer_id
/// This pre-warms active buyer integrations
async fn pre_warm_buyer_integrations(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    match sqlx::query(
        "SELECT id FROM buyer_integrations WHERE status = 'available' ORDER BY created_at DESC LIMIT 200"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let integration_id: uuid::Uuid = row.get("id");
                let cache_key = format!("buyer_integration:id:{}", integration_id);
                // Trigger cache by calling get_or_insert_with (BuyerIntegration::find_by_id is already cached in buyer_router)
                let _ = cache
                    .get_or_insert_with(&cache_key, 3600, || async {
                        use leadsnebula_core::models::buyer_integration::BuyerIntegration;
                        BuyerIntegration::find_by_id(pool, &integration_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm buyer integrations: {}", e);
        }
    }

    warmed
}

/// Pre-warm qualification configs (1h TTL - configs rarely change)
/// Uses find_by_buyer_ids which takes a slice, so we batch them
async fn pre_warm_qualification_configs(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Get list of buyer_ids with active qualification configs
    match sqlx::query(
        "SELECT DISTINCT buyer_id FROM buyer_qualification_configs WHERE enabled = true AND is_active = true LIMIT 200"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let buyer_ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.get::<uuid::Uuid, _>("buyer_id")).collect();

            // Batch fetch all configs at once
            if !buyer_ids.is_empty() {
                use leadsnebula_core::models::buyer_qualification_config::BuyerQualificationConfig;
                if let Ok(configs_map) = BuyerQualificationConfig::find_by_buyer_ids(pool, &buyer_ids).await {
                    for buyer_id in buyer_ids {
                        let cache_key = format!("qual:buyers:{}", buyer_id);
                        // Trigger cache by calling get_or_insert_with
                        if let Some(config) = configs_map.get(&buyer_id).and_then(|c| c.as_ref()) {
                            let _ = cache
                                .get_or_insert_with(&cache_key, 3600, || async {
                                    Ok::<BuyerQualificationConfig, anyhow::Error>(config.clone())
                                })
                                .await;
                            warmed += 1;
                        }
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm qualification configs: {}", e);
        }
    }

    warmed
}

/// Pre-warm buyer IDs (24h TTL - buyers rarely change)
/// This pre-warms the buyer:id cache used in buyer_router
async fn pre_warm_buyer_ids(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    match sqlx::query(
        "SELECT id FROM buyers WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let buyer_id: uuid::Uuid = row.get("id");
                let cache_key = format!("buyer:id:{}", buyer_id);
                // Trigger cache by calling get_or_insert_with
                let _ = cache
                    .get_or_insert_with(&cache_key, 86400, || async {
                        use leadsnebula_core::models::buyer::Buyer;
                        Buyer::find_by_id(pool, buyer_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm buyer IDs: {}", e);
        }
    }

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
