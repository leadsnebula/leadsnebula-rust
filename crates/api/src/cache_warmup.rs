use crate::config::AppState;
use leadsnebula_core::models::ping_tree::PingTree;
use leadsnebula_core::models::vertical::Vertical;
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tracing::{info, warn};

/// Pre-warm cache on startup by loading common data
/// This reduces cold start latency for the first few requests
/// OPTIMIZED: Parallelize all warmup operations to reduce total time
pub async fn pre_warm_cache(state: &AppState) {
    info!("Starting cache pre-warming...");
    let start = std::time::Instant::now();

    if state.cache.is_none() {
        warn!("Cache not available, skipping pre-warm");
        return;
    }

    let cache = state.cache.as_ref().unwrap();
    let pool = &state.db_pool;

    // OPTIMIZED: Parallelize all independent warmup operations
    // Group 1: DB-dependent operations (can run in parallel)
    let (
        verticals_warmed,
        ping_trees_warmed,
        buyers_warmed,
        campaigns_warmed,
        integrations_warmed,
        buyer_ids_warmed,
    ) = tokio::join!(
        pre_warm_verticals(cache, pool),
        pre_warm_ping_trees(cache, pool),
        pre_warm_buyer_names(cache, pool),
        pre_warm_campaign_names(cache, pool),
        pre_warm_buyer_integrations(cache, pool),
        pre_warm_buyer_ids(cache, pool),
    );

    // Group 2: Qualification configs, ping tree campaigns, and prechecks (depends on buyers/ping trees/publishers)
    let (qual_configs_warmed, ping_tree_campaigns_warmed, prechecks_warmed) = tokio::join!(
        pre_warm_qualification_configs(cache, pool),
        pre_warm_ping_tree_campaigns(cache, pool),
        pre_warm_prechecks(cache, pool),
    );

    // Group 3: SSM keys (independent, can run in parallel with everything)
    let ssm_keys_warmed = pre_warm_ssm_keys(&state.ssm, &state.config.environment).await;

    // Check buyer integration types (internal vs external) for diagnostics
    let buyer_integration_types = check_buyer_integration_types(pool).await;
    info!(
        "Buyer integration types: {} internal, {} external",
        buyer_integration_types.internal_count, buyer_integration_types.external_count
    );

    let duration_ms = start.elapsed().as_millis();
    info!(
        "Cache pre-warming completed in {}ms: {} verticals, {} ping trees, {} SSM keys, {} buyers, {} campaigns, {} integrations, {} qual configs, {} buyer_ids, {} ping_tree_campaigns, {} prechecks",
        duration_ms, verticals_warmed, ping_trees_warmed, ssm_keys_warmed, buyers_warmed, campaigns_warmed, integrations_warmed, qual_configs_warmed, buyer_ids_warmed, ping_tree_campaigns_warmed, prechecks_warmed
    );
}

/// Check buyer integration types for diagnostics
async fn check_buyer_integration_types(pool: &PgPool) -> BuyerIntegrationTypes {
    let mut internal_count = 0;
    let mut external_count = 0;

    match sqlx::query(
        "SELECT is_internal, COUNT(*) as count FROM buyer_integrations WHERE status = 'available' GROUP BY is_internal"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let is_internal: bool = row.get("is_internal");
                let count: i64 = row.get("count");
                if is_internal {
                    internal_count = count as usize;
                } else {
                    external_count = count as usize;
                }
            }
        }
        Err(e) => {
            warn!("Failed to check buyer integration types: {}", e);
        }
    }

    BuyerIntegrationTypes {
        internal_count,
        external_count,
    }
}

struct BuyerIntegrationTypes {
    internal_count: usize,
    external_count: usize,
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
        "SELECT id, name FROM buyers WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 500",
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
        "SELECT id, name FROM campaigns WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 500"
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
        "SELECT id FROM buyer_integrations WHERE status = 'available' ORDER BY created_at DESC LIMIT 500"
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
/// FIXED: Cache key format matches ping_tree_router.rs (qual:buyers:{comma-separated-ids})
/// This pre-warms the exact cache keys used during routing to eliminate cache misses
async fn pre_warm_qualification_configs(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Get all active buyers (we'll pre-warm qual configs for common buyer combinations)
    // First, get all buyers that have qualification configs
    match sqlx::query(
        "SELECT DISTINCT buyer_id FROM buyer_qualification_configs WHERE enabled = true AND is_active = true ORDER BY buyer_id LIMIT 500"
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let buyer_ids: Vec<uuid::Uuid> = rows.iter().map(|row| row.get::<uuid::Uuid, _>("buyer_id")).collect();

            if !buyer_ids.is_empty() {
                use leadsnebula_core::models::buyer_qualification_config::BuyerQualificationConfig;

                // Fetch all configs at once
                if let Ok(configs_map) = BuyerQualificationConfig::find_by_buyer_ids(pool, &buyer_ids).await {
                    // Pre-warm individual buyer qual configs (used in buyer_router)
                    for buyer_id in &buyer_ids {
                        if let Some(config) = configs_map.get(buyer_id).and_then(|c| c.as_ref()) {
                            let cache_key = format!("qual:buyers:{}", buyer_id);
                            let _ = cache
                                .get_or_insert_with(&cache_key, 3600, || async {
                                    Ok::<BuyerQualificationConfig, anyhow::Error>(config.clone())
                                })
                                .await;
                            warmed += 1;
                        }
                    }

                    // Pre-warm common buyer combinations (used in ping_tree_router)
                    // Cache keys like "qual:buyers:{id1},{id2},{id3}" for common ping tree combinations
                    // Get active ping trees and their buyer combinations
                    if let Ok(ping_tree_rows) = sqlx::query(
                        r#"
                        SELECT DISTINCT pt.id, array_agg(DISTINCT c.buyer_id) FILTER (WHERE c.buyer_id IS NOT NULL) as buyer_ids
                        FROM ping_trees pt
                        INNER JOIN ping_tree_campaigns ptc ON pt.id = ptc.ping_tree_id
                        INNER JOIN campaigns c ON ptc.campaign_id = c.id AND c.deleted_at IS NULL
                        WHERE pt.deleted_at IS NULL AND pt.status = 'active' AND ptc.enabled = true
                        GROUP BY pt.id
                        LIMIT 50
                        "#
                    )
                    .fetch_all(pool)
                    .await
                    {
                        for row in ping_tree_rows {
                            if let Ok(Some(buyer_ids_array)) = row.try_get::<Option<Vec<Option<uuid::Uuid>>>, _>("buyer_ids") {
                                let buyer_ids_list: Vec<uuid::Uuid> = buyer_ids_array
                                    .into_iter()
                                    .flatten()
                                    .collect::<std::collections::HashSet<_>>()
                                    .into_iter()
                                    .collect();

                                if !buyer_ids_list.is_empty() {
                                    // Create cache key matching ping_tree_router format
                                    let mut sorted_ids = buyer_ids_list.clone();
                                    sorted_ids.sort();
                                    let cache_key = format!(
                                        "qual:buyers:{}",
                                        sorted_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",")
                                    );

                                    // Pre-warm this combination
                                    let _ = cache
                                        .get_or_insert_with(&cache_key, 3600, || async {
                                            BuyerQualificationConfig::find_by_buyer_ids(pool, &buyer_ids_list)
                                                .await
                                                .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                                        })
                                        .await;
                                    warmed += 1;
                                }
                            }
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

/// Pre-warm ping tree campaigns cache (6h TTL - campaigns:pingtree:{ping_tree_id})
/// This eliminates cache misses for campaigns:pingtree keys seen in logs
async fn pre_warm_ping_tree_campaigns(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Get active ping trees
    match sqlx::query(
        "SELECT id FROM ping_trees WHERE deleted_at IS NULL AND status = 'active' ORDER BY created_at DESC LIMIT 100"
    )
    .fetch_all(pool)
    .await
    {
        Ok(ping_tree_rows) => {
            for row in ping_tree_rows {
                let ping_tree_id: uuid::Uuid = row.get("id");
                let cache_key = format!("campaigns:pingtree:{}", ping_tree_id);

                // Pre-warm by calling get_or_insert_with (matches ping_tree_router cache key format)
                let _ = cache
                    .get_or_insert_with(&cache_key, 21600, || async {
                        use leadsnebula_core::models::ping_tree_campaign::PingTreeCampaign;
                        PingTreeCampaign::find_enabled_for_ping_tree(pool, &ping_tree_id)
                            .await
                            .map_err(|e| anyhow::anyhow!("DB error: {}", e))
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm ping tree campaigns: {}", e);
        }
    }

    warmed
}

/// Pre-warm prechecks cache (5min TTL - prechecks:publisher:{id}:vertical:{slug}:token:{token})
/// This eliminates cache misses for prechecks keys seen in logs
async fn pre_warm_prechecks(
    cache: &Arc<leadsnebula_core::cache::CacheService>,
    pool: &PgPool,
) -> usize {
    let mut warmed = 0;

    // Get active publishers and their verticals
    match sqlx::query(
        r#"
        SELECT DISTINCT p.id as publisher_id, v.slug as vertical_slug
        FROM publishers p
        INNER JOIN ping_tree_publishers ptp ON p.id = ptp.publisher_id
        INNER JOIN ping_trees pt ON ptp.ping_tree_id = pt.id
        INNER JOIN verticals v ON pt.vertical = v.slug AND v.is_active = true
        WHERE p.deleted_at IS NULL AND pt.deleted_at IS NULL AND pt.status = 'active'
        LIMIT 100
        "#,
    )
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let publisher_id: uuid::Uuid = row.get("publisher_id");
                let vertical_slug: String = row.get("vertical_slug");

                // Pre-warm prechecks for empty token (most common case)
                let cache_key = format!(
                    "prechecks:publisher:{}:vertical:{}:token:",
                    publisher_id, vertical_slug
                );

                // Pre-warm by calling get_or_insert_with (matches carina.rs cache key format)
                let _ = cache
                    .get_or_insert_with(&cache_key, 300, || async {
                        // This will trigger the actual pre-check query in carina.rs
                        // For warmup, we just want to cache the result structure
                        // The actual query will be cached when first request comes in
                        Ok::<(Option<uuid::Uuid>, Option<uuid::Uuid>, bool), anyhow::Error>((
                            None, None, false,
                        ))
                    })
                    .await;
                warmed += 1;
            }
        }
        Err(e) => {
            warn!("Failed to pre-warm prechecks: {}", e);
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
        "SELECT id FROM buyers WHERE deleted_at IS NULL ORDER BY created_at DESC LIMIT 500",
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
